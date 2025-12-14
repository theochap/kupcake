//! kona-node consensus client service.

mod cmd;

use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use k256::ecdsa::SigningKey;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::KonaNodeCmdBuilder;

use crate::{
    docker::{CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping, ServiceConfig},
    services::kona_node::cmd::DEFAULT_P2P_PORT,
};

use super::{anvil::AnvilHandler, l2_node::L2NodeRole, op_reth::OpRethHandler};

/// P2P keypair for kona-node identity.
#[derive(Debug, Clone)]
pub struct P2pKeypair {
    /// Private key (32 bytes hex-encoded, without 0x prefix)
    pub private_key: String,
    /// Node ID derived from the public key (64 bytes hex-encoded, without 0x prefix)
    pub node_id: String,
}

impl P2pKeypair {
    /// Generate a new random P2P keypair.
    pub fn generate() -> Self {
        use rand::Rng;

        // Generate 32 random bytes for the private key
        let mut rng = rand::rng();
        let private_key_bytes: [u8; 32] = rng.random();

        // Create signing key from private key bytes
        let signing_key = SigningKey::from_bytes(&private_key_bytes.into())
            .expect("32 bytes is a valid secp256k1 private key");

        // Get the verifying (public) key
        let verifying_key = signing_key.verifying_key();

        // Get uncompressed public key point (65 bytes: 0x04 prefix + 64 bytes)
        let public_key_point = verifying_key.to_encoded_point(false);
        let public_key_bytes = public_key_point.as_bytes();

        // Node ID is the public key without the 0x04 prefix (64 bytes = 128 hex chars)
        // Skip the first byte (0x04 uncompressed marker)
        let node_id = hex::encode(&public_key_bytes[1..]);
        let private_key = hex::encode(private_key_bytes);

        Self {
            private_key,
            node_id,
        }
    }
}

/// Default ports for kona-node.
pub const DEFAULT_RPC_PORT: u16 = 7545;
pub const DEFAULT_METRICS_PORT: u16 = 7300;

/// Configuration for the kona-node consensus client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KonaNodeBuilder {
    /// Docker image configuration for kona-node.
    pub docker_image: DockerImage,
    /// Container name for kona-node.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Port for the kona-node RPC server (container port).
    pub rpc_port: u16,
    /// Port for metrics (container port).
    pub metrics_port: u16,
    /// Host port for RPC. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_host_port: Option<u16>,
    /// Host port for metrics. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_host_port: Option<u16>,
    /// L1 slot duration in seconds (block time).
    pub l1_slot_duration: u64,
    /// Extra arguments to pass to kona-node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

/// Default Docker image for kona-node.
pub const DEFAULT_DOCKER_IMAGE: &str = "kona-node";
/// Default Docker tag for kona-node.
pub const DEFAULT_DOCKER_TAG: &str = "local";

impl Default for KonaNodeBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-kona-node".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            rpc_host_port: Some(0),
            metrics_host_port: Some(0),
            l1_slot_duration: 12,
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running kona-node instance.
pub struct KonaNodeHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// P2P port for peer discovery.
    pub p2p_port: u16,
    /// The RPC URL for the kona-node (internal Docker network).
    pub rpc_url: Url,
    /// The RPC URL accessible from host (if published). None if not published.
    pub rpc_host_url: Option<Url>,
    /// The metrics URL accessible from host (if published). None if not published.
    pub metrics_host_url: Option<Url>,
}

/// Foundry Docker image used for cast commands.
const FOUNDRY_IMAGE: &str = "ghcr.io/foundry-rs/foundry";
const FOUNDRY_TAG: &str = "latest";

/// Response from `opp2p_self` RPC endpoint.
#[derive(Debug, Deserialize)]
struct PeerENR {
    /// The ENR (Ethereum Node Record) for the node.
    #[serde(rename = "ENR")]
    enr: Option<String>,
}

impl KonaNodeHandler {
    /// Query the node's ENR from the `opp2p_self` RPC endpoint.
    ///
    /// This uses cast inside a Docker container connected to the same network
    /// to query the internal RPC URL.
    ///
    /// Retries with exponential backoff since the node may not be ready
    /// to respond immediately after starting.
    pub async fn fetch_enr(&self, docker: &mut KupDocker) -> Result<String, anyhow::Error> {
        let rpc_url = self.rpc_url.to_string();

        // Retry with exponential backoff
        let max_retries = 10;
        let mut delay = Duration::from_millis(500);
        let max_delay = Duration::from_secs(5);

        for attempt in 1..=max_retries {
            let config = ServiceConfig::new(DockerImage::new(FOUNDRY_IMAGE, FOUNDRY_TAG))
                .cmd(vec![
                    "rpc".to_string(),
                    "opp2p_self".to_string(),
                    "--rpc-url".to_string(),
                    rpc_url.clone(),
                ])
                .entrypoint(vec!["cast".to_string()])
                .env(vec!["FOUNDRY_DISABLE_NIGHTLY_WARNING=1".to_string()]);

            match docker.run_command(config).await {
                Ok(output) => {
                    let peer_info: PeerENR = serde_json::from_str(&output)
                        .context("Failed to parse opp2p_self response")?;

                    if let Some(enr) = peer_info.enr {
                        return Ok(enr);
                    }
                    return Err(anyhow::anyhow!("Node returned empty ENR"));
                }
                Err(e) => {
                    if attempt == max_retries {
                        return Err(e).context("Failed to fetch ENR after max retries");
                    }
                    tracing::debug!(
                        attempt,
                        max_retries,
                        error = %e,
                        delay_ms = delay.as_millis(),
                        "Failed to fetch ENR, retrying..."
                    );
                    tokio::time::sleep(delay).await;
                    delay = std::cmp::min(delay * 2, max_delay);
                }
            }
        }

        unreachable!()
    }
}

impl KonaNodeBuilder {
    /// Start the kona-node consensus client.
    ///
    /// # Arguments
    /// * `docker` - Docker client
    /// * `host_config_path` - Path on host for config files
    /// * `anvil_handler` - Handler for the L1 Anvil instance
    /// * `op_reth_handler` - Handler for the paired op-reth instance
    /// * `role` - Role of this node (sequencer or validator)
    /// * `jwt_filename` - The JWT secret filename (shared with op-reth)
    /// * `bootnodes` - List of ENR strings for P2P peer discovery
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        op_reth_handler: &OpRethHandler,
        role: L2NodeRole,
        jwt_filename: &str,
        bootnodes: &[String],
    ) -> Result<KonaNodeHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // Generate a P2P keypair for this node
        let p2p_keypair = P2pKeypair::generate();

        tracing::debug!(
            container_name = %self.container_name,
            node_id = %p2p_keypair.node_id,
            "Generated P2P keypair for kona-node"
        );

        let mut cmd_builder = KonaNodeCmdBuilder::new(
            anvil_handler.l1_rpc_url.to_string(),
            op_reth_handler.authrpc_url.to_string(),
            self.container_name.clone(),
            container_config_path.join("rollup.json"),
            container_config_path.join(jwt_filename),
        )
        .mode(role.as_kona_mode())
        .l1_slot_duration(self.l1_slot_duration)
        .rpc_port(self.rpc_port)
        .metrics(true, self.metrics_port)
        .discovery(true)
        .bootnodes(bootnodes.to_vec())
        .p2p_priv_key(&p2p_keypair.private_key)
        .extra_args(self.extra_args.clone());

        cmd_builder = cmd_builder.unsafe_block_signer_key(
            anvil_handler
                .accounts
                .unsafe_block_signer
                .private_key
                .clone(),
        );

        let cmd = cmd_builder.build();

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(self.rpc_port, self.rpc_host_port),
            PortMapping::tcp_optional(self.metrics_port, self.metrics_host_port),
            PortMapping::tcp_optional(DEFAULT_P2P_PORT, Some(0)),
        ]
        .into_iter()
        .flatten()
        .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .bind(host_config_path, &container_config_path, "rw");

        let handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions {
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start kona-node container")?;

        // Build internal Docker network URL
        let rpc_url = KupDocker::build_http_url(&handler.container_name, self.rpc_port)?;

        // Build host-accessible URLs from bound ports
        let rpc_host_url = handler
            .get_tcp_host_port(self.rpc_port)
            .map(|port| Url::parse(&format!("http://localhost:{}/", port)))
            .transpose()
            .context("Failed to build RPC host URL")?;

        let metrics_host_url = handler
            .get_tcp_host_port(self.metrics_port)
            .map(|port| Url::parse(&format!("http://localhost:{}/", port)))
            .transpose()
            .context("Failed to build metrics host URL")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            ?rpc_host_url,
            ?metrics_host_url,
            "kona-node container started"
        );

        Ok(KonaNodeHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            rpc_url,
            rpc_host_url,
            metrics_host_url,
            p2p_port: DEFAULT_P2P_PORT,
        })
    }
}
