//! L2 Node combining op-reth execution client and kona-node consensus client.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{OpConductorBuilder, OpConductorHandler, docker::KupDocker};

/// Wait for an execution client RPC to be ready by polling with `eth_chainId`.
async fn wait_for_execution_rpc_ready(rpc_url: &str, timeout_secs: u64) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let max_duration = Duration::from_secs(timeout_secs);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("Failed to create HTTP client")?;

    tracing::debug!(rpc_url = %rpc_url, timeout_secs = %timeout_secs, "Waiting for execution RPC to be ready");

    loop {
        if start.elapsed() > max_duration {
            anyhow::bail!("Timeout waiting for execution RPC at {} to be ready", rpc_url);
        }

        let response = client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_chainId",
                "params": [],
                "id": 1
            }))
            .send()
            .await;

        match response {
            Ok(resp) => {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if body.get("result").is_some() {
                        tracing::debug!(rpc_url = %rpc_url, "Execution RPC is ready");
                        return Ok(());
                    }
                }
            }
            Err(_) => {}
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Wait for a consensus client RPC (kona-node) to be ready by polling with `optimism_syncStatus`.
async fn wait_for_consensus_rpc_ready(rpc_url: &str, timeout_secs: u64) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let max_duration = Duration::from_secs(timeout_secs);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("Failed to create HTTP client")?;

    tracing::debug!(rpc_url = %rpc_url, timeout_secs = %timeout_secs, "Waiting for consensus RPC to be ready");

    loop {
        if start.elapsed() > max_duration {
            anyhow::bail!(
                "Timeout waiting for consensus RPC at {} to be ready",
                rpc_url
            );
        }

        let response = client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "optimism_syncStatus",
                "params": [],
                "id": 1
            }))
            .send()
            .await;

        match response {
            Ok(resp) => {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    // Accept either a result or even an error - as long as we get a JSON-RPC response
                    if body.get("result").is_some() || body.get("error").is_some() {
                        tracing::debug!(rpc_url = %rpc_url, "Consensus RPC is ready");
                        return Ok(());
                    }
                }
            }
            Err(_) => {}
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

use super::{
    anvil::AnvilHandler,
    kona_node::{KonaNodeBuilder, KonaNodeHandler},
    op_reth::{OpRethBuilder, OpRethHandler},
};

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

/// Configuration for an L2 node (op-reth + kona-node pair).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2NodeBuilder {
    /// Role of this node (sequencer or validator).
    pub role: L2NodeRole,
    /// Configuration for op-reth execution client.
    pub op_reth: OpRethBuilder,
    /// Configuration for kona-node consensus client.
    pub kona_node: KonaNodeBuilder,
    /// Configuration for op-conductor (only for sequencer nodes in multi-sequencer setups).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_conductor: Option<OpConductorBuilder>,
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

    /// Set a unique suffix for container names to avoid conflicts.
    pub fn with_name_suffix(mut self, suffix: &str) -> Self {
        self.op_reth.container_name = format!("{}-{}", self.op_reth.container_name, suffix);
        self.kona_node.container_name = format!("{}-{}", self.kona_node.container_name, suffix);
        if let Some(ref mut conductor) = self.op_conductor {
            conductor.container_name = format!("{}-{}", conductor.container_name, suffix);
        }
        self
    }

    /// Attach an op-conductor configuration to this node.
    pub fn with_conductor(mut self, conductor: OpConductorBuilder) -> Self {
        self.op_conductor = Some(conductor);
        self
    }

    /// Generate a JWT secret for authenticated communication between op-reth and kona-node.
    fn generate_jwt_secret() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let secret: [u8; 32] = rng.random();
        hex::encode(secret)
    }

    /// Write a JWT secret to a file for this node pair.
    async fn write_jwt_secret(
        host_config_path: &PathBuf,
        node_id: &str,
    ) -> Result<String, anyhow::Error> {
        let jwt_secret = Self::generate_jwt_secret();
        let jwt_filename = format!("jwt-{}.hex", node_id);
        let jwt_path = host_config_path.join(&jwt_filename);

        tokio::fs::write(&jwt_path, &jwt_secret)
            .await
            .context(format!(
                "Failed to write JWT secret file: {}",
                jwt_path.display()
            ))?;

        tracing::debug!(path = ?jwt_path, node_id, "JWT secret written for L2 node");
        Ok(jwt_filename)
    }

    /// Start the L2 node (op-reth + kona-node + optional op-conductor).
    ///
    /// For sequencer nodes, this starts with block production enabled.
    /// For validator nodes, this connects to the sequencer for block syncing.
    ///
    /// After starting, this adds the node's enodes to the peer lists
    /// so that subsequent nodes can use them as bootnodes.
    ///
    /// # Arguments
    /// * `docker` - Docker client
    /// * `host_config_path` - Path on host for config files
    /// * `anvil_handler` - Handler for the L1 Anvil instance
    /// * `sequencer_rpc` - Optional URL of the sequencer's kona-node RPC (required for validators)
    /// * `kona_node_enodes` - Mutable list of kona-node enodes for P2P discovery. The node's enode will be added after startup.
    /// * `op_reth_enodes` - Mutable list of op-reth enodes for P2P discovery. The node's enode will be added after startup.
    /// * `l1_chain_id` - L1 chain ID (used to determine if we need a custom L1 config for kona-node)
    /// * `conductor_context` - Context for op-conductor startup (leader, follower, or none).
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        sequencer_rpc: Option<&Url>,
        kona_node_enodes: &mut Vec<String>,
        op_reth_enodes: &mut Vec<String>,
        l1_chain_id: u64,
        conductor_context: ConductorContext,
    ) -> Result<L2NodeHandler, anyhow::Error> {
        // Generate a unique JWT secret for this node pair
        // Use the op-reth container name as the node ID for uniqueness
        let jwt_filename =
            Self::write_jwt_secret(host_config_path, &self.op_reth.container_name).await?;

        // Start op-reth first, passing existing op-reth enodes as bootnodes
        let op_reth_handler = self
            .op_reth
            .start(
                docker,
                host_config_path,
                sequencer_rpc,
                &jwt_filename,
                op_reth_enodes,
            )
            .await?;

        // Add op-reth's precomputed enode to the peer list for subsequent nodes
        let op_reth_enode = op_reth_handler.enode();
        tracing::info!(
            container_name = %op_reth_handler.container_name,
            enode = %op_reth_enode,
            "Added op-reth enode to peer list"
        );
        op_reth_enodes.push(op_reth_enode);

        // Pre-compute conductor RPC URL if conductor is configured
        // kona-node needs this at startup to enable conductor control mode
        // The conductor will be started after kona-node, but kona-node needs the URL now
        let conductor_rpc_url = self.op_conductor.as_ref().map(|c| {
            format!("http://{}:{}/", c.container_name, c.rpc_port)
        });

        // Determine if this sequencer is the Raft leader (first sequencer starts active)
        let is_conductor_leader = matches!(conductor_context, ConductorContext::Leader { .. });

        // Start kona-node with conductor RPC URL (if configured)
        // kona-node must have --conductor.rpc set for conductor to recognize it
        let kona_node_handler = self
            .kona_node
            .start(
                docker,
                host_config_path,
                anvil_handler,
                &op_reth_handler,
                self.role,
                &jwt_filename,
                kona_node_enodes,
                l1_chain_id,
                conductor_rpc_url.as_deref(),
                is_conductor_leader,
            )
            .await?;

        // Add kona-node's precomputed enode to the peer list for subsequent nodes
        let kona_node_enode = kona_node_handler.enode();
        tracing::info!(
            container_name = %kona_node_handler.container_name,
            enode = %kona_node_enode,
            "Added kona-node enode to peer list"
        );
        kona_node_enodes.push(kona_node_enode);

        // Start op-conductor AFTER both op-reth and kona-node are running
        // Conductor needs to connect to both EL (op-reth) and CL (kona-node) RPCs on startup
        // Wait for both RPCs to be ready before starting conductor (conductor connects immediately)
        // Use the host-accessible URLs since we're running outside Docker
        if self.op_conductor.is_some() {
            // Wait for op-reth execution RPC
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

            // Wait for kona-node consensus RPC
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

        let op_conductor = match (&self.op_conductor, conductor_context) {
            (Some(conductor_config), ConductorContext::Leader { index }) => {
                let server_id = format!("sequencer-{}", index);
                tracing::info!(
                    server_id = %server_id,
                    container_name = %conductor_config.container_name,
                    "Starting op-conductor as Raft leader (after EL and CL)..."
                );
                Some(
                    conductor_config
                        .start_leader(
                            docker,
                            host_config_path,
                            &server_id,
                            &op_reth_handler,
                            kona_node_handler.rpc_url.as_str(),
                        )
                        .await
                        .context("Failed to start op-conductor leader")?,
                )
            }
            (Some(conductor_config), ConductorContext::Follower { index }) => {
                let server_id = format!("sequencer-{}", index);
                tracing::info!(
                    server_id = %server_id,
                    container_name = %conductor_config.container_name,
                    "Starting op-conductor as Raft follower (after EL and CL)..."
                );
                Some(
                    conductor_config
                        .start_follower(
                            docker,
                            host_config_path,
                            &server_id,
                            &op_reth_handler,
                            kona_node_handler.rpc_url.as_str(),
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
#[derive(Clone)]
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
