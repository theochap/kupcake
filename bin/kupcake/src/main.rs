//! kupcake is a CLI tool to help you bootstrap a rust-based op-stack chain in a few clicks.

mod cli;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use rand::Rng;

use cli::{Cli, OutData};
use kupcake_deploy::{
    AnvilConfig, Deployer, GrafanaConfig, KonaNodeConfig, KupDockerConfig, L2NodesConfig,
    MonitoringConfig, OpBatcherConfig, OpChallengerConfig, OpDeployerConfig, OpProposerConfig,
    OpRethConfig, PrometheusConfig,
};

const FOUNDRY_DOCKER_IMAGE: &str = "ghcr.io/foundry-rs/foundry";
const FOUNDRY_DOCKER_TAG: &str = "latest";

const OP_DEPLOYER_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-deployer";
const OP_DEPLOYER_DOCKER_TAG: &str = "v0.5.0-rc.2";

const KONA_NODE_DOCKER_IMAGE: &str = "ghcr.io/theochap/kona-node";
const KONA_NODE_DOCKER_TAG: &str = "test";

const OP_RETH_DOCKER_IMAGE: &str = "ghcr.io/paradigmxyz/op-reth";
const OP_RETH_DOCKER_TAG: &str = "latest";

const OP_BATCHER_DOCKER_IMAGE: &str = "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-batcher";
const OP_BATCHER_DOCKER_TAG: &str = "v1.16.2";

const OP_PROPOSER_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-proposer";
const OP_PROPOSER_DOCKER_TAG: &str = "develop";

const OP_CHALLENGER_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-challenger";
const OP_CHALLENGER_DOCKER_TAG: &str = "develop";

const PROMETHEUS_DOCKER_IMAGE: &str = "prom/prometheus";
const PROMETHEUS_DOCKER_TAG: &str = "latest";

const GRAFANA_DOCKER_IMAGE: &str = "grafana/grafana";
const GRAFANA_DOCKER_TAG: &str = "latest";

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
            kona_node_docker_image: KONA_NODE_DOCKER_IMAGE.to_string(),
            kona_node_docker_tag: KONA_NODE_DOCKER_TAG.to_string(),
            op_reth_docker_image: OP_RETH_DOCKER_IMAGE.to_string(),
            op_reth_docker_tag: OP_RETH_DOCKER_TAG.to_string(),
            op_batcher_docker_image: OP_BATCHER_DOCKER_IMAGE.to_string(),
            op_batcher_docker_tag: OP_BATCHER_DOCKER_TAG.to_string(),
            op_proposer_docker_image: OP_PROPOSER_DOCKER_IMAGE.to_string(),
            op_proposer_docker_tag: OP_PROPOSER_DOCKER_TAG.to_string(),
            op_challenger_docker_image: OP_CHALLENGER_DOCKER_IMAGE.to_string(),
            op_challenger_docker_tag: OP_CHALLENGER_DOCKER_TAG.to_string(),
            prometheus_docker_image: PROMETHEUS_DOCKER_IMAGE.to_string(),
            prometheus_docker_tag: PROMETHEUS_DOCKER_TAG.to_string(),
            grafana_docker_image: GRAFANA_DOCKER_IMAGE.to_string(),
            grafana_docker_tag: GRAFANA_DOCKER_TAG.to_string(),
            net_name: format!("{}-network", network_name),
            no_cleanup: cli.no_cleanup,
        },

        op_deployer_config: OpDeployerConfig {
            container_name: format!("{}-op-deployer", network_name),
        },

        l2_nodes_config: L2NodesConfig {
            op_reth: OpRethConfig {
                container_name: format!("{}-op-reth", network_name),
                ..Default::default()
            },
            kona_node: KonaNodeConfig {
                container_name: format!("{}-kona-node", network_name),
                ..Default::default()
            },
            op_batcher: OpBatcherConfig {
                container_name: format!("{}-op-batcher", network_name),
                ..Default::default()
            },
            op_proposer: OpProposerConfig {
                container_name: format!("{}-op-proposer", network_name),
                ..Default::default()
            },
            op_challenger: OpChallengerConfig {
                container_name: format!("{}-op-challenger", network_name),
                ..Default::default()
            },
        },

        monitoring_config: MonitoringConfig {
            prometheus: PrometheusConfig {
                container_name: format!("{}-prometheus", network_name),
                ..Default::default()
            },
            grafana: GrafanaConfig {
                container_name: format!("{}-grafana", network_name),
                ..Default::default()
            },
            enabled: true,
        },

        // Use the dashboards from the project's grafana/dashboards directory
        dashboards_path: Some(PathBuf::from("grafana/dashboards")),
    };

    deployer.deploy().await?;

    Ok(())
}

