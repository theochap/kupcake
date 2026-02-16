use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{
    AnvilConfig, AnvilHandler, DeploymentConfigHash, DeploymentVersion, KupDocker, KupDockerConfig,
    L2StackBuilder, MetricsTarget, MonitoringConfig, OpBatcherHandler, OpChallengerHandler,
    OpDeployerConfig, OpProposerHandler, services, services::l2_node::L2NodeHandler,
    services::MonitoringHandler,
};

/// The default name for the kupcake configuration file.
pub const KUPCONF_FILENAME: &str = "Kupcake.toml";

/// Handler for the complete L2 stack setup.
pub struct L2StackHandler {
    /// Handlers for sequencer nodes.
    pub sequencers: Vec<L2NodeHandler>,
    /// Handlers for validator nodes.
    pub validators: Vec<L2NodeHandler>,
    pub op_batcher: OpBatcherHandler,
    pub op_proposer: OpProposerHandler,
    pub op_challenger: OpChallengerHandler,
}

/// Deployment result containing all service handlers.
///
/// This is returned by `Deployer::deploy()` and provides access to all running containers.
pub struct DeploymentResult {
    /// Handler for the L1 Anvil instance.
    pub anvil: AnvilHandler,
    /// Handlers for all L2 stack components.
    pub l2_stack: L2StackHandler,
    /// Docker client handle. Containers are cleaned up when this is dropped
    /// (unless `no_cleanup` was set).
    _docker: KupDocker,
}

impl L2StackHandler {
    /// Get the primary sequencer node handler (the first sequencer).
    pub fn primary_sequencer(&self) -> &L2NodeHandler {
        &self.sequencers[0]
    }

    /// Get the total number of L2 nodes (sequencers + validators).
    pub fn node_count(&self) -> usize {
        self.sequencers.len() + self.validators.len()
    }

    /// Iterate over all nodes (sequencers first, then validators).
    pub fn all_nodes(&self) -> impl Iterator<Item = &L2NodeHandler> {
        self.sequencers.iter().chain(self.validators.iter())
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

    /// Whether to run in detached mode (exit after deployment).
    #[serde(default)]
    pub detach: bool,
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

    /// Determine if contract deployment is needed based on configuration hash.
    ///
    /// Returns `true` if contracts should be deployed, `false` if they can be skipped.
    ///
    /// Deployment is needed if:
    /// - `force_deploy` flag is set
    /// - L2 stack directory doesn't exist
    /// - Deployment version file is missing or corrupted
    /// - Configuration hash has changed
    fn needs_contract_deployment(
        force_deploy: bool,
        l2_nodes_data_path: &PathBuf,
        version_file_path: &PathBuf,
        current_hash: &str,
    ) -> bool {
        if force_deploy {
            tracing::info!("Force deploy flag set, redeploying contracts");
            return true;
        }

        if !l2_nodes_data_path.exists() {
            tracing::info!("L2 stack directory does not exist, deploying contracts");
            return true;
        }

        if !version_file_path.exists() {
            tracing::warn!("Deployment version file missing, assuming stale deployment");
            return true;
        }

        match DeploymentVersion::load_from_file(version_file_path) {
            Ok(prev_version) => {
                if prev_version.config_hash == current_hash {
                    tracing::info!(
                        config_hash = %current_hash,
                        "Deployment configuration unchanged, skipping contract deployment"
                    );
                    false
                } else {
                    tracing::info!(
                        previous_hash = %prev_version.config_hash,
                        current_hash = %current_hash,
                        "Deployment configuration changed, redeploying contracts"
                    );
                    true
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Failed to load deployment version file, redeploying contracts"
                );
                true
            }
        }
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

        // Add metrics targets for sequencer nodes
        for (i, node) in l2_stack.sequencers.iter().enumerate() {
            let suffix = if i == 0 {
                String::new()
            } else {
                format!("-sequencer-{}", i)
            };

            targets.push(MetricsTarget {
                job_name: format!("op-reth{}", suffix),
                container_name: node.op_reth.container_name.clone(),
                port: RETH_METRICS_PORT,
                service_label: "op-reth-sequencer".to_string(),
                layer_label: "execution".to_string(),
            });

            targets.push(MetricsTarget {
                job_name: format!("kona-node{}", suffix),
                container_name: node.kona_node.container_name.clone(),
                port: KONA_METRICS_PORT,
                service_label: "kona-node-sequencer".to_string(),
                layer_label: "consensus".to_string(),
            });
        }

        // Add metrics targets for validator nodes
        for (i, node) in l2_stack.validators.iter().enumerate() {
            let suffix = format!("-validator-{}", i + 1);

            targets.push(MetricsTarget {
                job_name: format!("op-reth{}", suffix),
                container_name: node.op_reth.container_name.clone(),
                port: RETH_METRICS_PORT,
                service_label: "op-reth-validator".to_string(),
                layer_label: "execution".to_string(),
            });

            targets.push(MetricsTarget {
                job_name: format!("kona-node{}", suffix),
                container_name: node.kona_node.container_name.clone(),
                port: KONA_METRICS_PORT,
                service_label: "kona-node-validator".to_string(),
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

    /// Print detached mode information including container names and stop command.
    fn print_detached_info(
        outdata: &PathBuf,
        anvil: &AnvilHandler,
        l2_stack: &L2StackHandler,
        monitoring: &Option<MonitoringHandler>,
        network_id: &str,
    ) {
        let mut container_names = Vec::new();

        // Add anvil container
        container_names.push(anvil.container_name.clone());

        // Add all L2 node containers (sequencers and validators)
        for node in l2_stack.all_nodes() {
            container_names.push(node.op_reth.container_name.clone());
            container_names.push(node.kona_node.container_name.clone());

            // Add op-conductor if present (for sequencer nodes)
            if let Some(ref conductor) = node.op_conductor {
                container_names.push(conductor.container_name.clone());
            }
        }

        // Add L2 stack service containers
        container_names.push(l2_stack.op_batcher.container_name.clone());
        container_names.push(l2_stack.op_proposer.container_name.clone());
        container_names.push(l2_stack.op_challenger.container_name.clone());

        // Add monitoring containers if present
        if let Some(mon) = monitoring {
            container_names.push(mon.prometheus.container_name.clone());
            container_names.push(mon.grafana.container_name.clone());
        }

        // Build the docker stop command
        let stop_command = format!(
            "docker stop {} && docker network rm {}",
            container_names.join(" "),
            network_id
        );

        // Print the detached mode information
        tracing::info!("✓ Detached mode enabled. Containers are running in the background.");
        tracing::info!("");
        tracing::info!("Configuration saved to: {}", outdata.join(KUPCONF_FILENAME).display());
        tracing::info!("");
        tracing::info!("Running containers:");
        for name in &container_names {
            tracing::info!("  - {}", name);
        }
        tracing::info!("");
        tracing::info!("Network: {}", network_id);
        tracing::info!("");
        tracing::info!("To stop all containers:");
        tracing::info!("  {}", stop_command);
        tracing::info!("");
        tracing::info!("To view logs:");
        tracing::info!("  docker logs <container-name>");
    }

    pub async fn deploy(self, force_deploy: bool, wait_for_exit: bool) -> Result<DeploymentResult> {
        tracing::info!("Starting deployment process...");

        // Compute hash of current deployment configuration before any moves occur
        let current_config = DeploymentConfigHash::from_deployer(&self);
        let current_hash = current_config.compute_hash();

        // Save values we'll need after self is consumed
        let detach = self.detach;
        let outdata = self.outdata.clone();

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
        let version_file_path = l2_nodes_data_path.join(".deployment-version.json");

        // Determine if we need to deploy contracts
        let needs_deployment = Self::needs_contract_deployment(
            force_deploy,
            &l2_nodes_data_path,
            &version_file_path,
            &current_hash,
        );

        if needs_deployment {
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

            // Save version file after successful deployment
            let version = DeploymentVersion::new(current_hash.clone());
            version
                .save_to_file(&version_file_path)
                .context("Failed to save deployment version")?;

            tracing::info!(
                config_hash = %current_hash,
                "Deployment version saved"
            );
        }

        let node_count = self.l2_stack.node_count();
        tracing::info!(
            node_count,
            sequencer_count = self.l2_stack.sequencers.len(),
            validator_count = self.l2_stack.validators.len(),
            "Starting L2 stack ({} node(s) + op-batcher + op-proposer + op-challenger)...",
            node_count
        );

        let l2_stack = self
            .l2_stack
            .start(
                &mut docker,
                l2_nodes_data_path.clone(),
                &anvil,
                self.l1_chain_id,
            )
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

        // Log endpoints for sequencer nodes
        for (i, node) in l2_stack.sequencers.iter().enumerate() {
            let label = if i == 0 {
                "sequencer".to_string()
            } else {
                format!("sequencer-{}", i)
            };
            if let Some(ref url) = node.op_reth.http_host_url {
                tracing::info!("L2 {} (op-reth) HTTP:    {}", label, url);
            }
            if let Some(ref url) = node.op_reth.ws_host_url {
                tracing::info!("L2 {} (op-reth) WS:      {}", label, url);
            }
            if let Some(ref url) = node.kona_node.rpc_host_url {
                tracing::info!("L2 {} (kona-node) RPC:   {}", label, url);
            }

            // Log op-conductor endpoints if present
            if let Some(ref conductor) = node.op_conductor {
                if let Some(ref url) = conductor.rpc_host_url {
                    tracing::info!("L2 {} (op-conductor) RPC:     {}", label, url);
                }
            }
        }

        // Log endpoints for validator nodes
        for (i, node) in l2_stack.validators.iter().enumerate() {
            let label = format!("validator-{}", i + 1);
            if let Some(ref url) = node.op_reth.http_host_url {
                tracing::info!("L2 {} (op-reth) HTTP:    {}", label, url);
            }
            if let Some(ref url) = node.op_reth.ws_host_url {
                tracing::info!("L2 {} (op-reth) WS:      {}", label, url);
            }
            if let Some(ref url) = node.kona_node.rpc_host_url {
                tracing::info!("L2 {} (kona-node) RPC:   {}", label, url);
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

        // Log internal endpoints for sequencer nodes
        for (i, node) in l2_stack.sequencers.iter().enumerate() {
            let label = if i == 0 {
                "sequencer".to_string()
            } else {
                format!("sequencer-{}", i)
            };
            tracing::info!(
                "L2 {} (op-reth) HTTP:    {}",
                label,
                node.op_reth.http_rpc_url
            );
            tracing::info!(
                "L2 {} (op-reth) WS:      {}",
                label,
                node.op_reth.ws_rpc_url
            );
            tracing::info!("L2 {} (kona-node) RPC:   {}", label, node.kona_node.rpc_url);

            // Log op-conductor internal endpoints if present
            if let Some(ref conductor) = node.op_conductor {
                tracing::info!("L2 {} (op-conductor) RPC:     {}", label, conductor.rpc_url);
            }
        }

        // Log internal endpoints for validator nodes
        for (i, node) in l2_stack.validators.iter().enumerate() {
            let label = format!("validator-{}", i + 1);
            tracing::info!(
                "L2 {} (op-reth) HTTP:    {}",
                label,
                node.op_reth.http_rpc_url
            );
            tracing::info!(
                "L2 {} (op-reth) WS:      {}",
                label,
                node.op_reth.ws_rpc_url
            );
            tracing::info!("L2 {} (kona-node) RPC:   {}", label, node.kona_node.rpc_url);
        }

        tracing::info!("Op Batcher RPC:       {}", l2_stack.op_batcher.rpc_url);
        tracing::info!("Op Proposer RPC:      {}", l2_stack.op_proposer.rpc_url);
        tracing::info!("Op Challenger RPC:    {}", l2_stack.op_challenger.rpc_url);

        tracing::info!("");

        if wait_for_exit {
            if detach {
                // Detached mode: print management info and exit
                Self::print_detached_info(
                    &outdata,
                    &anvil,
                    &l2_stack,
                    &monitoring,
                    &docker.network_id,
                );
            } else {
                // Normal mode: wait for Ctrl+C
                tracing::info!("Press Ctrl+C to stop all nodes and cleanup.");
                tokio::signal::ctrl_c().await?;
            }
        }

        Ok(DeploymentResult { anvil, l2_stack, _docker: docker })
    }
}
