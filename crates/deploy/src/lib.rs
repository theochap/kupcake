//! kupcake-deploy - Deployment library for the OP Stack.
//!
//! This crate provides the deployment functionality for bootstrapping a rust-based
//! OP Stack chain.

use alloy_core::primitives::Bytes;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod docker;
mod fs;
pub mod services;

pub use docker::{
    KupDocker, KupDockerConfig, PortMapping, PortProtocol, ServiceConfig, ServiceHandler,
};
pub use services::{
    AnvilConfig, AnvilHandler, GrafanaConfig, KonaNodeConfig, KonaNodeHandler, MetricsTarget,
    MonitoringConfig, OpBatcherConfig, OpBatcherHandler, OpChallengerConfig, OpChallengerHandler,
    OpDeployerConfig, OpProposerConfig, OpProposerHandler, OpRethConfig, OpRethHandler,
    PrometheusConfig,
};

/// Account information from Anvil.
pub struct AccountInfo {
    pub address: Bytes,
    pub private_key: Bytes,
}

/// Combined configuration for all L2 node components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2NodesConfig {
    /// Configuration for op-reth execution client.
    pub op_reth: OpRethConfig,
    /// Configuration for kona-node consensus client.
    pub kona_node: KonaNodeConfig,
    /// Configuration for op-batcher.
    pub op_batcher: OpBatcherConfig,
    /// Configuration for op-proposer.
    pub op_proposer: OpProposerConfig,
    /// Configuration for op-challenger.
    pub op_challenger: OpChallengerConfig,
}

impl Default for L2NodesConfig {
    fn default() -> Self {
        Self {
            op_reth: OpRethConfig::default(),
            kona_node: KonaNodeConfig::default(),
            op_batcher: OpBatcherConfig::default(),
            op_proposer: OpProposerConfig::default(),
            op_challenger: OpChallengerConfig::default(),
        }
    }
}

/// Handler for the complete L2 node setup.
pub struct L2NodesHandler {
    pub op_reth: OpRethHandler,
    pub kona_node: KonaNodeHandler,
    pub op_batcher: OpBatcherHandler,
    pub op_proposer: OpProposerHandler,
    pub op_challenger: OpChallengerHandler,
}

/// Main deployer that orchestrates the entire OP Stack deployment.
pub struct Deployer {
    pub l1_chain_id: u64,
    pub l2_chain_id: u64,
    pub outdata: PathBuf,

    pub anvil_config: AnvilConfig,
    pub op_deployer_config: OpDeployerConfig,
    pub docker_config: KupDockerConfig,
    pub l2_nodes_config: L2NodesConfig,
    pub monitoring_config: MonitoringConfig,

    /// Path to the dashboards directory (optional).
    pub dashboards_path: Option<PathBuf>,
}

impl Deployer {
    /// Generate a JWT secret for authenticated communication between op-reth and kona-node.
    fn generate_jwt_secret() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let secret: [u8; 32] = rng.random();
        hex::encode(secret)
    }

    /// Write the JWT secret to a file.
    async fn write_jwt_secret(host_config_path: &PathBuf) -> Result<PathBuf, anyhow::Error> {
        let jwt_secret = Self::generate_jwt_secret();
        let jwt_path = host_config_path.join("jwt.hex");

        tokio::fs::write(&jwt_path, &jwt_secret)
            .await
            .context("Failed to write JWT secret file")?;

        tracing::debug!(path = ?jwt_path, "JWT secret written");
        Ok(jwt_path)
    }

    /// Start all L2 node components.
    ///
    /// This starts op-reth first (execution client), then kona-node (consensus client),
    /// followed by op-batcher (batch submitter), op-proposer, and op-challenger.
    /// The components communicate via the Engine API using JWT authentication.
    async fn start_l2_nodes(
        l2_nodes_config: L2NodesConfig,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
    ) -> Result<L2NodesHandler, anyhow::Error> {
        if !host_config_path.exists() {
            fs::FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Generate JWT secret for Engine API authentication
        Self::write_jwt_secret(&host_config_path).await?;

        tracing::info!("Starting op-reth execution client...");

        // Start op-reth first
        let op_reth_handler = l2_nodes_config
            .op_reth
            .start(docker, &host_config_path)
            .await?;

        // Give op-reth a moment to initialize before starting kona-node
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        tracing::info!("Starting kona-node consensus client...");

        // Start kona-node
        let kona_node_handler = l2_nodes_config
            .kona_node
            .start(docker, &host_config_path, anvil_handler, &op_reth_handler)
            .await?;

        // Give kona-node a moment to initialize before starting op-batcher
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        tracing::info!("Starting op-batcher...");

        // Start op-batcher
        let op_batcher_handler = l2_nodes_config
            .op_batcher
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &op_reth_handler,
                &kona_node_handler,
            )
            .await?;

        tracing::info!("Starting op-proposer...");

        // Start op-proposer
        let op_proposer_handler = l2_nodes_config
            .op_proposer
            .start(docker, &host_config_path, anvil_handler, &kona_node_handler)
            .await?;

        tracing::info!("Starting op-challenger...");

        // Start op-challenger
        let op_challenger_handler = l2_nodes_config
            .op_challenger
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &kona_node_handler,
                &l2_nodes_config.op_reth,
            )
            .await?;

        tracing::info!(
            l2_http_rpc = %op_reth_handler.http_rpc_url,
            l2_ws_rpc = %op_reth_handler.ws_rpc_url,
            kona_node_rpc = %kona_node_handler.rpc_url,
            op_batcher_rpc = %op_batcher_handler.rpc_url,
            op_proposer_rpc = %op_proposer_handler.rpc_url,
            op_challenger_rpc = %op_challenger_handler.rpc_url,
            "L2 nodes started successfully"
        );

        Ok(L2NodesHandler {
            op_reth: op_reth_handler,
            kona_node: kona_node_handler,
            op_batcher: op_batcher_handler,
            op_proposer: op_proposer_handler,
            op_challenger: op_challenger_handler,
        })
    }

    /// Build metrics targets for Prometheus scraping from L2 node handlers.
    fn build_metrics_targets(l2_nodes: &L2NodesHandler) -> Vec<MetricsTarget> {
        use services::kona_node::DEFAULT_METRICS_PORT as KONA_METRICS_PORT;
        use services::op_batcher::DEFAULT_METRICS_PORT as BATCHER_METRICS_PORT;
        use services::op_challenger::DEFAULT_METRICS_PORT as CHALLENGER_METRICS_PORT;
        use services::op_proposer::DEFAULT_METRICS_PORT as PROPOSER_METRICS_PORT;
        use services::op_reth::DEFAULT_METRICS_PORT as RETH_METRICS_PORT;

        vec![
            MetricsTarget {
                job_name: "op-reth",
                container_name: l2_nodes.op_reth.container_name.clone(),
                port: RETH_METRICS_PORT,
                service_label: "op-reth",
                layer_label: "execution",
            },
            MetricsTarget {
                job_name: "kona-node",
                container_name: l2_nodes.kona_node.container_name.clone(),
                port: KONA_METRICS_PORT,
                service_label: "kona-node",
                layer_label: "consensus",
            },
            MetricsTarget {
                job_name: "op-batcher",
                container_name: l2_nodes.op_batcher.container_name.clone(),
                port: BATCHER_METRICS_PORT,
                service_label: "op-batcher",
                layer_label: "batcher",
            },
            MetricsTarget {
                job_name: "op-proposer",
                container_name: l2_nodes.op_proposer.container_name.clone(),
                port: PROPOSER_METRICS_PORT,
                service_label: "op-proposer",
                layer_label: "proposer",
            },
            MetricsTarget {
                job_name: "op-challenger",
                container_name: l2_nodes.op_challenger.container_name.clone(),
                port: CHALLENGER_METRICS_PORT,
                service_label: "op-challenger",
                layer_label: "challenger",
            },
        ]
    }

    pub async fn deploy(self) -> Result<()> {
        tracing::info!("Starting deployment process...");

        // Initialize Docker client
        let mut docker = KupDocker::new(self.docker_config)
            .await
            .context("Failed to initialize Docker client")?;

        tracing::info!(
            anvil_config = ?self.anvil_config,
            "Starting Anvil..."
        );

        let anvil = self
            .anvil_config
            .start(&mut docker, self.outdata.join("anvil"), self.l1_chain_id)
            .await?;

        tracing::info!("Deploying L1 contracts...");

        // Deploy L1 contracts - the deployer output goes to the same directory
        // that will be used for L2 nodes config (genesis.json, rollup.json)
        let l2_nodes_data_path = self.outdata.join("deployer");

        self.op_deployer_config
            .deploy_contracts(
                &mut docker,
                l2_nodes_data_path.clone(),
                &anvil,
                self.l1_chain_id,
                self.l2_chain_id,
            )
            .await?;

        tracing::info!(
            "Starting L2 nodes (op-reth + kona-node + op-batcher + op-proposer + op-challenger)..."
        );

        let l2_nodes = Self::start_l2_nodes(
            self.l2_nodes_config,
            &mut docker,
            l2_nodes_data_path.clone(),
            &anvil,
        )
        .await
        .context("Failed to start L2 nodes")?;

        // Start monitoring stack if enabled
        let monitoring = if self.monitoring_config.enabled {
            tracing::info!("Starting monitoring stack (Prometheus + Grafana)...");

            let monitoring_data_path = self.outdata.join("monitoring");
            let metrics_targets = Self::build_metrics_targets(&l2_nodes);

            Some(
                self.monitoring_config
                    .start(
                        &mut docker,
                        monitoring_data_path,
                        metrics_targets,
                        self.dashboards_path,
                    )
                    .await
                    .context("Failed to start monitoring stack")?,
            )
        } else {
            None
        };

        tracing::info!("✓ Deployment complete!");
        tracing::info!("");
        tracing::info!("L1 (Anvil) RPC:       {}", anvil.l1_rpc_url);
        tracing::info!("L2 (op-reth) HTTP:    {}", l2_nodes.op_reth.http_rpc_url);
        tracing::info!("L2 (op-reth) WS:      {}", l2_nodes.op_reth.ws_rpc_url);
        tracing::info!("Kona Node RPC:        {}", l2_nodes.kona_node.rpc_url);
        tracing::info!("Op Batcher RPC:       {}", l2_nodes.op_batcher.rpc_url);
        tracing::info!("Op Proposer RPC:      {}", l2_nodes.op_proposer.rpc_url);
        tracing::info!("Op Challenger RPC:    {}", l2_nodes.op_challenger.rpc_url);

        if let Some(ref mon) = monitoring {
            tracing::info!("Prometheus:           {}", mon.prometheus.url);
            tracing::info!("Grafana:              {}", mon.grafana.url);
        }

        tracing::info!("");

        tracing::info!("Press Ctrl+C to stop all nodes and cleanup.");

        tokio::signal::ctrl_c().await?;

        Ok(())
    }
}

