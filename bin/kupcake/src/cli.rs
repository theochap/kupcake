use clap::Parser;
use kupcake_deploy::{
    ANVIL_DEFAULT_IMAGE, ANVIL_DEFAULT_TAG, GRAFANA_DEFAULT_IMAGE, GRAFANA_DEFAULT_TAG,
    KONA_NODE_DEFAULT_IMAGE, KONA_NODE_DEFAULT_TAG, OP_BATCHER_DEFAULT_IMAGE,
    OP_BATCHER_DEFAULT_TAG, OP_CHALLENGER_DEFAULT_IMAGE, OP_CHALLENGER_DEFAULT_TAG,
    OP_DEPLOYER_DEFAULT_IMAGE, OP_DEPLOYER_DEFAULT_TAG, OP_PROPOSER_DEFAULT_IMAGE,
    OP_PROPOSER_DEFAULT_TAG, OP_RETH_DEFAULT_IMAGE, OP_RETH_DEFAULT_TAG, PROMETHEUS_DEFAULT_IMAGE,
    PROMETHEUS_DEFAULT_TAG,
};
use tracing::level_filters::LevelFilter;

/// The default L1 chain ID (Sepolia).
const DEFAULT_L1_CHAIN_INFO: L1Chain = L1Chain::Sepolia;
/// The default L1 RPC URL (Sepolia public node).

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum::Display, strum::EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum L1Provider {
    PublicNode,
    #[strum(default)]
    Custom(String),
}

impl L1Provider {
    pub fn to_rpc_url(&self, chain: L1Chain) -> anyhow::Result<String> {
        match self {
            L1Provider::PublicNode if chain == L1Chain::Sepolia => {
                Ok("https://ethereum-sepolia-rpc.publicnode.com".to_string())
            }
            L1Provider::PublicNode if chain == L1Chain::Mainnet => {
                Ok("https://ethereum-mainnet-rpc.publicnode.com".to_string())
            }
            L1Provider::PublicNode => {
                anyhow::bail!("Public node is not supported for custom chains");
            }
            L1Provider::Custom(url) => Ok(url.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum L1Chain {
    Sepolia,
    Mainnet,
}

impl L1Chain {
    pub fn to_chain_id(&self) -> u64 {
        match self {
            L1Chain::Sepolia => 11155111,
            L1Chain::Mainnet => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum L2Chain {
    OpSepolia,
    OpMainnet,
    BaseSepolia,
    BaseMainnet,
    #[strum(serialize = "{0}")]
    Custom(u64),
}

impl L2Chain {
    pub fn to_chain_id(&self) -> u64 {
        match self {
            L2Chain::OpSepolia => 11155420,
            L2Chain::OpMainnet => 10,
            L2Chain::BaseSepolia => 84532,
            L2Chain::BaseMainnet => 8453,
            L2Chain::Custom(id) => *id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, strum::EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum OutData {
    TempDir,
    #[strum(serialize = "{0}")]
    Path(String),
}

#[derive(Parser)]
#[command(name = "kup")]
#[command(
    author,
    version,
    about = "Bootstrap a rust-based op-stack chain in a few clicks"
)]
pub struct Cli {
    /// The verbosity level.
    #[arg(short, long, env = "KUP_VERBOSITY", default_value_t = LevelFilter::INFO)]
    pub verbosity: LevelFilter,

    /// A custom name for the network. If not provided, the network will be named:
    /// kup-<l1-chain-name>-<l2-chain-name>.
    #[arg(short, long, visible_alias = "name", env = "KUP_NETWORK_NAME")]
    pub network: Option<String>,

    /// The URL of an L1 RPC endpoint.
    ///
    /// If not provided, the L1 chain will be started with a public node endpoint.
    ///
    /// If public node is selected, anvil will be started in fork-mode using a node from `<https://publicnode.com/>`
    #[arg(long, alias = "l1-rpc", env = "KUP_L1_RPC_URL", default_value_t = L1Provider::PublicNode)]
    pub l1_rpc_provider: L1Provider,

    /// The L1 chain info (chain ID or name).
    #[arg(long, alias = "l1", env = "KUP_L1_CHAIN", default_value_t = DEFAULT_L1_CHAIN_INFO)]
    pub l1_chain: L1Chain,

    /// The L2 chain info (chain ID or name).
    /// If not provided, the L2 chain id will be generated randomly.
    #[arg(long, alias = "l2", env = "KUP_L2_CHAIN")]
    pub l2_chain: Option<L2Chain>,

    /// Redeploy all contracts.
    /// If not provided and the deployer data directory exists, the contracts will not be redeployed.
    #[arg(long, env = "KUP_REDEPLOY", default_value_t = false)]
    pub redeploy: bool,

    /// The path to the output data directory.
    ///
    /// If not provided, the data will be stored at: ./data_<network-name>
    #[arg(long, alias = "outdata", env = "KUP_OUTDATA")]
    pub outdata: Option<OutData>,

    /// Skips the cleanup of docker containers when the program exits.
    #[arg(long, env = "KUP_NO_CLEANUP")]
    pub no_cleanup: bool,

    /// The block time in seconds for the L1 chain (Anvil) and L2 derivation.
    ///
    /// Defaults to 12 seconds (Ethereum mainnet block time).
    #[arg(long, env = "KUP_BLOCK_TIME", default_value_t = 12)]
    pub block_time: u64,

    /// The number of L2 nodes to deploy.
    ///
    /// The first node is always the sequencer, additional nodes are validators.
    /// Defaults to 3 (sequencer + 2 validators).
    #[arg(long, alias = "nodes", env = "KUP_L2_NODES", default_value_t = 3)]
    pub l2_nodes: usize,

    /// Path to an existing kupconf.toml configuration file to load.
    ///
    /// When provided, the deployment will use the configuration from this file
    /// instead of generating a new one from CLI arguments.
    #[arg(long, alias = "conf", env = "KUP_CONFIG")]
    pub config: Option<String>,

    /// Docker image overrides for all services.
    #[clap(flatten)]
    pub docker_images: DockerImageOverrides,
}

/// Docker image overrides for all services.
#[derive(Debug, Clone, Parser)]
pub struct DockerImageOverrides {
    /// Docker image for Anvil (L1 chain).
    #[arg(long, env = "KUP_ANVIL_IMAGE", default_value = ANVIL_DEFAULT_IMAGE)]
    pub anvil_image: String,

    /// Docker tag for Anvil.
    #[arg(long, env = "KUP_ANVIL_TAG", default_value = ANVIL_DEFAULT_TAG)]
    pub anvil_tag: String,

    /// Docker image for op-reth (L2 execution client).
    #[arg(long, env = "KUP_OP_RETH_IMAGE", default_value = OP_RETH_DEFAULT_IMAGE)]
    pub op_reth_image: String,

    /// Docker tag for op-reth.
    #[arg(long, env = "KUP_OP_RETH_TAG", default_value = OP_RETH_DEFAULT_TAG)]
    pub op_reth_tag: String,

    /// Docker image for kona-node (L2 consensus client).
    #[arg(long, env = "KUP_KONA_NODE_IMAGE", default_value = KONA_NODE_DEFAULT_IMAGE)]
    pub kona_node_image: String,

    /// Docker tag for kona-node.
    #[arg(long, env = "KUP_KONA_NODE_TAG", default_value = KONA_NODE_DEFAULT_TAG)]
    pub kona_node_tag: String,

    /// Docker image for op-batcher.
    #[arg(long, env = "KUP_OP_BATCHER_IMAGE", default_value = OP_BATCHER_DEFAULT_IMAGE)]
    pub op_batcher_image: String,

    /// Docker tag for op-batcher.
    #[arg(long, env = "KUP_OP_BATCHER_TAG", default_value = OP_BATCHER_DEFAULT_TAG)]
    pub op_batcher_tag: String,

    /// Docker image for op-proposer.
    #[arg(long, env = "KUP_OP_PROPOSER_IMAGE", default_value = OP_PROPOSER_DEFAULT_IMAGE)]
    pub op_proposer_image: String,

    /// Docker tag for op-proposer.
    #[arg(long, env = "KUP_OP_PROPOSER_TAG", default_value = OP_PROPOSER_DEFAULT_TAG)]
    pub op_proposer_tag: String,

    /// Docker image for op-challenger.
    #[arg(long, env = "KUP_OP_CHALLENGER_IMAGE", default_value = OP_CHALLENGER_DEFAULT_IMAGE)]
    pub op_challenger_image: String,

    /// Docker tag for op-challenger.
    #[arg(long, env = "KUP_OP_CHALLENGER_TAG", default_value = OP_CHALLENGER_DEFAULT_TAG)]
    pub op_challenger_tag: String,

    /// Docker image for op-deployer.
    #[arg(long, env = "KUP_OP_DEPLOYER_IMAGE", default_value = OP_DEPLOYER_DEFAULT_IMAGE)]
    pub op_deployer_image: String,

    /// Docker tag for op-deployer.
    #[arg(long, env = "KUP_OP_DEPLOYER_TAG", default_value = OP_DEPLOYER_DEFAULT_TAG)]
    pub op_deployer_tag: String,

    /// Docker image for Prometheus.
    #[arg(long, env = "KUP_PROMETHEUS_IMAGE", default_value = PROMETHEUS_DEFAULT_IMAGE)]
    pub prometheus_image: String,

    /// Docker tag for Prometheus.
    #[arg(long, env = "KUP_PROMETHEUS_TAG", default_value = PROMETHEUS_DEFAULT_TAG)]
    pub prometheus_tag: String,

    /// Docker image for Grafana.
    #[arg(long, env = "KUP_GRAFANA_IMAGE", default_value = GRAFANA_DEFAULT_IMAGE)]
    pub grafana_image: String,

    /// Docker tag for Grafana.
    #[arg(long, env = "KUP_GRAFANA_TAG", default_value = GRAFANA_DEFAULT_TAG)]
    pub grafana_tag: String,
}
