use clap::{Parser, Subcommand};
use kupcake_deploy::{
    ANVIL_DEFAULT_IMAGE, ANVIL_DEFAULT_TAG, GRAFANA_DEFAULT_IMAGE, GRAFANA_DEFAULT_TAG,
    KONA_NODE_DEFAULT_IMAGE, KONA_NODE_DEFAULT_TAG, OP_BATCHER_DEFAULT_IMAGE,
    OP_BATCHER_DEFAULT_TAG, OP_CHALLENGER_DEFAULT_IMAGE, OP_CHALLENGER_DEFAULT_TAG,
    OP_CONDUCTOR_DEFAULT_IMAGE, OP_CONDUCTOR_DEFAULT_TAG, OP_DEPLOYER_DEFAULT_IMAGE,
    OP_DEPLOYER_DEFAULT_TAG, OP_PROPOSER_DEFAULT_IMAGE, OP_PROPOSER_DEFAULT_TAG,
    OP_RETH_DEFAULT_IMAGE, OP_RETH_DEFAULT_TAG, PROMETHEUS_DEFAULT_IMAGE, PROMETHEUS_DEFAULT_TAG,
};
use tracing::level_filters::LevelFilter;

/// L1 source configuration - can be a known chain name or a custom RPC URL.
///
/// When a chain name is provided (e.g., "sepolia", "mainnet"), the known chain ID
/// and a public RPC endpoint are used. When a custom RPC URL is provided,
/// the chain ID is detected via the `eth_chainId` RPC method.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum L1Source {
    /// Known chain with predefined chain ID and public RPC URL
    Sepolia,
    Mainnet,
    /// Custom RPC URL - chain ID will be detected via eth_chainId
    Custom(String),
}

impl std::str::FromStr for L1Source {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sepolia" => Ok(L1Source::Sepolia),
            "mainnet" => Ok(L1Source::Mainnet),
            // Anything else is treated as a custom RPC URL
            _ => Ok(L1Source::Custom(s.to_string())),
        }
    }
}

impl L1Source {
    /// Returns the RPC URL for this L1 source.
    pub fn rpc_url(&self) -> String {
        match self {
            L1Source::Sepolia => "https://ethereum-sepolia-rpc.publicnode.com".to_string(),
            L1Source::Mainnet => "https://ethereum-rpc.publicnode.com".to_string(),
            L1Source::Custom(url) => url.clone(),
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
    #[arg(short, long, env = "KUP_VERBOSITY", default_value_t = LevelFilter::INFO, global = true)]
    pub verbosity: LevelFilter,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Deploy a new OP Stack network (default command).
    Deploy(DeployArgs),

    /// Clean up containers and network by prefix.
    ///
    /// Stops and removes all containers whose names start with the given prefix,
    /// then removes the associated Docker network (<prefix>-network).
    Cleanup(CleanupArgs),
}

/// Arguments for the cleanup command.
#[derive(Parser)]
pub struct CleanupArgs {
    /// The network name prefix to clean up.
    ///
    /// All containers starting with this prefix will be stopped and removed,
    /// and the network <prefix>-network will be removed.
    #[arg(required = true)]
    pub prefix: String,
}

/// Arguments for the deploy command.
#[derive(Parser)]
pub struct DeployArgs {
    /// A custom name for the network. If not provided, the network will be named:
    /// kup-<l1-chain-name>-<l2-chain-name>.
    #[arg(short, long, visible_alias = "name", env = "KUP_NETWORK_NAME")]
    pub network: Option<String>,

    /// The L1 chain source - either a chain name or RPC URL.
    ///
    /// Accepts:
    /// - Chain names: "sepolia", "mainnet" (uses public RPC endpoints)
    /// - Custom RPC URL: "https://..." (chain ID detected via eth_chainId)
    ///
    /// If not provided, the L1 chain will run in local mode without forking
    /// and with a random chain ID.
    #[arg(long, alias = "l1-chain", env = "KUP_L1")]
    pub l1: Option<L1Source>,

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

    /// Run in detached mode. Deploy the network and exit, leaving containers running.
    #[arg(long, env = "KUP_DETACH")]
    pub detach: bool,

    /// The block time in seconds for the L1 chain (Anvil) and L2 derivation.
    ///
    /// Defaults to 12 seconds (Ethereum mainnet block time).
    #[arg(long, env = "KUP_BLOCK_TIME", default_value_t = 12)]
    pub block_time: u64,

    /// The total number of L2 nodes to deploy.
    ///
    /// This is the sum of sequencers and validators.
    /// Defaults to 5 (2 sequencers + 3 validators).
    #[arg(long, alias = "nodes", env = "KUP_L2_NODES", default_value_t = 5)]
    pub l2_nodes: usize,

    /// The number of sequencer nodes to deploy.
    ///
    /// If more than 1 sequencer is specified, op-conductor will be deployed
    /// to coordinate the sequencers using Raft consensus.
    /// Must be at least 1 and at most equal to l2_nodes.
    /// Defaults to 2 (2 sequencers).
    #[arg(
        long,
        alias = "sequencers",
        env = "KUP_SEQUENCERS",
        default_value_t = 2
    )]
    pub sequencer_count: usize,

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

impl Default for DeployArgs {
    fn default() -> Self {
        Self {
            network: None,
            l1: None, // Local mode by default (random chain ID)
            l2_chain: None,
            redeploy: false,
            outdata: None,
            no_cleanup: false,
            detach: false,
            block_time: 12,
            l2_nodes: 5,
            sequencer_count: 2,
            config: None,
            docker_images: DockerImageOverrides::default(),
        }
    }
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

    /// Docker image for op-conductor.
    #[arg(long, env = "KUP_OP_CONDUCTOR_IMAGE", default_value = OP_CONDUCTOR_DEFAULT_IMAGE)]
    pub op_conductor_image: String,

    /// Docker tag for op-conductor.
    #[arg(long, env = "KUP_OP_CONDUCTOR_TAG", default_value = OP_CONDUCTOR_DEFAULT_TAG)]
    pub op_conductor_tag: String,

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

impl Default for DockerImageOverrides {
    fn default() -> Self {
        Self {
            anvil_image: ANVIL_DEFAULT_IMAGE.to_string(),
            anvil_tag: ANVIL_DEFAULT_TAG.to_string(),
            op_reth_image: OP_RETH_DEFAULT_IMAGE.to_string(),
            op_reth_tag: OP_RETH_DEFAULT_TAG.to_string(),
            kona_node_image: KONA_NODE_DEFAULT_IMAGE.to_string(),
            kona_node_tag: KONA_NODE_DEFAULT_TAG.to_string(),
            op_batcher_image: OP_BATCHER_DEFAULT_IMAGE.to_string(),
            op_batcher_tag: OP_BATCHER_DEFAULT_TAG.to_string(),
            op_proposer_image: OP_PROPOSER_DEFAULT_IMAGE.to_string(),
            op_proposer_tag: OP_PROPOSER_DEFAULT_TAG.to_string(),
            op_challenger_image: OP_CHALLENGER_DEFAULT_IMAGE.to_string(),
            op_challenger_tag: OP_CHALLENGER_DEFAULT_TAG.to_string(),
            op_conductor_image: OP_CONDUCTOR_DEFAULT_IMAGE.to_string(),
            op_conductor_tag: OP_CONDUCTOR_DEFAULT_TAG.to_string(),
            op_deployer_image: OP_DEPLOYER_DEFAULT_IMAGE.to_string(),
            op_deployer_tag: OP_DEPLOYER_DEFAULT_TAG.to_string(),
            prometheus_image: PROMETHEUS_DEFAULT_IMAGE.to_string(),
            prometheus_tag: PROMETHEUS_DEFAULT_TAG.to_string(),
            grafana_image: GRAFANA_DEFAULT_IMAGE.to_string(),
            grafana_tag: GRAFANA_DEFAULT_TAG.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l1_source_parse_sepolia() {
        let source: L1Source = "sepolia".parse().unwrap();
        assert_eq!(source, L1Source::Sepolia);

        // Case insensitive
        let source: L1Source = "SEPOLIA".parse().unwrap();
        assert_eq!(source, L1Source::Sepolia);

        let source: L1Source = "Sepolia".parse().unwrap();
        assert_eq!(source, L1Source::Sepolia);
    }

    #[test]
    fn test_l1_source_parse_mainnet() {
        let source: L1Source = "mainnet".parse().unwrap();
        assert_eq!(source, L1Source::Mainnet);

        // Case insensitive
        let source: L1Source = "MAINNET".parse().unwrap();
        assert_eq!(source, L1Source::Mainnet);
    }

    #[test]
    fn test_l1_source_parse_custom_url() {
        let url = "https://my-custom-rpc.example.com";
        let source: L1Source = url.parse().unwrap();
        assert_eq!(source, L1Source::Custom(url.to_string()));

        // Any unknown string becomes a custom URL
        let source: L1Source = "http://localhost:8545".parse().unwrap();
        assert_eq!(source, L1Source::Custom("http://localhost:8545".to_string()));
    }

    #[test]
    fn test_l1_source_rpc_url() {
        assert_eq!(
            L1Source::Sepolia.rpc_url(),
            "https://ethereum-sepolia-rpc.publicnode.com"
        );
        assert_eq!(
            L1Source::Mainnet.rpc_url(),
            "https://ethereum-rpc.publicnode.com"
        );

        let custom_url = "https://my-rpc.example.com";
        assert_eq!(L1Source::Custom(custom_url.to_string()).rpc_url(), custom_url);
    }
}
