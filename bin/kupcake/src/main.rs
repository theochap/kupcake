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
    let mut builder = DeployerBuilder::new(cli.l1_chain.to_chain_id())
        .no_cleanup(cli.no_cleanup)
        .dashboards_path(PathBuf::from("grafana/dashboards"));

    // Set L2 chain ID if provided
    if let Some(l2_chain) = cli.l2_chain {
        builder = builder.l2_chain_id(l2_chain.to_chain_id());
    }

    // Set network name if provided
    if let Some(network_name) = cli.network {
        builder = builder.network_name(network_name);
    }

    // Set output data path if provided
    if let Some(outdata) = cli.outdata {
        let outdata_path = match outdata {
            OutData::TempDir => OutDataPath::TempDir,
            OutData::Path(path) => OutDataPath::Path(PathBuf::from(path)),
        };
        builder = builder.outdata(outdata_path);
    }

    // Set L1 RPC URL if available
    if let Ok(rpc_url) = cli.l1_rpc_provider.to_rpc_url(cli.l1_chain) {
        builder = builder.l1_rpc_url(rpc_url);
    }

    // Build the deployer configuration
    let deployer = builder.build().await?;

    // Save the configuration to kupconf.toml before deploying
    deployer.save_config()?;

    deployer.deploy(cli.redeploy).await?;

    Ok(())
}
