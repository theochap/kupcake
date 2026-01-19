//! kupcake is a CLI tool to help you bootstrap a rust-based op-stack chain in a few clicks.

mod cli;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, CleanupArgs, Commands, DeployArgs, L1Source, OutData};
use kupcake_deploy::{Deployer, DeployerBuilder, OutDataPath, cleanup_by_prefix};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize the logger.
    tracing_subscriber::fmt()
        .with_max_level(cli.verbosity)
        .init();

    match cli.command {
        Some(Commands::Cleanup(args)) => run_cleanup(args).await,
        Some(Commands::Deploy(args)) => run_deploy(args).await,
        // Default to deploy with default args when no subcommand is provided
        None => run_deploy(DeployArgs::default()).await,
    }
}

async fn run_cleanup(args: CleanupArgs) -> Result<()> {
    tracing::info!("Cleaning up network with prefix: {}", args.prefix);

    let result = cleanup_by_prefix(&args.prefix).await?;

    if result.containers_removed.is_empty() && result.network_removed.is_none() {
        tracing::info!("Nothing to clean up");
    } else {
        if !result.containers_removed.is_empty() {
            tracing::info!(
                "Removed {} container(s):",
                result.containers_removed.len()
            );
            for name in &result.containers_removed {
                tracing::info!("  - {}", name);
            }
        }
        if let Some(network) = &result.network_removed {
            tracing::info!("Removed network: {}", network);
        }
        tracing::info!("Cleanup completed successfully");
    }

    Ok(())
}

async fn run_deploy(args: DeployArgs) -> Result<()> {
    // If a config file is provided, load it and deploy
    if let Some(config_path) = &args.config {
        let config_path = PathBuf::from(config_path);
        let deployer = Deployer::load_from_file(&config_path)?;

        tracing::info!(
            config_path = %config_path.display(),
            outdata_path = %deployer.outdata.display(),
            l1_chain_id = deployer.l1_chain_id,
            l2_chain_id = deployer.l2_chain_id,
            "Loading deployment from config file..."
        );

        let _result = deployer.deploy(args.redeploy).await?;

        return Ok(());
    }

    // Determine L1 chain ID and RPC URL based on provided arguments
    // - None: local mode with random L1 chain ID
    // - Known chain (sepolia/mainnet): use known chain ID and public RPC
    // - Custom RPC URL: detect chain ID via eth_chainId
    let (l1_chain_id, l1_rpc_url) = resolve_l1_config(args.l1).await?;

    // Create a new deployment from CLI arguments
    let mut deployer_builder = DeployerBuilder::new(l1_chain_id)
        .maybe_l2_chain_id(args.l2_chain.map(|c| c.to_chain_id()))
        .maybe_network_name(args.network)
        .maybe_outdata(args.outdata.map(|o| match o {
            OutData::TempDir => OutDataPath::TempDir,
            OutData::Path(path) => OutDataPath::Path(PathBuf::from(path)),
        }))
        .maybe_l1_rpc_url(l1_rpc_url)
        .no_cleanup(args.no_cleanup)
        .detach(args.detach)
        .publish_all_ports(args.publish_all_ports)
        .block_time(args.block_time)
        .l2_node_count(args.l2_nodes)
        .sequencer_count(args.sequencer_count)
        // Docker images
        .anvil_image(args.docker_images.anvil_image)
        .anvil_tag(args.docker_images.anvil_tag)
        .op_reth_image(args.docker_images.op_reth_image)
        .op_reth_tag(args.docker_images.op_reth_tag)
        .kona_node_image(args.docker_images.kona_node_image)
        .kona_node_tag(args.docker_images.kona_node_tag)
        .op_batcher_image(args.docker_images.op_batcher_image)
        .op_batcher_tag(args.docker_images.op_batcher_tag)
        .op_proposer_image(args.docker_images.op_proposer_image)
        .op_proposer_tag(args.docker_images.op_proposer_tag)
        .op_challenger_image(args.docker_images.op_challenger_image)
        .op_challenger_tag(args.docker_images.op_challenger_tag)
        .op_conductor_image(args.docker_images.op_conductor_image)
        .op_conductor_tag(args.docker_images.op_conductor_tag)
        .op_deployer_image(args.docker_images.op_deployer_image)
        .op_deployer_tag(args.docker_images.op_deployer_tag)
        .prometheus_image(args.docker_images.prometheus_image)
        .prometheus_tag(args.docker_images.prometheus_tag)
        .grafana_image(args.docker_images.grafana_image)
        .grafana_tag(args.docker_images.grafana_tag);

    // Apply binary paths if provided (these override Docker images)
    if let Some(path) = args.docker_images.op_reth_binary {
        deployer_builder = deployer_builder.with_op_reth_binary(path);
    }
    if let Some(path) = args.docker_images.kona_node_binary {
        deployer_builder = deployer_builder.with_kona_node_binary(path);
    }
    if let Some(path) = args.docker_images.op_batcher_binary {
        deployer_builder = deployer_builder.with_op_batcher_binary(path);
    }
    if let Some(path) = args.docker_images.op_proposer_binary {
        deployer_builder = deployer_builder.with_op_proposer_binary(path);
    }
    if let Some(path) = args.docker_images.op_challenger_binary {
        deployer_builder = deployer_builder.with_op_challenger_binary(path);
    }
    if let Some(path) = args.docker_images.op_conductor_binary {
        deployer_builder = deployer_builder.with_op_conductor_binary(path);
    }

    let deployer = deployer_builder
        .dashboards_path(PathBuf::from("grafana/dashboards"))
        .build()
        .await?;

    // Save the configuration to kupconf.toml before deploying
    deployer.save_config()?;

    let _result = deployer.deploy(args.redeploy).await?;

    Ok(())
}

/// Resolve L1 chain ID and RPC URL from CLI arguments.
///
/// Returns `(l1_chain_id, l1_rpc_url)` where `l1_rpc_url` is `None` for local mode.
async fn resolve_l1_config(l1_source: Option<L1Source>) -> Result<(u64, Option<String>)> {
    use rand::Rng;

    let Some(source) = l1_source else {
        // Local mode: no forking, random L1 chain ID
        let chain_id = rand::rng().random_range(10000..=99999);
        tracing::info!(l1_chain_id = chain_id, "Running in local mode without L1 forking");
        return Ok((chain_id, None));
    };

    let rpc_url = source.rpc_url();

    // Detect chain ID via eth_chainId
    tracing::info!(rpc_url = %rpc_url, "Detecting L1 chain ID from RPC...");
    let chain_id = fetch_chain_id(&rpc_url).await?;
    tracing::info!(l1_chain_id = chain_id, rpc_url = %rpc_url, "Detected L1 chain ID");

    Ok((chain_id, Some(rpc_url)))
}

/// Fetch the chain ID from an Ethereum RPC endpoint using eth_chainId.
async fn fetch_chain_id(rpc_url: &str) -> Result<u64> {
    use anyhow::Context;
    use serde_json::{json, Value};

    let client = reqwest::Client::new();
    let response = client
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_chainId",
            "params": [],
            "id": 1
        }))
        .send()
        .await
        .context("Failed to send eth_chainId request")?;

    let body: Value = response
        .json()
        .await
        .context("Failed to parse eth_chainId response")?;

    let chain_id_hex = body["result"]
        .as_str()
        .context("eth_chainId response missing 'result' field")?;

    // Parse hex string (with or without 0x prefix) to u64
    let chain_id = u64::from_str_radix(chain_id_hex.trim_start_matches("0x"), 16)
        .context("Failed to parse chain ID from hex")?;

    Ok(chain_id)
}
