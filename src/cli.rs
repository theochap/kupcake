use clap::Parser;
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
    /// If not provided, the L1 chain will be started with an empty state.
    ///
    /// If public node is selected, anvil will be started in fork-mode using a node from `<https://publicnode.com/>`
    #[arg(long, alias = "l1-rpc", env = "KUP_L1_RPC_URL")]
    pub l1_rpc_provider: Option<L1Provider>,

    /// The L1 chain info (chain ID or name).
    #[arg(long, alias = "l1", env = "KUP_L1_CHAIN", default_value_t = DEFAULT_L1_CHAIN_INFO)]
    pub l1_chain: L1Chain,

    /// The L2 chain info (chain ID or name).
    /// If not provided, the L2 chain id will be generated randomly.
    #[arg(long, alias = "l2", env = "KUP_L2_CHAIN")]
    pub l2_chain: Option<L2Chain>,

    /// The path to the output data directory.
    ///
    /// If not provided, the data will be stored at: ./data_<network-name>
    #[arg(long, alias = "outdata", env = "KUP_OUTDATA")]
    pub outdata: Option<OutData>,

    /// Skips the cleanup of docker containers when the program exits.
    #[arg(long, env = "KUP_NO_CLEANUP")]
    pub no_cleanup: bool,
}
