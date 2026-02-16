use clap::{Parser, Subcommand};
use kupcake_deploy::{
    ANVIL_DEFAULT_IMAGE, ANVIL_DEFAULT_TAG, GRAFANA_DEFAULT_IMAGE, GRAFANA_DEFAULT_TAG,
    KONA_NODE_DEFAULT_IMAGE, KONA_NODE_DEFAULT_TAG, OP_BATCHER_DEFAULT_IMAGE,
    OP_BATCHER_DEFAULT_TAG, OP_CHALLENGER_DEFAULT_IMAGE, OP_CHALLENGER_DEFAULT_TAG,
    OP_CONDUCTOR_DEFAULT_IMAGE, OP_CONDUCTOR_DEFAULT_TAG, OP_DEPLOYER_DEFAULT_IMAGE,
    OP_DEPLOYER_DEFAULT_TAG, OP_PROPOSER_DEFAULT_IMAGE, OP_PROPOSER_DEFAULT_TAG,
    OP_RBUILDER_DEFAULT_IMAGE, OP_RBUILDER_DEFAULT_TAG, OP_RETH_DEFAULT_IMAGE,
    OP_RETH_DEFAULT_TAG, PROMETHEUS_DEFAULT_IMAGE, PROMETHEUS_DEFAULT_TAG,
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

    /// Check the health of a deployed network.
    ///
    /// Loads the Kupcake.toml configuration, verifies containers are running,
    /// queries RPC endpoints, and checks that chain IDs and block production match expectations.
    Health(HealthArgs),

    /// Send ETH to an L2 address via the OptimismPortal deposit mechanism.
    ///
    /// Bridges ETH from the L1 (Anvil) deployer account to a specified L2 address
    /// by calling depositTransaction on the OptimismPortalProxy contract.
    Faucet(FaucetArgs),

    /// Generate continuous L2 traffic using Flashbots Contender.
    ///
    /// Runs a Contender Docker container against a deployed L2 network,
    /// automatically funding the spammer account via the L1→L2 faucet deposit.
    /// Supports built-in scenarios (transfers, erc20, uni_v2) and custom TOML files.
    Spam(SpamArgs),
}

/// Arguments for the health check command.
#[derive(Parser)]
pub struct HealthArgs {
    /// Network name or path to Kupcake.toml / outdata directory.
    ///
    /// If a network name is given (e.g. "kup-nutty-songs"), loads
    /// the config from the default path: ./data-<name>/Kupcake.toml
    /// Otherwise treats the argument as a file/directory path.
    #[arg(required = true)]
    pub config: String,
}

/// Arguments for the faucet command.
#[derive(Parser)]
pub struct FaucetArgs {
    /// Network name or path to Kupcake.toml / outdata directory.
    ///
    /// If a network name is given (e.g. "kup-nutty-songs"), loads
    /// the config from the default path: ./data-<name>/Kupcake.toml
    /// Otherwise treats the argument as a file/directory path.
    #[arg(required = true)]
    pub config: String,

    /// L2 address to receive the ETH (0x-prefixed, 40 hex chars).
    #[arg(long)]
    pub to: String,

    /// Amount of ETH to send.
    #[arg(long, default_value_t = 1.0)]
    pub amount: f64,

    /// Wait for the deposit to appear on L2 before returning.
    #[arg(long)]
    pub wait: bool,
}

/// Arguments for the spam command.
#[derive(Parser)]
pub struct SpamArgs {
    /// Network name or path to Kupcake.toml / outdata directory.
    ///
    /// If a network name is given (e.g. "kup-nutty-songs"), loads
    /// the config from the default path: ./data-<name>/Kupcake.toml
    /// Otherwise treats the argument as a file/directory path.
    #[arg(required = true)]
    pub config: String,

    /// Scenario to run: built-in name (transfers, erc20, uni_v2) or path to a custom TOML file.
    #[arg(long, default_value = "transfers")]
    pub scenario: String,

    /// Transactions per second.
    #[arg(long, default_value_t = 10)]
    pub tps: u64,

    /// Duration in seconds (ignored if --forever is set).
    #[arg(long, default_value_t = 30)]
    pub duration: u64,

    /// Run indefinitely until Ctrl+C.
    #[arg(long)]
    pub forever: bool,

    /// Number of spammer accounts to use.
    #[arg(short, long, default_value_t = 10)]
    pub accounts: u64,

    /// Minimum balance (ETH) for spammer accounts.
    #[arg(long, default_value = "0.1")]
    pub min_balance: String,

    /// Amount of ETH to fund the funder account on L2.
    #[arg(long, default_value_t = 100.0)]
    pub fund_amount: f64,

    /// Index of the funder account in anvil.json (accounts 0-9 are reserved for OP Stack roles).
    #[arg(long, default_value_t = 10)]
    pub funder_account_index: usize,

    /// Generate a report after completion.
    #[arg(long)]
    pub report: bool,

    /// Docker image for Contender.
    #[arg(long, env = "KUP_CONTENDER_IMAGE", default_value = kupcake_deploy::spam::CONTENDER_DEFAULT_IMAGE)]
    pub contender_image: String,

    /// Docker tag for Contender.
    #[arg(long, env = "KUP_CONTENDER_TAG", default_value = kupcake_deploy::spam::CONTENDER_DEFAULT_TAG)]
    pub contender_tag: String,

    /// Target sequencer index (0-based).
    #[arg(long, default_value_t = 0)]
    pub target_node: usize,

    /// Extra arguments to pass directly to contender (after --).
    #[arg(last = true)]
    pub extra_args: Vec<String>,
}

impl SpamArgs {
    /// Resolve the target node index to a Docker-internal RPC URL from the deployer
    /// and build a `SpamConfig`.
    pub fn into_config(
        self,
        deployer: &kupcake_deploy::Deployer,
    ) -> anyhow::Result<kupcake_deploy::spam::SpamConfig> {
        if self.target_node >= deployer.l2_stack.sequencers.len() {
            anyhow::bail!(
                "Target node index {} is out of range (only {} sequencer(s) available)",
                self.target_node,
                deployer.l2_stack.sequencers.len()
            );
        }
        let rpc_url = deployer.l2_stack.sequencers[self.target_node]
            .op_reth
            .docker_rpc_url();

        Ok(kupcake_deploy::spam::SpamConfig {
            scenario: self.scenario,
            tps: self.tps,
            duration: self.duration,
            forever: self.forever,
            accounts: self.accounts,
            min_balance: self.min_balance,
            fund_amount: self.fund_amount,
            funder_account_index: self.funder_account_index,
            report: self.report,
            contender_image: self.contender_image,
            contender_tag: self.contender_tag,
            rpc_url,
            extra_args: self.extra_args,
        })
    }
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

    /// Deploy and immediately start spamming with a named preset.
    ///
    /// Accepts an optional preset name: light, medium, heavy, erc20, uniswap, stress.
    /// If no preset is specified, defaults to "light".
    /// Cannot be combined with --detach.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "light",
        value_name = "PRESET",
        env = "KUP_SPAM",
        conflicts_with = "detach"
    )]
    pub spam: Option<String>,

    /// Publish all exposed container ports to random host ports.
    ///
    /// When enabled, Docker will automatically assign random available ports on the host
    /// for all exposed container ports (equivalent to `docker run -P`).
    /// The custom Docker network is still used for container-to-container communication.
    #[arg(long, env = "KUP_PUBLISH_ALL_PORTS")]
    pub publish_all_ports: bool,

    /// The block time in seconds for the L1 chain (Anvil) and L2 derivation.
    ///
    /// Defaults to 4 seconds to make the initial deployment faster.
    #[arg(long, env = "KUP_BLOCK_TIME", default_value_t = 4)]
    pub block_time: u64,

    /// Manually specify the L2 genesis timestamp (Unix timestamp in seconds).
    ///
    /// When forking from L1, the genesis timestamp is automatically calculated
    /// as: latest_block_timestamp - (block_time * block_number)
    /// This option overrides that calculation and sets an explicit genesis timestamp.
    ///
    /// Use this when you need a specific genesis timestamp for testing or alignment.
    #[arg(long, env = "KUP_GENESIS_TIMESTAMP")]
    pub genesis_timestamp: Option<u64>,

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

    /// Enable flashblocks support.
    ///
    /// When enabled, sequencer nodes use op-rbuilder (a fork of op-reth with
    /// flashblocks capabilities) instead of op-reth. Kona-node's built-in
    /// flashblocks relay connects the sequencer's op-rbuilder to validator nodes.
    #[arg(long, env = "KUP_FLASHBLOCKS")]
    pub flashblocks: bool,

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
            spam: None,
            publish_all_ports: false,
            block_time: 12,
            genesis_timestamp: None,
            l2_nodes: 5,
            sequencer_count: 2,
            flashblocks: false,
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

    /// Docker image for op-rbuilder (flashblocks-enabled execution client).
    #[arg(long, env = "KUP_OP_RBUILDER_IMAGE", default_value = OP_RBUILDER_DEFAULT_IMAGE)]
    pub op_rbuilder_image: String,

    /// Docker tag for op-rbuilder.
    #[arg(long, env = "KUP_OP_RBUILDER_TAG", default_value = OP_RBUILDER_DEFAULT_TAG)]
    pub op_rbuilder_tag: String,

    // Binary path overrides (alternative to Docker images)
    /// Path to a local op-reth binary to use instead of a Docker image.
    ///
    /// When provided, the binary will be copied into a lightweight Docker image.
    /// This is useful for testing local builds.
    #[arg(long, env = "KUP_OP_RETH_BINARY")]
    pub op_reth_binary: Option<String>,

    /// Path to a local kona-node binary to use instead of a Docker image.
    ///
    /// When provided, the binary will be copied into a lightweight Docker image.
    /// This is useful for testing local builds.
    #[arg(long, env = "KUP_KONA_NODE_BINARY")]
    pub kona_node_binary: Option<String>,

    /// Path to a local op-batcher binary to use instead of a Docker image.
    ///
    /// When provided, the binary will be copied into a lightweight Docker image.
    /// This is useful for testing local builds.
    #[arg(long, env = "KUP_OP_BATCHER_BINARY")]
    pub op_batcher_binary: Option<String>,

    /// Path to a local op-proposer binary to use instead of a Docker image.
    ///
    /// When provided, the binary will be copied into a lightweight Docker image.
    /// This is useful for testing local builds.
    #[arg(long, env = "KUP_OP_PROPOSER_BINARY")]
    pub op_proposer_binary: Option<String>,

    /// Path to a local op-challenger binary to use instead of a Docker image.
    ///
    /// When provided, the binary will be copied into a lightweight Docker image.
    /// This is useful for testing local builds.
    #[arg(long, env = "KUP_OP_CHALLENGER_BINARY")]
    pub op_challenger_binary: Option<String>,

    /// Path to a local op-conductor binary to use instead of a Docker image.
    ///
    /// When provided, the binary will be copied into a lightweight Docker image.
    /// This is useful for testing local builds.
    #[arg(long, env = "KUP_OP_CONDUCTOR_BINARY")]
    pub op_conductor_binary: Option<String>,

    /// Path to a local op-rbuilder binary to use instead of a Docker image.
    ///
    /// When provided, the binary will be copied into a lightweight Docker image.
    /// This is useful for testing local builds.
    #[arg(long, env = "KUP_OP_RBUILDER_BINARY")]
    pub op_rbuilder_binary: Option<String>,
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
            op_rbuilder_image: OP_RBUILDER_DEFAULT_IMAGE.to_string(),
            op_rbuilder_tag: OP_RBUILDER_DEFAULT_TAG.to_string(),
            op_reth_binary: None,
            kona_node_binary: None,
            op_batcher_binary: None,
            op_proposer_binary: None,
            op_challenger_binary: None,
            op_conductor_binary: None,
            op_rbuilder_binary: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Helper to parse CLI args, simulating `kupcake <args>`.
    fn parse_cli(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("kupcake").chain(args.iter().copied()))
    }

    /// Extract DeployArgs from parsed CLI (handles both explicit `deploy` and default).
    fn deploy_args(cli: &Cli) -> &DeployArgs {
        match &cli.command {
            Some(Commands::Deploy(args)) => args,
            None => panic!("Expected deploy command (default)"),
            _ => panic!("Expected Deploy, got a different subcommand"),
        }
    }

    // ── --spam flag CLI parsing tests ──

    #[test]
    fn test_spam_flag_absent() {
        let cli = parse_cli(&["deploy"]).unwrap();
        assert!(deploy_args(&cli).spam.is_none());
    }

    #[test]
    fn test_spam_flag_absent_default_command() {
        // No subcommand at all → default deploy (None), no spam to check
        let cli = parse_cli(&[]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_spam_flag_no_value_defaults_to_light() {
        let cli = parse_cli(&["deploy", "--spam"]).unwrap();
        assert_eq!(deploy_args(&cli).spam.as_deref(), Some("light"));
    }

    #[test]
    fn test_spam_flag_with_preset_value() {
        for preset in &["light", "medium", "heavy", "erc20", "uniswap", "stress"] {
            let cli = parse_cli(&["deploy", "--spam", preset]).unwrap();
            assert_eq!(
                deploy_args(&cli).spam.as_deref(),
                Some(*preset),
                "Failed for preset: {}",
                preset
            );
        }
    }

    #[test]
    fn test_spam_flag_accepts_arbitrary_string() {
        // clap accepts any string; validation happens later via SpamPreset::from_str
        let cli = parse_cli(&["deploy", "--spam", "custom-name"]).unwrap();
        assert_eq!(deploy_args(&cli).spam.as_deref(), Some("custom-name"));
    }

    #[test]
    fn test_spam_conflicts_with_detach() {
        let result = parse_cli(&["deploy", "--spam", "--detach"]);
        assert!(result.is_err(), "--spam and --detach should conflict");
        let err = format!("{}", result.err().unwrap());
        assert!(
            err.contains("--spam") || err.contains("--detach"),
            "Error should mention the conflicting flags, got: {}",
            err
        );
    }

    #[test]
    fn test_spam_flag_with_other_deploy_flags() {
        // --spam should work alongside other deploy flags
        let cli = parse_cli(&[
            "deploy",
            "--spam",
            "heavy",
            "--no-cleanup",
            "--block-time",
            "2",
            "--l2-nodes",
            "3",
        ])
        .unwrap();
        let args = deploy_args(&cli);
        assert_eq!(args.spam.as_deref(), Some("heavy"));
        assert!(args.no_cleanup);
        assert_eq!(args.block_time, 2);
        assert_eq!(args.l2_nodes, 3);
    }

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
        assert_eq!(
            source,
            L1Source::Custom("http://localhost:8545".to_string())
        );
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
        assert_eq!(
            L1Source::Custom(custom_url.to_string()).rpc_url(),
            custom_url
        );
    }
}
