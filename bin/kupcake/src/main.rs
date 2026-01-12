//! kupcake is a CLI tool to help you bootstrap a rust-based op-stack chain in a few clicks.

mod cli;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, CleanupArgs, Commands, DeployArgs, OutData};
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

        deployer.deploy(args.redeploy).await?;

        return Ok(());
    }

    // Otherwise, create a new deployment from CLI arguments
    let deployer = DeployerBuilder::new(args.l1_chain.to_chain_id())
        .maybe_l2_chain_id(args.l2_chain.map(|c| c.to_chain_id()))
        .maybe_network_name(args.network)
        .maybe_outdata(args.outdata.map(|o| match o {
            OutData::TempDir => OutDataPath::TempDir,
            OutData::Path(path) => OutDataPath::Path(PathBuf::from(path)),
        }))
        .maybe_l1_rpc_url(args.l1_rpc_provider.to_rpc_url(args.l1_chain).ok())
        .no_cleanup(args.no_cleanup)
        .detach(args.detach)
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
        .grafana_tag(args.docker_images.grafana_tag)
        .dashboards_path(PathBuf::from("grafana/dashboards"))
        .build()
        .await?;

    // Save the configuration to kupconf.toml before deploying
    deployer.save_config()?;

    deployer.deploy(args.redeploy).await?;

    Ok(())
}
