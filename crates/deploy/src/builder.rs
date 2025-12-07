//! Builder module for creating a [`Deployer`] configuration.
//!
//! This module provides the [`DeployerBuilder`] struct which simplifies the creation
//! of a [`Deployer`] by handling network name generation, output directory creation,
//! and genesis timestamp fetching from L1 RPC.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rand::Rng;
use serde::Deserialize;

use crate::{
    ANVIL_DEFAULT_IMAGE, ANVIL_DEFAULT_TAG, AnvilConfig, Deployer, DockerImage,
    GRAFANA_DEFAULT_IMAGE, GRAFANA_DEFAULT_TAG, GrafanaConfig, KONA_NODE_DEFAULT_IMAGE,
    KONA_NODE_DEFAULT_TAG, KonaNodeBuilder, KupDockerConfig, L2StackBuilder, MonitoringConfig,
    OP_BATCHER_DEFAULT_IMAGE, OP_BATCHER_DEFAULT_TAG, OP_CHALLENGER_DEFAULT_IMAGE,
    OP_CHALLENGER_DEFAULT_TAG, OP_DEPLOYER_DEFAULT_IMAGE, OP_DEPLOYER_DEFAULT_TAG,
    OP_PROPOSER_DEFAULT_IMAGE, OP_PROPOSER_DEFAULT_TAG, OP_RETH_DEFAULT_IMAGE, OP_RETH_DEFAULT_TAG,
    OpBatcherBuilder, OpChallengerBuilder, OpDeployerConfig, OpProposerBuilder, OpRethBuilder,
    PROMETHEUS_DEFAULT_IMAGE, PROMETHEUS_DEFAULT_TAG, PrometheusConfig,
};

/// Block header information from an RPC response.
#[derive(Debug, Deserialize)]
struct BlockInfo {
    #[serde(deserialize_with = "deserialize_u64_from_hex")]
    number: u64,
    #[serde(deserialize_with = "deserialize_u64_from_hex")]
    timestamp: u64,
}

/// JSON-RPC response wrapper.
#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: T,
}

/// Deserialize a u64 from a hex string (with 0x prefix).
fn deserialize_u64_from_hex<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(serde::de::Error::custom)
}

/// Fetches the latest block from an Ethereum RPC endpoint.
async fn fetch_latest_block(rpc_url: &str) -> Result<BlockInfo> {
    let client = reqwest::Client::new();
    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": ["latest", false],
            "id": 1
        }))
        .send()
        .await
        .context("Failed to send RPC request")?;

    let json: JsonRpcResponse<BlockInfo> = response
        .json()
        .await
        .context("Failed to parse RPC response")?;

    Ok(json.result)
}

/// Specifies how the output data directory should be created.
#[derive(Debug, Clone)]
pub enum OutDataPath {
    /// Use a temporary directory that will be cleaned up.
    TempDir,
    /// Use a specific path.
    Path(PathBuf),
}

/// Builder for creating a [`Deployer`] configuration.
///
/// This builder handles:
/// - Network name generation (if not provided)
/// - L2 chain ID generation (random if not provided)
/// - Output data directory creation
/// - Genesis timestamp fetching from L1 RPC
///
/// # Example
///
/// ```no_run
/// use kupcake_deploy::DeployerBuilder;
///
/// # async fn example() -> anyhow::Result<()> {
/// let deployer = DeployerBuilder::new(11155111) // Sepolia chain ID
///     .network_name("my-network")
///     .l2_chain_id(12345)
///     .l1_rpc_url("https://ethereum-sepolia-rpc.publicnode.com")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DeployerBuilder {
    /// The L1 chain ID (required).
    l1_chain_id: u64,
    /// The L2 chain ID (optional, random if not provided).
    l2_chain_id: Option<u64>,
    /// The network name (optional, generated if not provided).
    network_name: Option<String>,
    /// The output data path specification.
    outdata: Option<OutDataPath>,
    /// The L1 RPC URL for forking (optional).
    l1_rpc_url: Option<String>,
    /// Whether to skip cleanup of docker containers.
    no_cleanup: bool,
    /// Path to custom dashboards directory.
    dashboards_path: Option<PathBuf>,
    /// Whether monitoring is enabled.
    monitoring_enabled: bool,
    /// Block time in seconds for both L1 (Anvil) and L2 derivation.
    block_time: u64,

    // Docker images
    anvil_docker: DockerImage,
    op_reth_docker: DockerImage,
    kona_node_docker: DockerImage,
    op_batcher_docker: DockerImage,
    op_proposer_docker: DockerImage,
    op_challenger_docker: DockerImage,
    op_deployer_docker: DockerImage,
    prometheus_docker: DockerImage,
    grafana_docker: DockerImage,
}

impl DeployerBuilder {
    /// Create a new [`DeployerBuilder`] with the required L1 chain ID.
    pub fn new(l1_chain_id: u64) -> Self {
        Self {
            l1_chain_id,
            l2_chain_id: None,
            network_name: None,
            outdata: None,
            l1_rpc_url: None,
            no_cleanup: false,
            dashboards_path: None,
            monitoring_enabled: true,
            block_time: 12,
            anvil_docker: DockerImage::new(ANVIL_DEFAULT_IMAGE, ANVIL_DEFAULT_TAG),
            op_reth_docker: DockerImage::new(OP_RETH_DEFAULT_IMAGE, OP_RETH_DEFAULT_TAG),
            kona_node_docker: DockerImage::new(KONA_NODE_DEFAULT_IMAGE, KONA_NODE_DEFAULT_TAG),
            op_batcher_docker: DockerImage::new(OP_BATCHER_DEFAULT_IMAGE, OP_BATCHER_DEFAULT_TAG),
            op_proposer_docker: DockerImage::new(
                OP_PROPOSER_DEFAULT_IMAGE,
                OP_PROPOSER_DEFAULT_TAG,
            ),
            op_challenger_docker: DockerImage::new(
                OP_CHALLENGER_DEFAULT_IMAGE,
                OP_CHALLENGER_DEFAULT_TAG,
            ),
            op_deployer_docker: DockerImage::new(
                OP_DEPLOYER_DEFAULT_IMAGE,
                OP_DEPLOYER_DEFAULT_TAG,
            ),
            prometheus_docker: DockerImage::new(PROMETHEUS_DEFAULT_IMAGE, PROMETHEUS_DEFAULT_TAG),
            grafana_docker: DockerImage::new(GRAFANA_DEFAULT_IMAGE, GRAFANA_DEFAULT_TAG),
        }
    }

    /// Set the block time in seconds.
    ///
    /// This affects both the Anvil L1 chain and the kona-node L1 slot duration.
    /// Defaults to 12 seconds (Ethereum mainnet block time).
    pub fn block_time(mut self, block_time: u64) -> Self {
        self.block_time = block_time;
        self
    }

    // ==================== Docker Image Setters ====================

    /// Set Docker image for Anvil.
    pub fn anvil_image(mut self, image: impl Into<String>) -> Self {
        self.anvil_docker.image = image.into();
        self
    }

    /// Set Docker tag for Anvil.
    pub fn anvil_tag(mut self, tag: impl Into<String>) -> Self {
        self.anvil_docker.tag = tag.into();
        self
    }

    /// Set Docker image for op-reth.
    pub fn op_reth_image(mut self, image: impl Into<String>) -> Self {
        self.op_reth_docker.image = image.into();
        self
    }

    /// Set Docker tag for op-reth.
    pub fn op_reth_tag(mut self, tag: impl Into<String>) -> Self {
        self.op_reth_docker.tag = tag.into();
        self
    }

    /// Set Docker image for kona-node.
    pub fn kona_node_image(mut self, image: impl Into<String>) -> Self {
        self.kona_node_docker.image = image.into();
        self
    }

    /// Set Docker tag for kona-node.
    pub fn kona_node_tag(mut self, tag: impl Into<String>) -> Self {
        self.kona_node_docker.tag = tag.into();
        self
    }

    /// Set Docker image for op-batcher.
    pub fn op_batcher_image(mut self, image: impl Into<String>) -> Self {
        self.op_batcher_docker.image = image.into();
        self
    }

    /// Set Docker tag for op-batcher.
    pub fn op_batcher_tag(mut self, tag: impl Into<String>) -> Self {
        self.op_batcher_docker.tag = tag.into();
        self
    }

    /// Set Docker image for op-proposer.
    pub fn op_proposer_image(mut self, image: impl Into<String>) -> Self {
        self.op_proposer_docker.image = image.into();
        self
    }

    /// Set Docker tag for op-proposer.
    pub fn op_proposer_tag(mut self, tag: impl Into<String>) -> Self {
        self.op_proposer_docker.tag = tag.into();
        self
    }

    /// Set Docker image for op-challenger.
    pub fn op_challenger_image(mut self, image: impl Into<String>) -> Self {
        self.op_challenger_docker.image = image.into();
        self
    }

    /// Set Docker tag for op-challenger.
    pub fn op_challenger_tag(mut self, tag: impl Into<String>) -> Self {
        self.op_challenger_docker.tag = tag.into();
        self
    }

    /// Set Docker image for op-deployer.
    pub fn op_deployer_image(mut self, image: impl Into<String>) -> Self {
        self.op_deployer_docker.image = image.into();
        self
    }

    /// Set Docker tag for op-deployer.
    pub fn op_deployer_tag(mut self, tag: impl Into<String>) -> Self {
        self.op_deployer_docker.tag = tag.into();
        self
    }

    /// Set Docker image for Prometheus.
    pub fn prometheus_image(mut self, image: impl Into<String>) -> Self {
        self.prometheus_docker.image = image.into();
        self
    }

    /// Set Docker tag for Prometheus.
    pub fn prometheus_tag(mut self, tag: impl Into<String>) -> Self {
        self.prometheus_docker.tag = tag.into();
        self
    }

    /// Set Docker image for Grafana.
    pub fn grafana_image(mut self, image: impl Into<String>) -> Self {
        self.grafana_docker.image = image.into();
        self
    }

    /// Set Docker tag for Grafana.
    pub fn grafana_tag(mut self, tag: impl Into<String>) -> Self {
        self.grafana_docker.tag = tag.into();
        self
    }

    /// Set the L2 chain ID.
    ///
    /// If not set, a random chain ID between 10000 and 99999 will be generated.
    pub fn l2_chain_id(mut self, l2_chain_id: u64) -> Self {
        self.l2_chain_id = Some(l2_chain_id);
        self
    }

    /// Set the L2 chain ID if `Some`, otherwise do nothing.
    ///
    /// If not set, a random chain ID between 10000 and 99999 will be generated.
    pub fn maybe_l2_chain_id(mut self, l2_chain_id: Option<u64>) -> Self {
        if let Some(id) = l2_chain_id {
            self.l2_chain_id = Some(id);
        }
        self
    }

    /// Set the network name.
    ///
    /// If not set, a memorable two-word name will be generated (e.g., "kup-happy-turtle").
    pub fn network_name(mut self, name: impl Into<String>) -> Self {
        self.network_name = Some(name.into());
        self
    }

    /// Set the network name if `Some`, otherwise do nothing.
    ///
    /// If not set, a memorable two-word name will be generated (e.g., "kup-happy-turtle").
    pub fn maybe_network_name(mut self, name: Option<String>) -> Self {
        if let Some(n) = name {
            self.network_name = Some(n);
        }
        self
    }

    /// Set the output data directory path.
    ///
    /// If not set, defaults to `./data-<network-name>`.
    pub fn outdata(mut self, outdata: OutDataPath) -> Self {
        self.outdata = Some(outdata);
        self
    }

    /// Set the output data directory path if `Some`, otherwise do nothing.
    ///
    /// If not set, defaults to `./data-<network-name>`.
    pub fn maybe_outdata(mut self, outdata: Option<OutDataPath>) -> Self {
        if let Some(o) = outdata {
            self.outdata = Some(o);
        }
        self
    }

    /// Set the output data directory to a specific path.
    pub fn outdata_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.outdata = Some(OutDataPath::Path(path.into()));
        self
    }

    /// Set the L1 RPC URL for forking.
    ///
    /// When provided, Anvil will fork from this RPC endpoint and the genesis
    /// timestamp will be set to match the latest block.
    pub fn l1_rpc_url(mut self, url: impl Into<String>) -> Self {
        self.l1_rpc_url = Some(url.into());
        self
    }

    /// Set the L1 RPC URL for forking if `Some`, otherwise do nothing.
    ///
    /// When provided, Anvil will fork from this RPC endpoint and the genesis
    /// timestamp will be set to match the latest block.
    pub fn maybe_l1_rpc_url(mut self, url: Option<String>) -> Self {
        if let Some(u) = url {
            self.l1_rpc_url = Some(u);
        }
        self
    }

    /// Set whether to skip cleanup of docker containers on exit.
    pub fn no_cleanup(mut self, no_cleanup: bool) -> Self {
        self.no_cleanup = no_cleanup;
        self
    }

    /// Set the path to custom Grafana dashboards.
    pub fn dashboards_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.dashboards_path = Some(path.into());
        self
    }

    /// Enable or disable monitoring (Prometheus + Grafana).
    pub fn monitoring_enabled(mut self, enabled: bool) -> Self {
        self.monitoring_enabled = enabled;
        self
    }

    /// Build the [`Deployer`] configuration.
    ///
    /// This method:
    /// 1. Generates a network name if not provided
    /// 2. Generates a random L2 chain ID if not provided
    /// 3. Creates the output data directory if it doesn't exist
    /// 4. Fetches genesis timestamp from L1 RPC if an RPC URL is provided
    pub async fn build(self) -> Result<Deployer> {
        // Generate L2 chain ID if not provided
        let l2_chain_id = self
            .l2_chain_id
            .unwrap_or_else(|| rand::rng().random_range(10000..=99999));

        // Generate network name if not provided
        let network_name = self.network_name.unwrap_or_else(|| {
            let name = names::Generator::default()
                .next()
                .unwrap_or_else(|| "unknown-network".to_string());
            format!("kup-{}", name)
        });

        // Determine output data path
        let outdata_path = match self.outdata {
            None => PathBuf::from(format!("data-{}", network_name)),
            Some(OutDataPath::TempDir) => {
                let temp_dir = tempdir::TempDir::new("data-kup-")
                    .context("Failed to create temporary directory")?;
                PathBuf::from(temp_dir.path().to_string_lossy().to_string())
            }
            Some(OutDataPath::Path(path)) => path,
        };

        // Create the output data directory if it doesn't exist
        if !outdata_path.try_exists().context(format!(
            "Failed to check if output data directory exists at path {}. Ensure you provided valid permissions to the directory.",
            outdata_path.display()
        ))? {
            std::fs::create_dir_all(&outdata_path)
                .context("Failed to create output data directory")?;
        }

        let outdata_path = outdata_path
            .canonicalize()
            .context("Failed to canonicalize output data directory path")?;

        // Fetch genesis timestamp and fork block number if L1 RPC URL is provided
        let (genesis_timestamp, fork_block_number) = if let Some(ref rpc_url) = self.l1_rpc_url {
            let block = fetch_latest_block(rpc_url)
                .await
                .context("Failed to fetch latest block from L1 RPC")?;
            (
                Some(
                    block
                        .timestamp
                        .saturating_sub(self.block_time * block.number),
                ),
                Some(block.number),
            )
        } else {
            (None, None)
        };

        tracing::info!(
            network_name,
            l1_chain_id = self.l1_chain_id,
            l2_chain_id,
            outdata_path = %outdata_path.display(),
            "Building OP Stack deployer configuration..."
        );

        // Build the Deployer
        let deployer = Deployer {
            l1_chain_id: self.l1_chain_id,
            l2_chain_id,
            outdata: outdata_path,

            anvil: AnvilConfig {
                docker_image: self.anvil_docker,
                container_name: format!("{}-anvil", network_name),
                fork_url: self.l1_rpc_url,
                timestamp: genesis_timestamp,
                fork_block_number,
                block_time: self.block_time,
                ..Default::default()
            },

            docker: KupDockerConfig {
                net_name: format!("{}-network", network_name),
                no_cleanup: self.no_cleanup,
            },

            op_deployer: OpDeployerConfig {
                docker_image: self.op_deployer_docker,
                container_name: format!("{}-op-deployer", network_name),
            },

            l2_stack: L2StackBuilder {
                op_reth: OpRethBuilder {
                    docker_image: self.op_reth_docker,
                    container_name: format!("{}-op-reth", network_name),
                    ..Default::default()
                },
                kona_node: KonaNodeBuilder {
                    docker_image: self.kona_node_docker,
                    container_name: format!("{}-kona-node", network_name),
                    l1_slot_duration: self.block_time,
                    ..Default::default()
                },
                op_batcher: OpBatcherBuilder {
                    docker_image: self.op_batcher_docker,
                    container_name: format!("{}-op-batcher", network_name),
                    ..Default::default()
                },
                op_proposer: OpProposerBuilder {
                    docker_image: self.op_proposer_docker,
                    container_name: format!("{}-op-proposer", network_name),
                    ..Default::default()
                },
                op_challenger: OpChallengerBuilder {
                    docker_image: self.op_challenger_docker,
                    container_name: format!("{}-op-challenger", network_name),
                    ..Default::default()
                },
            },

            monitoring: MonitoringConfig {
                prometheus: PrometheusConfig {
                    docker_image: self.prometheus_docker,
                    container_name: format!("{}-prometheus", network_name),
                    ..Default::default()
                },
                grafana: GrafanaConfig {
                    docker_image: self.grafana_docker,
                    container_name: format!("{}-grafana", network_name),
                    ..Default::default()
                },
                enabled: self.monitoring_enabled,
            },

            dashboards_path: self.dashboards_path,
        };

        Ok(deployer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = DeployerBuilder::new(11155111);
        assert_eq!(builder.l1_chain_id, 11155111);
        assert!(builder.l2_chain_id.is_none());
        assert!(builder.network_name.is_none());
        assert!(builder.outdata.is_none());
        assert!(builder.l1_rpc_url.is_none());
        assert!(!builder.no_cleanup);
        assert!(builder.monitoring_enabled);
    }

    #[test]
    fn test_builder_with_options() {
        let builder = DeployerBuilder::new(11155111)
            .l2_chain_id(12345)
            .network_name("test-network")
            .no_cleanup(true)
            .monitoring_enabled(false);

        assert_eq!(builder.l2_chain_id, Some(12345));
        assert_eq!(builder.network_name, Some("test-network".to_string()));
        assert!(builder.no_cleanup);
        assert!(!builder.monitoring_enabled);
    }
}
