//! kupcake is a CLI tool to help you bootstrap a rust-based op-stack chain in a few clicks.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use rand::Rng;

use kupcake::{
    cli::{Cli, OutData},
    deploy::{AnvilConfig, Deployer, KupDockerConfig, OpDeployerConfig},
};

const FOUNDRY_DOCKER_IMAGE: &str = "ghcr.io/foundry-rs/foundry";
const FOUNDRY_DOCKER_TAG: &str = "latest";

const OP_DEPLOYER_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-deployer";
const OP_DEPLOYER_DOCKER_TAG: &str = "v0.5.0-rc.2";

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize the logger.
    tracing_subscriber::fmt()
        .with_max_level(cli.verbosity)
        .init();

    let l2_chain_id = cli
        .l2_chain
        .map(|l2_chain| l2_chain.to_chain_id())
        .unwrap_or_else(|| rand::rng().random_range(10000..=99999));

    let l1_chain = cli.l1_chain.to_string();

    // If the L2 chain is not provided, use the L2 chain id as the name.
    let l2_chain = cli
        .l2_chain
        .map(|l2_chain| l2_chain.to_string())
        .unwrap_or_else(|| l2_chain_id.to_string());

    // If the network name is not provided, generate a memorable two-word name
    let network_name = cli.network.clone().unwrap_or_else(|| {
        let name = names::Generator::default()
            .next()
            .unwrap_or_else(|| "unknown-network".to_string());
        format!("kup-{}", name)
    });

    let outdata_path = match &cli.outdata {
        None => PathBuf::from(format!("data-{}", network_name)),
        Some(OutData::TempDir) => {
            let temp_dir = tempdir::TempDir::new("data-kup-")
                .context("Failed to create temporary directory")?;
            PathBuf::from(temp_dir.path().to_string_lossy().to_string())
        }
        Some(OutData::Path(path)) => PathBuf::from(path),
    };

    // Create the output data directory if it doesn't exist.
    if !outdata_path.try_exists().context(format!(
        "Failed to check if output data directory exists at path {}. Ensure you provided valid permissions to the directory.",
        outdata_path.display().to_string()
    ))? {
        std::fs::create_dir_all(&outdata_path).context("Failed to create output data directory")?;
    }

    let outdata_path = outdata_path
        .canonicalize()
        .context("Failed to canonicalize output data directory path")?;

    tracing::info!(
        network_name,
        l1_chain,
        l2_chain,
        outdata_path = outdata_path.display().to_string(),
        "Starting OP Stack network..."
    );

    // Deploy the network.
    let deployer = Deployer {
        l1_chain_id: cli.l1_chain.to_chain_id(),
        l2_chain_id,
        outdata: outdata_path.clone(),

        anvil_config: AnvilConfig {
            container_name: format!("{}-anvil", network_name),
            host: "0.0.0.0".to_string(),
            port: 8545,
            fork_url: cli.l1_rpc_provider.to_rpc_url(cli.l1_chain)?,
            extra_args: vec![],
        },

        docker_config: KupDockerConfig {
            foundry_docker_image: FOUNDRY_DOCKER_IMAGE.to_string(),
            foundry_docker_tag: FOUNDRY_DOCKER_TAG.to_string(),
            op_deployer_docker_image: OP_DEPLOYER_DOCKER_IMAGE.to_string(),
            op_deployer_docker_tag: OP_DEPLOYER_DOCKER_TAG.to_string(),
            net_name: format!("{}-network", network_name),
            no_cleanup: cli.no_cleanup,
        },

        op_deployer_config: OpDeployerConfig {
            container_name: format!("{}-op-deployer", network_name),
        },
    };

    deployer.deploy().await?;

    Ok(())
}
