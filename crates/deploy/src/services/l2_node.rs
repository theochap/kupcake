//! L2 Node combining op-reth execution client and kona-node consensus client.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use backon::{ConstantBuilder, Retryable};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    OpConductorBuilder, OpConductorHandler,
    docker::{DockerImage, KupDocker},
    service::KupcakeService,
    services::{
        OpConductorInput, OpRethInput,
        kona_node::{KonaNodeBuilder, KonaNodeHandler, KonaNodeInput, P2pKeypair},
        op_reth::{OpRethBuilder, OpRethHandler},
    },
};

/// Interval between RPC readiness checks.
const RPC_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Wait for an execution client RPC to be ready by polling with `eth_chainId`.
async fn wait_for_execution_rpc_ready(rpc_url: &str, timeout_secs: u64) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("Failed to create HTTP client")?;

    tracing::debug!(rpc_url = %rpc_url, timeout_secs = %timeout_secs, "Waiting for execution RPC to be ready");

    let max_retries = (timeout_secs * 1000) as usize / RPC_POLL_INTERVAL.as_millis() as usize;
    let backoff = ConstantBuilder::default()
        .with_delay(RPC_POLL_INTERVAL)
        .with_max_times(max_retries);

    (|| async {
        let resp = client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_chainId",
                "params": [],
                "id": 1
            }))
            .send()
            .await
            .context("request failed")?;

        let body = resp
            .json::<serde_json::Value>()
            .await
            .context("invalid json")?;
        if body.get("result").is_some() {
            Ok(())
        } else {
            anyhow::bail!("no result in response")
        }
    })
    .retry(backoff)
    .await
    .with_context(|| {
        format!(
            "Timeout waiting for execution RPC at {} to be ready",
            rpc_url
        )
    })?;

    tracing::debug!(rpc_url = %rpc_url, "Execution RPC is ready");
    Ok(())
}

/// Wait for a consensus client RPC (kona-node) to be ready by polling with `optimism_syncStatus`.
async fn wait_for_consensus_rpc_ready(rpc_url: &str, timeout_secs: u64) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("Failed to create HTTP client")?;

    tracing::debug!(rpc_url = %rpc_url, timeout_secs = %timeout_secs, "Waiting for consensus RPC to be ready");

    let max_retries = (timeout_secs * 1000) as usize / RPC_POLL_INTERVAL.as_millis() as usize;
    let backoff = ConstantBuilder::default()
        .with_delay(RPC_POLL_INTERVAL)
        .with_max_times(max_retries);

    (|| async {
        let resp = client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "optimism_syncStatus",
                "params": [],
                "id": 1
            }))
            .send()
            .await
            .context("request failed")?;

        let body = resp
            .json::<serde_json::Value>()
            .await
            .context("invalid json")?;
        // Accept either a result or even an error - as long as we get a JSON-RPC response
        if body.get("result").is_some() || body.get("error").is_some() {
            Ok(())
        } else {
            anyhow::bail!("no JSON-RPC result or error in response")
        }
    })
    .retry(backoff)
    .await
    .with_context(|| {
        format!(
            "Timeout waiting for consensus RPC at {} to be ready",
            rpc_url
        )
    })?;

    tracing::debug!(rpc_url = %rpc_url, "Consensus RPC is ready");
    Ok(())
}

/// Context for op-conductor startup when part of a multi-sequencer setup.
#[derive(Debug, Clone, Copy, Default)]
pub enum ConductorContext {
    /// No conductor for this node (single sequencer or validator).
    #[default]
    None,
    /// This is the Raft leader - bootstrap the cluster.
    Leader {
        /// Index of this sequencer (0-based).
        index: usize,
    },
    /// This is a Raft follower - join existing cluster.
    Follower {
        /// Index of this sequencer (0-based).
        index: usize,
    },
}

/// Role of an L2 node in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
pub enum L2NodeRole {
    /// Sequencer node that produces blocks.
    Sequencer,
    /// Validator node that follows the sequencer.
    #[default]
    Validator,
}

impl L2NodeRole {
    /// Returns the kona-node mode string for this role.
    pub fn as_kona_mode(&self) -> &'static str {
        match self {
            L2NodeRole::Sequencer => "Sequencer",
            L2NodeRole::Validator => "Validator",
        }
    }
}

/// Input parameters for deploying an L2 node (op-reth + kona-node + optional op-conductor).
///
/// Decoupled from handler references — uses raw URLs and keys.
pub struct L2NodeInput {
    /// L1 RPC URL (e.g., Anvil's Docker-internal URL).
    pub l1_rpc_url: String,
    /// L1 RPC URL accessible from the host (used for L1 config generation).
    pub l1_host_url: Option<String>,
    /// Unsafe block signer private key (hex-encoded, without 0x prefix).
    pub unsafe_block_signer_key: String,
    /// Optional URL of the sequencer's op-reth HTTP RPC (required for validators).
    pub sequencer_rpc: Option<Url>,
    /// List of kona-node enodes for P2P discovery.
    pub kona_node_enodes: Vec<String>,
    /// List of op-reth enodes for P2P discovery.
    pub op_reth_enodes: Vec<String>,
    /// L1 chain ID (used to determine if we need a custom L1 config for kona-node).
    pub l1_chain_id: u64,
    /// Context for op-conductor startup (leader, follower, or none).
    pub conductor_context: ConductorContext,
    /// Optional flashblocks relay URL from the sequencer's kona-node.
    pub sequencer_flashblocks_relay_url: Option<Url>,
    /// Optional pre-generated P2P keypair for op-reth.
    /// If None, a random keypair will be generated.
    pub op_reth_p2p_secret_key: Option<String>,
}

/// Configuration for an L2 node (op-reth + kona-node pair).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2NodeBuilder<EL = OpRethBuilder, CL = KonaNodeBuilder, Cond = OpConductorBuilder> {
    /// Role of this node (sequencer or validator).
    pub role: L2NodeRole,
    /// Configuration for op-reth execution client.
    pub op_reth: EL,
    /// Configuration for kona-node consensus client.
    pub kona_node: CL,
    /// Configuration for op-conductor (only for sequencer nodes in multi-sequencer setups).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_conductor: Option<Cond>,
}

impl Default for L2NodeBuilder {
    fn default() -> Self {
        Self {
            role: L2NodeRole::Sequencer,
            op_reth: OpRethBuilder::default(),
            kona_node: KonaNodeBuilder::default(),
            op_conductor: None,
        }
    }
}

impl L2NodeBuilder {
    /// Create a new L2 node builder with the given role.
    pub fn new(role: L2NodeRole) -> Self {
        Self {
            role,
            ..Default::default()
        }
    }

    /// Create a sequencer node builder.
    pub fn sequencer() -> Self {
        Self::new(L2NodeRole::Sequencer)
    }

    /// Create a validator node builder.
    pub fn validator() -> Self {
        Self::new(L2NodeRole::Validator)
    }
}

impl<EL, CL, Cond> L2NodeBuilder<EL, CL, Cond> {
    /// Attach an op-conductor configuration to this node.
    pub fn with_conductor(mut self, conductor: Cond) -> Self {
        self.op_conductor = Some(conductor);
        self
    }
}

impl L2NodeBuilder {
    /// Set a unique suffix for container names to avoid conflicts.
    pub fn with_name_suffix(mut self, suffix: &str) -> Self {
        self.op_reth.container_name = format!("{}-{}", self.op_reth.container_name, suffix);
        self.kona_node.container_name = format!("{}-{}", self.kona_node.container_name, suffix);
        if let Some(ref mut conductor) = self.op_conductor {
            conductor.container_name = format!("{}-{}", conductor.container_name, suffix);
        }
        self
    }
}

impl<EL, CL, Cond> KupcakeService for L2NodeBuilder<EL, CL, Cond>
where
    EL: KupcakeService<Input = OpRethInput, Output = OpRethHandler>,
    CL: KupcakeService<Input = KonaNodeInput, Output = KonaNodeHandler>,
    Cond: KupcakeService<Input = OpConductorInput, Output = OpConductorHandler>,
{
    type Input = L2NodeInput;
    type Output = L2NodeHandler;

    fn container_name(&self) -> &str {
        self.op_reth.container_name()
    }

    fn docker_image(&self) -> &DockerImage {
        self.op_reth.docker_image()
    }

    async fn deploy<'a>(
        &'a self,
        docker: &'a mut KupDocker,
        host_config_path: &'a Path,
        input: L2NodeInput,
    ) -> Result<L2NodeHandler, anyhow::Error> {
        // Generate a unique JWT secret for this node pair
        let node_id = self.op_reth.container_name();
        let jwt_secret = {
            use rand::Rng;
            let mut rng = rand::rng();
            let secret: [u8; 32] = rng.random();
            hex::encode(secret)
        };
        let jwt_filename = format!("jwt-{}.hex", node_id);
        let jwt_path = host_config_path.join(&jwt_filename);

        tokio::fs::write(&jwt_path, &jwt_secret)
            .await
            .context(format!(
                "Failed to write JWT secret file: {}",
                jwt_path.display()
            ))?;

        tracing::debug!(path = ?jwt_path, %node_id, "JWT secret written for L2 node");

        // Use persisted P2P keypair if available, otherwise generate fresh
        let op_reth_p2p_keypair = match &input.op_reth_p2p_secret_key {
            Some(key) => P2pKeypair::from_private_key(key)
                .context("Failed to create P2P keypair from op-reth p2p_secret_key")?,
            None => P2pKeypair::generate(),
        };

        // Start op-reth first, passing existing op-reth enodes as bootnodes
        let op_reth_handler = self
            .op_reth
            .deploy(
                docker,
                host_config_path,
                OpRethInput {
                    sequencer_rpc: input.sequencer_rpc.clone(),
                    jwt_filename: jwt_filename.clone(),
                    bootnodes: input.op_reth_enodes,
                    p2p_keypair: op_reth_p2p_keypair,
                },
            )
            .await?;

        let op_reth_enode = op_reth_handler.enode();
        tracing::info!(
            container_name = %op_reth_handler.container_name,
            enode = %op_reth_enode,
            "op-reth enode computed"
        );

        // Pre-compute conductor RPC URL if conductor is configured
        let conductor_rpc_url = self.op_conductor.as_ref().map(|c| {
            format!(
                "http://{}:{}/",
                c.container_name(),
                super::op_conductor::DEFAULT_RPC_PORT
            )
        });

        // Determine if this sequencer is the Raft leader
        let is_conductor_leader =
            matches!(input.conductor_context, ConductorContext::Leader { .. });

        // Determine the flashblocks builder URL for kona-node
        let flashblocks_builder_url = match self.role {
            L2NodeRole::Sequencer => op_reth_handler
                .flashblocks_ws_url
                .as_ref()
                .map(|u| u.to_string()),
            L2NodeRole::Validator => input
                .sequencer_flashblocks_relay_url
                .as_ref()
                .map(|u| u.to_string()),
        };

        // When flashblocks is enabled, wait for op-rbuilder's HTTP RPC before starting kona-node
        if op_reth_handler.flashblocks_ws_url.is_some() {
            let wait_url = op_reth_handler
                .http_host_url
                .as_ref()
                .map(|u| u.as_str())
                .unwrap_or(op_reth_handler.http_rpc_url.as_str());
            tracing::info!(
                rpc_url = %wait_url,
                container_name = %op_reth_handler.container_name,
                "Waiting for op-rbuilder HTTP RPC before starting kona-node (flashblocks mode)..."
            );
            wait_for_execution_rpc_ready(wait_url, 120)
                .await
                .context("op-rbuilder RPC not ready before kona-node startup")?;
        }

        // Start kona-node with decoupled input
        let kona_node_handler = self
            .kona_node
            .deploy(
                docker,
                host_config_path,
                KonaNodeInput {
                    l1_rpc_url: input.l1_rpc_url,
                    l1_host_url: input.l1_host_url,
                    authrpc_url: op_reth_handler.authrpc_url.to_string(),
                    unsafe_block_signer_key: input.unsafe_block_signer_key,
                    role: self.role,
                    jwt_filename,
                    bootnodes: input.kona_node_enodes,
                    l1_chain_id: input.l1_chain_id,
                    conductor_rpc: conductor_rpc_url,
                    is_conductor_leader,
                    flashblocks_builder_url,
                },
            )
            .await?;

        let kona_node_enode = kona_node_handler.enode();
        tracing::info!(
            container_name = %kona_node_handler.container_name,
            enode = %kona_node_enode,
            "kona-node enode computed"
        );

        // Wait for both RPCs before starting conductor
        if self.op_conductor.is_some() {
            let op_reth_wait_url = op_reth_handler
                .http_host_url
                .as_ref()
                .map(|u| u.as_str())
                .unwrap_or(op_reth_handler.http_rpc_url.as_str());
            tracing::info!(
                rpc_url = %op_reth_wait_url,
                "Waiting for op-reth RPC to be ready before starting conductor..."
            );
            wait_for_execution_rpc_ready(op_reth_wait_url, 30)
                .await
                .context("op-reth RPC not ready in time for conductor startup")?;

            let kona_wait_url = kona_node_handler
                .rpc_host_url
                .as_ref()
                .map(|u| u.as_str())
                .unwrap_or(kona_node_handler.rpc_url.as_str());
            tracing::info!(
                rpc_url = %kona_wait_url,
                "Waiting for kona-node RPC to be ready before starting conductor..."
            );
            wait_for_consensus_rpc_ready(kona_wait_url, 30)
                .await
                .context("kona-node RPC not ready in time for conductor startup")?;
        }

        let op_conductor = match (&self.op_conductor, input.conductor_context) {
            (Some(conductor_config), ConductorContext::Leader { index }) => {
                let server_id = format!("sequencer-{}", index);
                tracing::info!(
                    server_id = %server_id,
                    container_name = %conductor_config.container_name(),
                    "Starting op-conductor as Raft leader (after EL and CL)..."
                );
                Some(
                    conductor_config
                        .deploy(
                            docker,
                            host_config_path,
                            OpConductorInput {
                                server_id,
                                execution_rpc_url: op_reth_handler.http_rpc_url.to_string(),
                                kona_node_rpc_url: kona_node_handler.rpc_url.to_string(),
                                bootstrap: true,
                            },
                        )
                        .await
                        .context("Failed to start op-conductor leader")?,
                )
            }
            (Some(conductor_config), ConductorContext::Follower { index }) => {
                let server_id = format!("sequencer-{}", index);
                tracing::info!(
                    server_id = %server_id,
                    container_name = %conductor_config.container_name(),
                    "Starting op-conductor as Raft follower (after EL and CL)..."
                );
                Some(
                    conductor_config
                        .deploy(
                            docker,
                            host_config_path,
                            OpConductorInput {
                                server_id,
                                execution_rpc_url: op_reth_handler.http_rpc_url.to_string(),
                                kona_node_rpc_url: kona_node_handler.rpc_url.to_string(),
                                bootstrap: false,
                            },
                        )
                        .await
                        .context("Failed to start op-conductor follower")?,
                )
            }
            _ => None,
        };

        Ok(L2NodeHandler {
            role: self.role,
            op_reth: op_reth_handler,
            kona_node: kona_node_handler,
            op_conductor,
        })
    }
}

/// Handler for a running L2 node (op-reth + kona-node pair).
pub struct L2NodeHandler {
    /// Role of this node.
    pub role: L2NodeRole,
    /// Handler for the op-reth execution client.
    pub op_reth: OpRethHandler,
    /// Handler for the kona-node consensus client.
    pub kona_node: KonaNodeHandler,
    /// Handler for the op-conductor instance (only present if this is a sequencer).
    pub op_conductor: Option<OpConductorHandler>,
}

impl L2NodeHandler {
    /// Returns true if this is a sequencer node.
    pub fn is_sequencer(&self) -> bool {
        self.role == L2NodeRole::Sequencer
    }

    /// Returns true if this is a validator node.
    pub fn is_validator(&self) -> bool {
        self.role == L2NodeRole::Validator
    }
}
