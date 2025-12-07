//! kupcake is a CLI tool to help you bootstrap a rust-based op-stack chain in a few clicks.

mod cli;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, OutData};
use kupcake_deploy::{Deployer, DeployerBuilder, OutDataPath};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize the logger.
    tracing_subscriber::fmt()
        .with_max_level(cli.verbosity)
        .init();

    // If a config file is provided, load it and deploy
    if let Some(config_path) = &cli.config {
        let config_path = PathBuf::from(config_path);
        let deployer = Deployer::load_from_file(&config_path)?;

        tracing::info!(
            config_path = %config_path.display(),
            outdata_path = %deployer.outdata.display(),
            l1_chain_id = deployer.l1_chain_id,
            l2_chain_id = deployer.l2_chain_id,
            "Loading deployment from config file..."
        );

        deployer.deploy(cli.redeploy).await?;

        return Ok(());
    }

    // Otherwise, create a new deployment from CLI arguments
    let deployer = DeployerBuilder::new(cli.l1_chain.to_chain_id())
        .maybe_l2_chain_id(cli.l2_chain.map(|c| c.to_chain_id()))
        .maybe_network_name(cli.network)
        .maybe_outdata(cli.outdata.map(|o| match o {
            OutData::TempDir => OutDataPath::TempDir,
            OutData::Path(path) => OutDataPath::Path(PathBuf::from(path)),
        }))
        .maybe_l1_rpc_url(cli.l1_rpc_provider.to_rpc_url(cli.l1_chain).ok())
        .no_cleanup(cli.no_cleanup)
        .block_time(cli.block_time)
        // Docker images
        .anvil_image(cli.docker_images.anvil_image)
        .anvil_tag(cli.docker_images.anvil_tag)
        .op_reth_image(cli.docker_images.op_reth_image)
        .op_reth_tag(cli.docker_images.op_reth_tag)
        .kona_node_image(cli.docker_images.kona_node_image)
        .kona_node_tag(cli.docker_images.kona_node_tag)
        .op_batcher_image(cli.docker_images.op_batcher_image)
        .op_batcher_tag(cli.docker_images.op_batcher_tag)
        .op_proposer_image(cli.docker_images.op_proposer_image)
        .op_proposer_tag(cli.docker_images.op_proposer_tag)
        .op_challenger_image(cli.docker_images.op_challenger_image)
        .op_challenger_tag(cli.docker_images.op_challenger_tag)
        .op_deployer_image(cli.docker_images.op_deployer_image)
        .op_deployer_tag(cli.docker_images.op_deployer_tag)
        .prometheus_image(cli.docker_images.prometheus_image)
        .prometheus_tag(cli.docker_images.prometheus_tag)
        .grafana_image(cli.docker_images.grafana_image)
        .grafana_tag(cli.docker_images.grafana_tag)
        .dashboards_path(PathBuf::from("grafana/dashboards"))
        .build()
        .await?;

    // Save the configuration to kupconf.toml before deploying
    deployer.save_config()?;

    deployer.deploy(cli.redeploy).await?;

    Ok(())
}
