//! kona-node consensus client service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use k256::ecdsa::SigningKey;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

pub use cmd::KonaNodeCmdBuilder;

use crate::{
    ExposedPort,
    docker::{
        ContainerPorts, CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping,
        ServiceConfig,
    },
    services::kona_node::cmd::DEFAULT_P2P_PORT,
};

use super::{anvil::AnvilHandler, l2_node::L2NodeRole, op_reth::OpRethHandler};

/// Ethereum Mainnet chain ID.
pub const MAINNET_CHAIN_ID: u64 = 1;
/// Ethereum Sepolia testnet chain ID.
pub const SEPOLIA_CHAIN_ID: u64 = 11155111;

/// Returns true if the chain ID is a known L1 chain (Mainnet or Sepolia).
///
/// Known chains have pre-deployed OPCM contracts and are in kona-node's registry.
/// Unknown chains are treated as local/custom chains that need custom configuration.
pub fn is_known_l1_chain(chain_id: u64) -> bool {
    chain_id == MAINNET_CHAIN_ID || chain_id == SEPOLIA_CHAIN_ID
}

/// Generate an L1 chain config file for local/custom Anvil chains.
///
/// This is needed because kona-node doesn't have custom chain IDs in its registry.
/// The config specifies that all hardforks are activated from genesis.
fn generate_local_l1_config(
    host_config_path: &PathBuf,
    chain_id: u64,
) -> Result<PathBuf, anyhow::Error> {
    let config = json!({
        "chain_id": chain_id,
        "genesis_time": 0,
        "block_time": 2,
        "hardforks": {
            "bedrock": 0,
            "regolith": 0,
            "canyon": 0,
            "delta": 0,
            "ecotone": 0,
            "fjord": 0,
            "granite": 0,
            "holocene": 0,
            "isthmus": 0,
            "merge": 0,
            "shanghai": 0,
            "cancun": 0
        }
    });

    let config_path = host_config_path.join("l1-config.json");
    let config_content =
        serde_json::to_string_pretty(&config).context("Failed to serialize L1 config")?;
    std::fs::write(&config_path, config_content).context("Failed to write L1 config file")?;

    tracing::debug!(path = %config_path.display(), chain_id, "Generated L1 config file for local chain");
    Ok(config_path)
}

/// P2P keypair for kona-node identity.
#[derive(Debug, Clone)]
pub struct P2pKeypair {
    /// Private key (32 bytes hex-encoded, without 0x prefix)
    pub private_key: String,
    /// Node ID derived from the public key (64 bytes hex-encoded, without 0x prefix)
    pub node_id: String,
}

impl P2pKeypair {
    /// Create a P2P keypair from an existing private key.
    ///
    /// # Arguments
    /// * `private_key_hex` - 32-byte private key as hex string (with or without 0x prefix)
    pub fn from_private_key(private_key_hex: &str) -> Result<Self, anyhow::Error> {
        let private_key_hex = private_key_hex
            .strip_prefix("0x")
            .unwrap_or(private_key_hex);

        let private_key_bytes: [u8; 32] = hex::decode(private_key_hex)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Private key must be exactly 32 bytes"))?;

        // Create signing key from private key bytes
        let signing_key = SigningKey::from_bytes(&private_key_bytes.into())
            .map_err(|e| anyhow::anyhow!("Invalid secp256k1 private key: {}", e))?;

        // Get the verifying (public) key
        let verifying_key = signing_key.verifying_key();

        // Get uncompressed public key point (65 bytes: 0x04 prefix + 64 bytes)
        let public_key_point = verifying_key.to_encoded_point(false);
        let public_key_bytes = public_key_point.as_bytes();

        // Node ID is the public key without the 0x04 prefix (64 bytes = 128 hex chars)
        // Skip the first byte (0x04 uncompressed marker)
        let node_id = hex::encode(&public_key_bytes[1..]);
        let private_key = hex::encode(private_key_bytes);

        Ok(Self {
            private_key,
            node_id,
        })
    }

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

    /// Compute an enode URL for this keypair.
    ///
    /// # Arguments
    /// * `hostname` - The hostname or IP address
    /// * `port` - The P2P port
    pub fn to_enode(&self, hostname: &str, port: u16) -> String {
        format!("enode://{}@{}:{}", self.node_id, hostname, port)
    }
}

/// Default ports for kona-node.
pub const DEFAULT_RPC_PORT: u16 = 7545;
pub const DEFAULT_METRICS_PORT: u16 = 7300;

/// Container port configuration for kona-node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KonaNodeContainerPorts {
    pub rpc: u16,
    pub metrics: u16,
    pub p2p: u16,
}

impl Default for KonaNodeContainerPorts {
    fn default() -> Self {
        Self {
            rpc: DEFAULT_RPC_PORT,
            metrics: DEFAULT_METRICS_PORT,
            p2p: DEFAULT_P2P_PORT,
        }
    }
}

/// Bound host port configuration for kona-node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KonaNodeBoundPorts {
    pub rpc: Option<u16>,
    pub metrics: Option<u16>,
    pub p2p: Option<u16>,
}

impl Default for KonaNodeBoundPorts {
    fn default() -> Self {
        Self {
            rpc: Some(0),
            metrics: Some(0),
            p2p: None, // P2P is only exposed internally, not published to host
        }
    }
}

/// Unified port configuration for kona-node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum KonaNodePorts {
    Host { bound_ports: KonaNodeBoundPorts },
    Bridge {
        container_name: String,
        container_ports: KonaNodeContainerPorts,
        bound_ports: KonaNodeBoundPorts,
    },
}

impl KonaNodePorts {
    /// Get the HTTP URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_http_url(&self) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .rpc
                    .ok_or_else(|| anyhow::anyhow!("RPC port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge { container_name, container_ports, .. } => {
                format!("http://{}:{}/", container_name, container_ports.rpc)
            }
        };
        Url::parse(&url_str).context("Failed to parse HTTP URL")
    }

    /// Get the HTTP URL for host access.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_http_url(&self) -> Option<anyhow::Result<Url>> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.rpc.map(|port| {
                    Url::parse(&format!("http://localhost:{}/", port))
                        .context("Failed to parse HTTP URL")
                })
            }
        }
    }
}

/// Configuration for the kona-node consensus client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KonaNodeBuilder {
    /// Docker image configuration for kona-node.
    pub docker_image: DockerImage,
    /// Container name for kona-node.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Unified port configuration.
    pub ports: KonaNodePorts,
    /// L1 slot duration in seconds (block time).
    pub l1_slot_duration: u64,
    /// P2P secret key (32 bytes hex-encoded) for deterministic node identity.
    /// If None, a random key will be generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p2p_secret_key: Option<String>,
    /// Extra arguments to pass to kona-node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

/// Default Docker image for kona-node.
pub const DEFAULT_DOCKER_IMAGE: &str = "kona";
/// Default Docker tag for kona-node.
pub const DEFAULT_DOCKER_TAG: &str = "local";

impl Default for KonaNodeBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-kona-node".to_string(),
            host: "0.0.0.0".to_string(),
            ports: KonaNodePorts::Bridge {
                container_name: "kupcake-kona-node".to_string(),
                container_ports: KonaNodeContainerPorts::default(),
                bound_ports: KonaNodeBoundPorts::default(),
            },
            l1_slot_duration: 12,
            p2p_secret_key: None,
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
    /// P2P keypair for this node.
    pub p2p_keypair: P2pKeypair,
    /// Port information for this container.
    pub ports: KonaNodePorts,
}

impl KonaNodeHandler {
    /// Returns the node ID (public key) for this kona-node.
    ///
    /// This is the 64-byte hex-encoded public key derived from the P2P private key.
    pub fn node_id(&self) -> &str {
        &self.p2p_keypair.node_id
    }

    /// Returns the enode URL for this node.
    ///
    /// kona-node uses enode format for peer discovery and bootstrap nodes.
    pub fn enode(&self) -> String {
        let p2p_port = match &self.ports {
            KonaNodePorts::Host { .. } | KonaNodePorts::Bridge { .. } => DEFAULT_P2P_PORT,
        };
        self.p2p_keypair.to_enode(&self.container_name, p2p_port)
    }

    /// Get the internal RPC URL for container-to-container communication.
    pub fn internal_rpc_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_http_url()
    }

    /// Get the host-accessible RPC URL (if published).
    pub fn host_rpc_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_http_url()
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
    /// * `bootnodes` - List of enode URLs for P2P peer discovery
    /// * `l1_chain_id` - L1 chain ID (used to determine if we need a custom L1 config)
    /// * `conductor_rpc` - Optional conductor RPC URL. If provided, enables conductor control.
    /// * `is_conductor_leader` - Whether this sequencer is the initial Raft leader. Leaders start
    ///   active, while followers start in stopped state waiting for conductor to activate them.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        op_reth_handler: &OpRethHandler,
        role: L2NodeRole,
        jwt_filename: &str,
        bootnodes: &[String],
        l1_chain_id: u64,
        conductor_rpc: Option<&str>,
        is_conductor_leader: bool,
    ) -> Result<KonaNodeHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // Extract ports from self.ports
        let (container_ports, bound_ports) = match &self.ports {
            KonaNodePorts::Host { bound_ports } => (KonaNodeContainerPorts::default(), bound_ports.clone()),
            KonaNodePorts::Bridge { container_ports, bound_ports, .. } => (*container_ports, bound_ports.clone()),
        };

        // Create or use the provided P2P keypair
        let p2p_keypair = match &self.p2p_secret_key {
            Some(key) => P2pKeypair::from_private_key(key)
                .context("Failed to create P2P keypair from provided secret key")?,
            None => P2pKeypair::generate(),
        };

        tracing::debug!(
            container_name = %self.container_name,
            node_id = %p2p_keypair.node_id,
            "Using P2P keypair for kona-node"
        );

        // In host network mode, use 0.0.0.0 for P2P listen IP
        // In bridge mode, use the container name for P2P advertise IP
        let p2p_ip = match &self.ports {
            KonaNodePorts::Host { .. } => "0.0.0.0",
            KonaNodePorts::Bridge { container_name, .. } => container_name.as_str(),
        };

        let mut cmd_builder = KonaNodeCmdBuilder::new(
            anvil_handler.internal_rpc_url()?.to_string(),
            op_reth_handler.internal_authrpc_url()?.to_string(),
            p2p_ip,
            container_config_path.join("rollup.json"),
            container_config_path.join(jwt_filename),
        )
        .mode(role.as_kona_mode())
        .l1_slot_duration(self.l1_slot_duration)
        .rpc_port(container_ports.rpc)
        .metrics(true, container_ports.metrics)
        .discovery(true)
        .bootnodes(bootnodes.to_vec())
        .p2p_priv_key(&p2p_keypair.private_key)
        .extra_args(self.extra_args.clone());

        // For local/custom chains (not Mainnet or Sepolia), generate and use a custom L1 config file
        if !is_known_l1_chain(l1_chain_id) {
            generate_local_l1_config(host_config_path, l1_chain_id)
                .context("Failed to generate L1 config for local chain")?;
            cmd_builder = cmd_builder
                .l1_config_file(container_config_path.join("l1-config.json").display().to_string());
        }

        // Configure conductor control if a conductor RPC URL is provided
        // Leader starts active, followers start stopped waiting for conductor to activate them
        if let Some(conductor_url) = conductor_rpc {
            cmd_builder = cmd_builder.conductor_rpc(conductor_url);

            if is_conductor_leader {
                tracing::info!(
                    conductor_rpc = %conductor_url,
                    container_name = %self.container_name,
                    "Configuring kona-node with conductor control (leader, starting active)"
                );
            } else {
                tracing::info!(
                    conductor_rpc = %conductor_url,
                    container_name = %self.container_name,
                    "Configuring kona-node with conductor control (follower, starting stopped)"
                );
                cmd_builder = cmd_builder.sequencer_stopped(true);
            }
        }

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
            PortMapping::tcp_optional(container_ports.rpc, bound_ports.rpc),
            PortMapping::tcp_optional(container_ports.metrics, bound_ports.metrics),
        ]
        .into_iter()
        .flatten()
        .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .expose(ExposedPort::tcp(container_ports.p2p))
            .expose(ExposedPort::udp(container_ports.p2p))
            .bind(host_config_path, &container_config_path, "rw");

        let service_handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions {
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start kona-node container")?;

        // Build runtime ports with actual bound ports
        let actual_bound_ports = KonaNodeBoundPorts {
            rpc: service_handler.ports.get_tcp_host_port(container_ports.rpc),
            metrics: service_handler.ports.get_tcp_host_port(container_ports.metrics),
            p2p: None, // P2P is only exposed internally, not published to host
        };

        let runtime_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => KonaNodePorts::Host {
                bound_ports: actual_bound_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => KonaNodePorts::Bridge {
                container_name: container_name.clone(),
                container_ports,
                bound_ports: actual_bound_ports,
            },
        };

        let rpc_host_url = runtime_ports.host_http_url();

        tracing::info!(
            container_id = %service_handler.container_id,
            container_name = %service_handler.container_name,
            ?rpc_host_url,
            "kona-node container started"
        );

        Ok(KonaNodeHandler {
            container_id: service_handler.container_id,
            container_name: service_handler.container_name,
            p2p_keypair,
            ports: runtime_ports,
        })
    }
}
