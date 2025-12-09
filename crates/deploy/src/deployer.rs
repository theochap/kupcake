use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{
    AnvilConfig, KupDocker, KupDockerConfig, L2StackBuilder, MetricsTarget, MonitoringConfig,
    OpBatcherHandler, OpChallengerHandler, OpDeployerConfig, OpProposerHandler, services,
    services::l2_node::L2NodeHandler,
};

/// The default name for the kupcake configuration file.
pub const KUPCONF_FILENAME: &str = "Kupcake.toml";

/// Handler for the complete L2 stack setup.
pub struct L2StackHandler {
    /// Handlers for all L2 nodes (sequencer first, then validators).
    pub nodes: Vec<L2NodeHandler>,
    pub op_batcher: OpBatcherHandler,
    pub op_proposer: OpProposerHandler,
    pub op_challenger: OpChallengerHandler,
}

impl L2StackHandler {
    /// Get the sequencer node handler (the first node).
    pub fn sequencer(&self) -> &L2NodeHandler {
        &self.nodes[0]
    }

    /// Get validator node handlers (all nodes except the first).
    pub fn validators(&self) -> Vec<&L2NodeHandler> {
        self.nodes.iter().skip(1).collect()
    }
}

/// Main deployer that orchestrates the entire OP Stack deployment.
///
/// This struct contains all the configuration needed to deploy an OP Stack chain
/// and can be serialized to/from TOML format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployer {
    /// The L1 chain ID.
    pub l1_chain_id: u64,
    /// The L2 chain ID.
    pub l2_chain_id: u64,
    /// Path to the output data directory.
    pub outdata: PathBuf,

    /// Configuration for the Anvil L1 fork.
    pub anvil: AnvilConfig,
    /// Configuration for the OP Deployer.
    pub op_deployer: OpDeployerConfig,
    /// Configuration for the Docker client.
    pub docker: KupDockerConfig,
    /// Configuration for all L2 components for the op-stack.
    #[serde(flatten)]
    pub l2_stack: L2StackBuilder,
    /// Configuration for the monitoring stack.
    pub monitoring: MonitoringConfig,

    /// Path to the dashboards directory (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboards_path: Option<PathBuf>,
}

impl Deployer {
    /// Save the configuration to a TOML file.
    pub fn save_to_file(&self, path: &PathBuf) -> Result<()> {
        let content =
            toml::to_string_pretty(self).context("Failed to serialize deployer config to TOML")?;
        std::fs::write(path, content)
            .context(format!("Failed to write config to {}", path.display()))?;
        tracing::info!(path = %path.display(), "Configuration saved");
        Ok(())
    }

    /// Load the configuration from a TOML file.
    pub fn load_from_file(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "Configuration file or directory not found: {}",
                path.display()
            ));
        }

        let config_path = if path.is_dir() {
            path.join(KUPCONF_FILENAME)
        } else {
            path.to_path_buf()
        };

        let content = std::fs::read_to_string(config_path)
            .context(format!("Failed to read config from {}", path.display()))?;
        let config: Self =
            toml::from_str(&content).context("Failed to parse config file as TOML")?;
        tracing::info!(path = %path.display(), "Configuration loaded");
        Ok(config)
    }

    /// Save the deployer's configuration to the default location (kupconf.toml in outdata).
    pub fn save_config(&self) -> Result<PathBuf> {
        let config_path = self.outdata.join(KUPCONF_FILENAME);
        self.save_to_file(&config_path)?;
        Ok(config_path)
    }
}

impl Deployer {
    /// Build metrics targets for Prometheus scraping from L2 stack handlers.
    fn build_metrics_targets(l2_stack: &L2StackHandler) -> Vec<MetricsTarget> {
        use services::kona_node::DEFAULT_METRICS_PORT as KONA_METRICS_PORT;
        use services::op_batcher::DEFAULT_METRICS_PORT as BATCHER_METRICS_PORT;
        use services::op_challenger::DEFAULT_METRICS_PORT as CHALLENGER_METRICS_PORT;
        use services::op_proposer::DEFAULT_METRICS_PORT as PROPOSER_METRICS_PORT;
        use services::op_reth::DEFAULT_METRICS_PORT as RETH_METRICS_PORT;

        let mut targets = Vec::new();

        // Add metrics targets for all L2 nodes (sequencer + validators)
        for (i, node) in l2_stack.nodes.iter().enumerate() {
            let role = if i == 0 { "sequencer" } else { "validator" };
            let suffix = if i == 0 {
                String::new()
            } else {
                format!("-{}", i)
            };

            targets.push(MetricsTarget {
                job_name: format!("op-reth{}", suffix),
                container_name: node.op_reth.container_name.clone(),
                port: RETH_METRICS_PORT,
                service_label: format!("op-reth-{}", role),
                layer_label: "execution".to_string(),
            });

            targets.push(MetricsTarget {
                job_name: format!("kona-node{}", suffix),
                container_name: node.kona_node.container_name.clone(),
                port: KONA_METRICS_PORT,
                service_label: format!("kona-node-{}", role),
                layer_label: "consensus".to_string(),
            });
        }

        // Add metrics targets for batcher, proposer, and challenger
        targets.push(MetricsTarget {
            job_name: "op-batcher".to_string(),
            container_name: l2_stack.op_batcher.container_name.clone(),
            port: BATCHER_METRICS_PORT,
            service_label: "op-batcher".to_string(),
            layer_label: "batcher".to_string(),
        });

        targets.push(MetricsTarget {
            job_name: "op-proposer".to_string(),
            container_name: l2_stack.op_proposer.container_name.clone(),
            port: PROPOSER_METRICS_PORT,
            service_label: "op-proposer".to_string(),
            layer_label: "proposer".to_string(),
        });

        targets.push(MetricsTarget {
            job_name: "op-challenger".to_string(),
            container_name: l2_stack.op_challenger.container_name.clone(),
            port: CHALLENGER_METRICS_PORT,
            service_label: "op-challenger".to_string(),
            layer_label: "challenger".to_string(),
        });

        targets
    }

    pub async fn deploy(self, force_deploy: bool) -> Result<()> {
        tracing::info!("Starting deployment process...");

        // Initialize Docker client
        let mut docker = KupDocker::new(self.docker)
            .await
            .context("Failed to initialize Docker client")?;

        tracing::info!(
            anvil_config = ?self.anvil,
            "Starting Anvil..."
        );

        let anvil = self
            .anvil
            .start(&mut docker, self.outdata.join("anvil"), self.l1_chain_id)
            .await?;

        // Deploy L1 contracts - the deployer output goes to the same directory
        // that will be used for L2 nodes config (genesis.json, rollup.json)
        let l2_nodes_data_path = self.outdata.join("l2-stack");

        // Deploy L1 contracts if force_deploy is true or if the deployer output directory does not exist yet.
        if force_deploy || !l2_nodes_data_path.exists() {
            tracing::info!("Deploying L1 contracts...");

            self.op_deployer
                .deploy_contracts(
                    &mut docker,
                    l2_nodes_data_path.clone(),
                    &anvil,
                    self.l1_chain_id,
                    self.l2_chain_id,
                )
                .await?;
        } else {
            tracing::info!("L1 contracts already deployed, skipping deployment");
        }

        let node_count = self.l2_stack.nodes.len();
        tracing::info!(
            node_count,
            "Starting L2 stack ({} node(s) + op-batcher + op-proposer + op-challenger)...",
            node_count
        );

        let l2_stack = self
            .l2_stack
            .start(&mut docker, l2_nodes_data_path.clone(), &anvil)
            .await
            .context("Failed to start L2 stack")?;

        // Start monitoring stack if enabled
        let monitoring = if self.monitoring.enabled {
            tracing::info!("Starting monitoring stack (Prometheus + Grafana)...");

            let monitoring_data_path = self.outdata.join("monitoring");
            let metrics_targets = Self::build_metrics_targets(&l2_stack);

            Some(
                self.monitoring
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
        tracing::info!("=== Host-accessible endpoints (curl from your terminal) ===");
        if let Some(ref url) = anvil.l1_host_url {
            tracing::info!("L1 (Anvil) RPC:       {}", url);
        }

        // Log endpoints for all L2 nodes
        for (i, node) in l2_stack.nodes.iter().enumerate() {
            let role = if i == 0 { "sequencer" } else { "validator" };
            if let Some(ref url) = node.op_reth.http_host_url {
                tracing::info!("L2 {} (op-reth) HTTP:    {}", role, url);
            }
            if let Some(ref url) = node.op_reth.ws_host_url {
                tracing::info!("L2 {} (op-reth) WS:      {}", role, url);
            }
            if let Some(ref url) = node.kona_node.rpc_host_url {
                tracing::info!("L2 {} (kona-node) RPC:   {}", role, url);
            }
        }

        if let Some(ref url) = l2_stack.op_batcher.rpc_host_url {
            tracing::info!("L2 (op-batcher) RPC:  {}", url);
        }
        if let Some(ref mon) = monitoring {
            if let Some(ref url) = mon.prometheus.host_url {
                tracing::info!("Prometheus:           {}", url);
            }
            if let Some(ref url) = mon.grafana.host_url {
                tracing::info!("Grafana:              {}", url);
            }
        }
        tracing::info!("");
        tracing::info!("=== Internal Docker network endpoints ===");
        tracing::info!("L1 (Anvil) RPC:       {}", anvil.l1_rpc_url);

        // Log internal endpoints for all L2 nodes
        for (i, node) in l2_stack.nodes.iter().enumerate() {
            let role = if i == 0 { "sequencer" } else { "validator" };
            tracing::info!(
                "L2 {} (op-reth) HTTP:    {}",
                role,
                node.op_reth.http_rpc_url
            );
            tracing::info!("L2 {} (op-reth) WS:      {}", role, node.op_reth.ws_rpc_url);
            tracing::info!("L2 {} (kona-node) RPC:   {}", role, node.kona_node.rpc_url);
        }

        tracing::info!("Op Batcher RPC:       {}", l2_stack.op_batcher.rpc_url);
        tracing::info!("Op Proposer RPC:      {}", l2_stack.op_proposer.rpc_url);
        tracing::info!("Op Challenger RPC:    {}", l2_stack.op_challenger.rpc_url);

        tracing::info!("");

        tracing::info!("Press Ctrl+C to stop all nodes and cleanup.");

        tokio::signal::ctrl_c().await?;

        Ok(())
    }
}
