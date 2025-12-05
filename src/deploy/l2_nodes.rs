//! L2 nodes deployment for the OP Stack using kona-node and op-reth.

use std::{collections::HashMap, path::PathBuf};

use anyhow::Context;
use bollard::{
    container::Config,
    secret::{HostConfig, PortBinding},
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::deploy::{
    anvil::AnvilHandler,
    docker::{CreateAndStartContainerOptions, KupDocker},
    fs::FsHandler,
};

/// Default ports for L2 node components.
pub const DEFAULT_OP_RETH_HTTP_PORT: u16 = 9545;
pub const DEFAULT_OP_RETH_WS_PORT: u16 = 9546;
pub const DEFAULT_OP_RETH_AUTHRPC_PORT: u16 = 9551;
pub const DEFAULT_OP_RETH_DISCOVERY_PORT: u16 = 30303;
pub const DEFAULT_OP_RETH_METRICS_PORT: u16 = 9001;

pub const DEFAULT_KONA_NODE_RPC_PORT: u16 = 7545;
pub const DEFAULT_KONA_NODE_METRICS_PORT: u16 = 7300;

pub const DEFAULT_OP_BATCHER_RPC_PORT: u16 = 8548;
pub const DEFAULT_OP_BATCHER_METRICS_PORT: u16 = 7301;

pub const DEFAULT_OP_PROPOSER_RPC_PORT: u16 = 8560;
pub const DEFAULT_OP_PROPOSER_METRICS_PORT: u16 = 7302;

pub const DEFAULT_OP_CHALLENGER_RPC_PORT: u16 = 8561;
pub const DEFAULT_OP_CHALLENGER_METRICS_PORT: u16 = 7303;

/// Configuration for the op-reth execution client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpRethConfig {
    /// Container name for op-reth.
    pub container_name: String,

    /// Host for the HTTP RPC endpoint.
    pub host: String,

    /// Port for the HTTP JSON-RPC server.
    pub http_port: u16,

    /// Port for the WebSocket JSON-RPC server.
    pub ws_port: u16,

    /// Port for the authenticated Engine API (used by kona-node).
    pub authrpc_port: u16,

    /// Port for P2P discovery.
    pub discovery_port: u16,

    /// Port for metrics.
    pub metrics_port: u16,

    /// Extra arguments to pass to op-reth.
    pub extra_args: Vec<String>,
}

impl Default for OpRethConfig {
    fn default() -> Self {
        Self {
            container_name: "kupcake-op-reth".to_string(),
            host: "0.0.0.0".to_string(),
            http_port: DEFAULT_OP_RETH_HTTP_PORT,
            ws_port: DEFAULT_OP_RETH_WS_PORT,
            authrpc_port: DEFAULT_OP_RETH_AUTHRPC_PORT,
            discovery_port: DEFAULT_OP_RETH_DISCOVERY_PORT,
            metrics_port: DEFAULT_OP_RETH_METRICS_PORT,
            extra_args: Vec::new(),
        }
    }
}

/// Configuration for the kona-node consensus client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KonaNodeConfig {
    /// Container name for kona-node.
    pub container_name: String,

    /// Host for the RPC endpoint.
    pub host: String,

    /// Port for the kona-node RPC server.
    pub rpc_port: u16,

    /// Port for metrics.
    pub metrics_port: u16,

    /// Extra arguments to pass to kona-node.
    pub extra_args: Vec<String>,
}

impl Default for KonaNodeConfig {
    fn default() -> Self {
        Self {
            container_name: "kupcake-kona-node".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_KONA_NODE_RPC_PORT,
            metrics_port: DEFAULT_KONA_NODE_METRICS_PORT,
            extra_args: Vec::new(),
        }
    }
}

/// Configuration for the op-batcher component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpBatcherConfig {
    /// Container name for op-batcher.
    pub container_name: String,

    /// Host for the RPC endpoint.
    pub host: String,

    /// Port for the op-batcher RPC server.
    pub rpc_port: u16,

    /// Port for metrics.
    pub metrics_port: u16,

    /// Max L1 tx size in bytes (default 120000).
    pub max_l1_tx_size_bytes: u64,

    /// Target number of frames per channel.
    pub target_num_frames: u64,

    /// Sub-safety margin (number of L1 blocks).
    pub sub_safety_margin: u64,

    /// Batch submission interval.
    pub poll_interval: String,

    /// Extra arguments to pass to op-batcher.
    pub extra_args: Vec<String>,
}

impl Default for OpBatcherConfig {
    fn default() -> Self {
        Self {
            container_name: "kupcake-op-batcher".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_OP_BATCHER_RPC_PORT,
            metrics_port: DEFAULT_OP_BATCHER_METRICS_PORT,
            max_l1_tx_size_bytes: 120000,
            target_num_frames: 1,
            sub_safety_margin: 10,
            poll_interval: "1s".to_string(),
            extra_args: Vec::new(),
        }
    }
}

/// Configuration for the op-proposer component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpProposerConfig {
    /// Container name for op-proposer.
    pub container_name: String,

    /// Host for the RPC endpoint.
    pub host: String,

    /// Port for the op-proposer RPC server.
    pub rpc_port: u16,

    /// Port for metrics.
    pub metrics_port: u16,

    /// Proposal interval.
    pub proposal_interval: String,

    /// Extra arguments to pass to op-proposer.
    pub extra_args: Vec<String>,
}

impl Default for OpProposerConfig {
    fn default() -> Self {
        Self {
            container_name: "kupcake-op-proposer".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_OP_PROPOSER_RPC_PORT,
            metrics_port: DEFAULT_OP_PROPOSER_METRICS_PORT,
            proposal_interval: "12s".to_string(),
            extra_args: Vec::new(),
        }
    }
}

/// Configuration for the op-challenger component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpChallengerConfig {
    /// Container name for op-challenger.
    pub container_name: String,

    /// Host for the RPC endpoint.
    pub host: String,

    /// Port for the op-challenger RPC server.
    pub rpc_port: u16,

    /// Port for metrics.
    pub metrics_port: u16,

    /// Extra arguments to pass to op-challenger.
    pub extra_args: Vec<String>,
}

impl Default for OpChallengerConfig {
    fn default() -> Self {
        Self {
            container_name: "kupcake-op-challenger".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_OP_CHALLENGER_RPC_PORT,
            metrics_port: DEFAULT_OP_CHALLENGER_METRICS_PORT,
            extra_args: Vec::new(),
        }
    }
}

/// Combined configuration for all L2 node components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2NodesConfig {
    /// Configuration for op-reth execution client.
    pub op_reth: OpRethConfig,

    /// Configuration for kona-node consensus client.
    pub kona_node: KonaNodeConfig,

    /// Configuration for op-batcher.
    pub op_batcher: OpBatcherConfig,

    /// Configuration for op-proposer.
    pub op_proposer: OpProposerConfig,

    /// Configuration for op-challenger.
    pub op_challenger: OpChallengerConfig,
}

impl Default for L2NodesConfig {
    fn default() -> Self {
        Self {
            op_reth: OpRethConfig::default(),
            kona_node: KonaNodeConfig::default(),
            op_batcher: OpBatcherConfig::default(),
            op_proposer: OpProposerConfig::default(),
            op_challenger: OpChallengerConfig::default(),
        }
    }
}

/// Handler for the op-reth execution client.
pub struct OpRethHandler {
    pub container_id: String,
    pub container_name: String,

    /// The HTTP RPC URL for the L2 execution client.
    pub http_rpc_url: Url,

    /// The WebSocket RPC URL for the L2 execution client.
    pub ws_rpc_url: Url,

    /// The authenticated RPC URL for Engine API (used by kona-node).
    pub authrpc_url: Url,
}

/// Handler for the kona-node consensus client.
pub struct KonaNodeHandler {
    pub container_id: String,
    pub container_name: String,

    /// The RPC URL for the kona-node.
    pub rpc_url: Url,
}

/// Handler for the op-batcher.
pub struct OpBatcherHandler {
    pub container_id: String,
    pub container_name: String,

    /// The RPC URL for the op-batcher.
    pub rpc_url: Url,
}

/// Handler for the op-proposer.
pub struct OpProposerHandler {
    pub container_id: String,
    pub container_name: String,

    /// The RPC URL for the op-proposer.
    pub rpc_url: Url,
}

/// Handler for the op-challenger.
pub struct OpChallengerHandler {
    pub container_id: String,
    pub container_name: String,

    /// The RPC URL for the op-challenger.
    pub rpc_url: Url,
}

/// Handler for the complete L2 node setup.
pub struct L2NodesHandler {
    pub op_reth: OpRethHandler,
    pub kona_node: KonaNodeHandler,
    pub op_batcher: OpBatcherHandler,
    pub op_proposer: OpProposerHandler,
    pub op_challenger: OpChallengerHandler,
}

impl L2NodesConfig {
    /// Generate a JWT secret for authenticated communication between op-reth and kona-node.
    fn generate_jwt_secret() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let secret: [u8; 32] = rng.random();
        hex::encode(secret)
    }

    /// Write the JWT secret to a file.
    async fn write_jwt_secret(host_config_path: &PathBuf) -> Result<PathBuf, anyhow::Error> {
        let jwt_secret = Self::generate_jwt_secret();
        let jwt_path = host_config_path.join("jwt.hex");

        tokio::fs::write(&jwt_path, &jwt_secret)
            .await
            .context("Failed to write JWT secret file")?;

        tracing::debug!(path = ?jwt_path, "JWT secret written");
        Ok(jwt_path)
    }

    /// Start the op-reth execution client.
    async fn start_op_reth(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
    ) -> Result<OpRethHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // Build the op-reth command
        let mut cmd = vec![
            "node".to_string(),
            "--chain".to_string(),
            container_config_path
                .join("genesis.json")
                .display()
                .to_string(),
            "--datadir".to_string(),
            container_config_path
                .join("reth-data")
                .display()
                .to_string(),
            // HTTP RPC configuration
            "--http".to_string(),
            "--http.addr".to_string(),
            "0.0.0.0".to_string(),
            "--http.port".to_string(),
            self.op_reth.http_port.to_string(),
            "--http.api".to_string(),
            "eth,net,web3,debug,trace,txpool".to_string(),
            // WebSocket RPC configuration
            "--ws".to_string(),
            "--ws.addr".to_string(),
            "0.0.0.0".to_string(),
            "--ws.port".to_string(),
            self.op_reth.ws_port.to_string(),
            "--ws.api".to_string(),
            "eth,net,web3,debug,trace,txpool".to_string(),
            // Auth RPC for Engine API (kona-node communication)
            "--authrpc.addr".to_string(),
            "0.0.0.0".to_string(),
            "--authrpc.port".to_string(),
            self.op_reth.authrpc_port.to_string(),
            "--authrpc.jwtsecret".to_string(),
            container_config_path.join("jwt.hex").display().to_string(),
            // Metrics
            "--metrics".to_string(),
            format!("0.0.0.0:{}", self.op_reth.metrics_port),
            // Discovery (disabled for local devnet)
            "--disable-discovery".to_string(),
            // Rollup mode
            "--rollup.sequencer-http".to_string(),
            format!(
                "http://{}:{}",
                self.op_reth.container_name, self.op_reth.http_port
            ),
            // Logging
            "--log.stdout.format".to_string(),
            "terminal".to_string(),
        ];

        // Add extra arguments
        cmd.extend(self.op_reth.extra_args.clone());

        // Configure port bindings
        let port_bindings = HashMap::from([
            (
                format!("{}/tcp", self.op_reth.http_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_reth.http_port.to_string()),
                }]),
            ),
            (
                format!("{}/tcp", self.op_reth.ws_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_reth.ws_port.to_string()),
                }]),
            ),
            (
                format!("{}/tcp", self.op_reth.authrpc_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_reth.authrpc_port.to_string()),
                }]),
            ),
            (
                format!("{}/tcp", self.op_reth.metrics_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_reth.metrics_port.to_string()),
                }]),
            ),
            (
                format!("{}/udp", self.op_reth.discovery_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_reth.discovery_port.to_string()),
                }]),
            ),
            (
                format!("{}/tcp", self.op_reth.discovery_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_reth.discovery_port.to_string()),
                }]),
            ),
        ]);

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            network_mode: Some(docker.network_id.clone()),
            ..Default::default()
        };

        let config = Config {
            image: Some(format!(
                "{}:{}",
                docker.config.op_reth_docker_image, docker.config.op_reth_docker_tag
            )),
            cmd: Some(cmd),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container_id = docker
            .create_and_start_container(
                &self.op_reth.container_name,
                config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-reth container")?;

        tracing::info!(
            container_id = %container_id,
            container_name = %self.op_reth.container_name,
            "op-reth container started"
        );

        // Determine RPC URLs based on Docker network mode
        let (http_rpc_url, ws_rpc_url, authrpc_url) = (
            Url::parse(&format!(
                "http://{}:{}",
                self.op_reth.container_name, self.op_reth.http_port
            ))
            .context("Failed to parse op-reth HTTP RPC URL")?,
            Url::parse(&format!(
                "ws://{}:{}",
                self.op_reth.container_name, self.op_reth.ws_port
            ))
            .context("Failed to parse op-reth WebSocket RPC URL")?,
            Url::parse(&format!(
                "http://{}:{}",
                self.op_reth.container_name, self.op_reth.authrpc_port
            ))
            .context("Failed to parse op-reth Auth RPC URL")?,
        );

        Ok(OpRethHandler {
            container_id,
            container_name: self.op_reth.container_name.clone(),
            http_rpc_url,
            ws_rpc_url,
            authrpc_url,
        })
    }

    /// Start the kona-node consensus client.
    async fn start_kona_node(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        op_reth_handler: &OpRethHandler,
    ) -> Result<KonaNodeHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // Build the kona-node command
        let mut cmd = vec![
            // Metrics
            "--metrics.enabled".to_string(),
            "--metrics.port".to_string(),
            format!("{}", self.kona_node.metrics_port),
            "node".to_string(),
            "--mode".to_string(),
            "sequencer".to_string(),
            "--l1".to_string(),
            anvil_handler.l1_rpc_url.to_string(),
            "--l1-beacon".to_string(),
            anvil_handler.l1_rpc_url.to_string(),
            "--l1.slot-duration".to_string(),
            "12".to_string(),
            "--l2".to_string(),
            op_reth_handler.authrpc_url.to_string(),
            "--p2p.no-discovery".to_string(),
            "--rollup-cfg".to_string(),
            container_config_path
                .join("rollup.json")
                .display()
                .to_string(),
            "--l2.jwt-secret".to_string(),
            container_config_path.join("jwt.hex").display().to_string(),
            // RPC server configuration
            "--rpc.port".to_string(),
            format!("{}", self.kona_node.rpc_port),
        ];

        // Add extra arguments
        cmd.extend(self.kona_node.extra_args.clone());

        // Configure port bindings
        let port_bindings = HashMap::from([
            (
                format!("{}/tcp", self.kona_node.rpc_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.kona_node.rpc_port.to_string()),
                }]),
            ),
            (
                format!("{}/tcp", self.kona_node.metrics_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.kona_node.metrics_port.to_string()),
                }]),
            ),
        ]);

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            network_mode: Some(docker.network_id.clone()),
            ..Default::default()
        };

        let config = Config {
            image: Some(format!(
                "{}:{}",
                docker.config.kona_node_docker_image, docker.config.kona_node_docker_tag
            )),
            cmd: Some(cmd),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container_id = docker
            .create_and_start_container(
                &self.kona_node.container_name,
                config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start kona-node container")?;

        tracing::info!(
            container_id = %container_id,
            container_name = %self.kona_node.container_name,
            "kona-node container started"
        );

        // Determine RPC URL based on Docker network mode
        let rpc_url = Url::parse(&format!(
            "http://{}:{}",
            self.kona_node.container_name, self.kona_node.rpc_port
        ))
        .context("Failed to parse kona-node RPC URL")?;

        Ok(KonaNodeHandler {
            container_id,
            container_name: self.kona_node.container_name.clone(),
            rpc_url,
        })
    }

    /// Start the op-batcher.
    async fn start_op_batcher(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        op_reth_handler: &OpRethHandler,
        kona_node_handler: &KonaNodeHandler,
    ) -> Result<OpBatcherHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // The batcher account is at index 7 in the Anvil accounts
        let batcher_private_key = &anvil_handler.account_infos[7].private_key;

        // Build the op-batcher command
        let mut cmd = vec![
            "op-batcher".to_string(),
            "--l1-eth-rpc".to_string(),
            anvil_handler.l1_rpc_url.to_string(),
            "--l2-eth-rpc".to_string(),
            op_reth_handler.http_rpc_url.to_string(),
            "--rollup-rpc".to_string(),
            kona_node_handler.rpc_url.to_string(),
            "--private-key".to_string(),
            batcher_private_key.to_string(),
            // RPC configuration
            "--rpc.addr".to_string(),
            "0.0.0.0".to_string(),
            "--rpc.port".to_string(),
            self.op_batcher.rpc_port.to_string(),
            "--rpc.enable-admin".to_string(),
            // Metrics
            "--metrics.enabled".to_string(),
            "--metrics.addr".to_string(),
            "0.0.0.0".to_string(),
            "--metrics.port".to_string(),
            self.op_batcher.metrics_port.to_string(),
            // Batcher configuration
            "--data-availability-type".to_string(),
            "blobs".to_string(),
            "--throttle.unsafe-da-bytes-lower-threshold".to_string(),
            "0".to_string(),
        ];

        // Add extra arguments
        cmd.extend(self.op_batcher.extra_args.clone());

        // Configure port bindings
        let port_bindings = HashMap::from([
            (
                format!("{}/tcp", self.op_batcher.rpc_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_batcher.rpc_port.to_string()),
                }]),
            ),
            (
                format!("{}/tcp", self.op_batcher.metrics_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_batcher.metrics_port.to_string()),
                }]),
            ),
        ]);

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            network_mode: Some(docker.network_id.clone()),
            ..Default::default()
        };

        let config = Config {
            image: Some(format!(
                "{}:{}",
                docker.config.op_batcher_docker_image, docker.config.op_batcher_docker_tag
            )),
            cmd: Some(cmd),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container_id = docker
            .create_and_start_container(
                &self.op_batcher.container_name,
                config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-batcher container")?;

        tracing::info!(
            container_id = %container_id,
            container_name = %self.op_batcher.container_name,
            "op-batcher container started"
        );

        // Determine RPC URL based on Docker network mode
        let rpc_url = Url::parse(&format!(
            "http://{}:{}",
            self.op_batcher.container_name, self.op_batcher.rpc_port
        ))
        .context("Failed to parse op-batcher RPC URL")?;

        Ok(OpBatcherHandler {
            container_id,
            container_name: self.op_batcher.container_name.clone(),
            rpc_url,
        })
    }

    /// Start the op-proposer.
    async fn start_op_proposer(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        _op_reth_handler: &OpRethHandler,
        kona_node_handler: &KonaNodeHandler,
        _l2_chain_id: u64,
    ) -> Result<OpProposerHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // The proposer account is at index 8 in the Anvil accounts
        let proposer_private_key = &anvil_handler.account_infos[8].private_key;

        // Read the DisputeGameFactory address from state.json
        let state_file_path = host_config_path.join("state.json");
        let state_content = tokio::fs::read_to_string(&state_file_path)
            .await
            .context("Failed to read state.json for DisputeGameFactory address")?;

        let state: serde_json::Value =
            serde_json::from_str(&state_content).context("Failed to parse state.json")?;

        let dgf_address = state["opChainDeployments"][0]["DisputeGameFactoryProxy"]
            .as_str()
            .context("DisputeGameFactory address not found in state.json")?;

        // Build the op-proposer command
        let mut cmd = vec![
            "op-proposer".to_string(),
            "--l1-eth-rpc".to_string(),
            anvil_handler.l1_rpc_url.to_string(),
            "--rollup-rpc".to_string(),
            kona_node_handler.rpc_url.to_string(),
            "--private-key".to_string(),
            proposer_private_key.to_string(),
            "--game-factory-address".to_string(),
            dgf_address.to_string(),
            "--proposal-interval".to_string(),
            self.op_proposer.proposal_interval.clone(),
            "--game-type".to_string(),
            "254".to_string(), // Permissioned game type
            // RPC configuration
            "--rpc.addr".to_string(),
            "0.0.0.0".to_string(),
            "--rpc.port".to_string(),
            self.op_proposer.rpc_port.to_string(),
            // Metrics
            "--metrics.enabled".to_string(),
            "--metrics.addr".to_string(),
            "0.0.0.0".to_string(),
            "--metrics.port".to_string(),
            self.op_proposer.metrics_port.to_string(),
        ];

        // Add extra arguments
        cmd.extend(self.op_proposer.extra_args.clone());

        // Configure port bindings
        let port_bindings = HashMap::from([
            (
                format!("{}/tcp", self.op_proposer.rpc_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_proposer.rpc_port.to_string()),
                }]),
            ),
            (
                format!("{}/tcp", self.op_proposer.metrics_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_proposer.metrics_port.to_string()),
                }]),
            ),
        ]);

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            network_mode: Some(docker.network_id.clone()),
            ..Default::default()
        };

        let config = Config {
            image: Some(format!(
                "{}:{}",
                docker.config.op_proposer_docker_image, docker.config.op_proposer_docker_tag
            )),
            cmd: Some(cmd),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container_id = docker
            .create_and_start_container(
                &self.op_proposer.container_name,
                config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-proposer container")?;

        tracing::info!(
            container_id = %container_id,
            container_name = %self.op_proposer.container_name,
            "op-proposer container started"
        );

        // Determine RPC URL based on Docker network mode
        let rpc_url = Url::parse(&format!(
            "http://{}:{}",
            self.op_proposer.container_name, self.op_proposer.rpc_port
        ))
        .context("Failed to parse op-proposer RPC URL")?;

        Ok(OpProposerHandler {
            container_id,
            container_name: self.op_proposer.container_name.clone(),
            rpc_url,
        })
    }

    /// Start the op-challenger.
    async fn start_op_challenger(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        kona_node_handler: &KonaNodeHandler,
        _l2_chain_id: u64,
    ) -> Result<OpChallengerHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // The challenger account is at index 9 in the Anvil accounts
        let challenger_private_key = &anvil_handler.account_infos[9].private_key;

        // Read the DisputeGameFactory address from state.json
        let state_file_path = host_config_path.join("state.json");
        let state_content = tokio::fs::read_to_string(&state_file_path)
            .await
            .context("Failed to read state.json for DisputeGameFactory address")?;

        let state: serde_json::Value =
            serde_json::from_str(&state_content).context("Failed to parse state.json")?;

        let dgf_address = state["opChainDeployments"][0]["DisputeGameFactoryProxy"]
            .as_str()
            .context("DisputeGameFactory address not found in state.json")?;

        // Build the op-challenger command
        let mut cmd = vec![
            "op-challenger".to_string(),
            "--l1-eth-rpc".to_string(),
            anvil_handler.l1_rpc_url.to_string(),
            "--rollup-rpc".to_string(),
            kona_node_handler.rpc_url.to_string(),
            "--l2-eth-rpc".to_string(),
            format!(
                "http://{}:{}",
                self.op_reth.container_name, self.op_reth.http_port
            ),
            "--private-key".to_string(),
            challenger_private_key.to_string(),
            "--game-factory-address".to_string(),
            dgf_address.to_string(),
            "--trace-type".to_string(),
            "permissioned".to_string(),
            "--game-allowlist".to_string(),
            "254".to_string(), // Permissioned game type
            // RPC configuration
            "--rpc.addr".to_string(),
            "0.0.0.0".to_string(),
            "--rpc.port".to_string(),
            self.op_challenger.rpc_port.to_string(),
            // Metrics
            "--metrics.enabled".to_string(),
            "--metrics.addr".to_string(),
            "0.0.0.0".to_string(),
            "--metrics.port".to_string(),
            self.op_challenger.metrics_port.to_string(),
        ];

        // Add extra arguments
        cmd.extend(self.op_challenger.extra_args.clone());

        // Configure port bindings
        let port_bindings = HashMap::from([
            (
                format!("{}/tcp", self.op_challenger.rpc_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_challenger.rpc_port.to_string()),
                }]),
            ),
            (
                format!("{}/tcp", self.op_challenger.metrics_port),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(self.op_challenger.metrics_port.to_string()),
                }]),
            ),
        ]);

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            network_mode: Some(docker.network_id.clone()),
            ..Default::default()
        };

        let config = Config {
            image: Some(format!(
                "{}:{}",
                docker.config.op_challenger_docker_image, docker.config.op_challenger_docker_tag
            )),
            cmd: Some(cmd),
            host_config: Some(host_config),
            ..Default::default()
        };

        let container_id = docker
            .create_and_start_container(
                &self.op_challenger.container_name,
                config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-challenger container")?;

        tracing::info!(
            container_id = %container_id,
            container_name = %self.op_challenger.container_name,
            "op-challenger container started"
        );

        // Determine RPC URL based on Docker network mode
        let rpc_url = Url::parse(&format!(
            "http://{}:{}",
            self.op_challenger.container_name, self.op_challenger.rpc_port
        ))
        .context("Failed to parse op-challenger RPC URL")?;

        Ok(OpChallengerHandler {
            container_id,
            container_name: self.op_challenger.container_name.clone(),
            rpc_url,
        })
    }

    /// Start all L2 node components.
    ///
    /// This starts op-reth first (execution client), then kona-node (consensus client),
    /// followed by op-batcher (batch submitter), op-proposer, and op-challenger.
    /// The components communicate via the Engine API using JWT authentication.
    pub async fn start(
        self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
        l2_chain_id: u64,
    ) -> Result<L2NodesHandler, anyhow::Error> {
        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Generate JWT secret for Engine API authentication
        Self::write_jwt_secret(&host_config_path).await?;

        tracing::info!("Starting op-reth execution client...");

        // Start op-reth first
        let op_reth_handler = self.start_op_reth(docker, &host_config_path).await?;

        // Give op-reth a moment to initialize before starting kona-node
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        tracing::info!("Starting kona-node consensus client...");

        // Start kona-node
        let kona_node_handler = self
            .start_kona_node(docker, &host_config_path, anvil_handler, &op_reth_handler)
            .await?;

        // Give kona-node a moment to initialize before starting op-batcher
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        tracing::info!("Starting op-batcher...");

        // Start op-batcher
        let op_batcher_handler = self
            .start_op_batcher(
                docker,
                &host_config_path,
                anvil_handler,
                &op_reth_handler,
                &kona_node_handler,
            )
            .await?;

        tracing::info!("Starting op-proposer...");

        // Start op-proposer
        let op_proposer_handler = self
            .start_op_proposer(
                docker,
                &host_config_path,
                anvil_handler,
                &op_reth_handler,
                &kona_node_handler,
                l2_chain_id,
            )
            .await?;

        tracing::info!("Starting op-challenger...");

        // Start op-challenger
        let op_challenger_handler = self
            .start_op_challenger(
                docker,
                &host_config_path,
                anvil_handler,
                &kona_node_handler,
                l2_chain_id,
            )
            .await?;

        tracing::info!(
            l2_http_rpc = %op_reth_handler.http_rpc_url,
            l2_ws_rpc = %op_reth_handler.ws_rpc_url,
            kona_node_rpc = %kona_node_handler.rpc_url,
            op_batcher_rpc = %op_batcher_handler.rpc_url,
            op_proposer_rpc = %op_proposer_handler.rpc_url,
            op_challenger_rpc = %op_challenger_handler.rpc_url,
            "L2 nodes started successfully"
        );

        Ok(L2NodesHandler {
            op_reth: op_reth_handler,
            kona_node: kona_node_handler,
            op_batcher: op_batcher_handler,
            op_proposer: op_proposer_handler,
            op_challenger: op_challenger_handler,
        })
    }
}
