use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{
    AnvilConfig, AnvilHandler, DeploymentConfigHash, DeploymentTarget, DeploymentVersion,
    KupDocker, KupDockerConfig, L2StackBuilder, MetricsTarget, MonitoringConfig, OpBatcherBuilder,
    OpBatcherHandler, OpChallengerBuilder, OpChallengerHandler, OpDeployerConfig,
    OpProposerBuilder, OpProposerHandler, fs,
    metrics::{DeploymentMetrics, ServiceMetrics, get_image_size},
    service::KupcakeService,
    services,
    services::MonitoringHandler,
    services::anvil::AnvilInput,
    services::l2_node::{L2NodeBuilder, L2NodeHandler},
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
    /// None when restored from snapshot (no state.json available).
    pub op_proposer: Option<OpProposerHandler>,
    /// None when restored from snapshot (no state.json available).
    pub op_challenger: Option<OpChallengerHandler>,
}

/// Deployment result containing all service handlers.
///
/// This is returned by `Deployer::deploy()` and provides access to all running containers.
pub struct DeploymentResult {
    /// Handler for the L1 Anvil instance.
    pub anvil: AnvilHandler,
    /// Handlers for all L2 stack components.
    pub l2_stack: L2StackHandler,
    /// Deployment metrics (per-service timings and image sizes).
    pub metrics: DeploymentMetrics,
    /// Monitoring stack handlers (if enabled).
    pub monitoring: Option<MonitoringHandler>,
}

/// Endpoints for a single service.
#[derive(Debug, Serialize)]
pub struct ServiceEndpoints {
    /// Endpoints on the internal Docker network.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub internal: BTreeMap<String, String>,
    /// Host-accessible endpoints.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub host: BTreeMap<String, String>,
}

/// All deployment endpoints, keyed by service label.
#[derive(Debug, Serialize)]
pub struct DeploymentEndpoints {
    pub services: BTreeMap<String, ServiceEndpoints>,
}

impl DeploymentEndpoints {
    /// Write endpoints to a TOML file.
    pub fn write_to_file(&self, path: &Path) -> Result<()> {
        let content =
            toml::to_string_pretty(self).context("Failed to serialize endpoints to TOML")?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write endpoints to {}", path.display()))?;
        tracing::info!(path = %path.display(), "Deployment endpoints written to file");
        Ok(())
    }
}

impl DeploymentResult {
    /// Collect all endpoints from the deployment into a structured format.
    pub fn endpoints(&self) -> DeploymentEndpoints {
        let monitoring = self.monitoring.as_ref();
        let mut services = BTreeMap::new();

        // Anvil
        {
            let mut internal = BTreeMap::new();
            let mut host = BTreeMap::new();
            internal.insert("rpc".to_string(), self.anvil.l1_rpc_url.to_string());
            if let Some(ref url) = self.anvil.l1_host_url {
                host.insert("rpc".to_string(), url.to_string());
            }
            services.insert(
                self.anvil.container_name.clone(),
                ServiceEndpoints { internal, host },
            );
        }

        // Sequencer nodes
        for (i, node) in self.l2_stack.sequencers.iter().enumerate() {
            let label = if i == 0 {
                "sequencer".to_string()
            } else {
                format!("sequencer-{}", i)
            };
            Self::collect_l2_node_endpoints(&mut services, node, &label);
        }

        // Validator nodes
        for (i, node) in self.l2_stack.validators.iter().enumerate() {
            let label = format!("validator-{}", i + 1);
            Self::collect_l2_node_endpoints(&mut services, node, &label);
        }

        // op-batcher
        {
            let mut internal = BTreeMap::new();
            let mut host = BTreeMap::new();
            internal.insert(
                "rpc".to_string(),
                self.l2_stack.op_batcher.rpc_url.to_string(),
            );
            if let Some(ref url) = self.l2_stack.op_batcher.rpc_host_url {
                host.insert("rpc".to_string(), url.to_string());
            }
            services.insert(
                self.l2_stack.op_batcher.container_name.clone(),
                ServiceEndpoints { internal, host },
            );
        }

        // op-proposer
        if let Some(ref proposer) = self.l2_stack.op_proposer {
            let mut internal = BTreeMap::new();
            internal.insert("rpc".to_string(), proposer.rpc_url.to_string());
            services.insert(
                proposer.container_name.clone(),
                ServiceEndpoints {
                    internal,
                    host: BTreeMap::new(),
                },
            );
        }

        // op-challenger
        if let Some(ref challenger) = self.l2_stack.op_challenger {
            let mut internal = BTreeMap::new();
            internal.insert("metrics".to_string(), challenger.metrics_url.to_string());
            services.insert(
                challenger.container_name.clone(),
                ServiceEndpoints {
                    internal,
                    host: BTreeMap::new(),
                },
            );
        }

        // Monitoring
        if let Some(mon) = monitoring {
            {
                let mut internal = BTreeMap::new();
                let mut host = BTreeMap::new();
                internal.insert("url".to_string(), mon.prometheus.url.to_string());
                if let Some(ref url) = mon.prometheus.host_url {
                    host.insert("url".to_string(), url.to_string());
                }
                services.insert(
                    mon.prometheus.container_name.clone(),
                    ServiceEndpoints { internal, host },
                );
            }
            {
                let mut internal = BTreeMap::new();
                let mut host = BTreeMap::new();
                internal.insert("url".to_string(), mon.grafana.url.to_string());
                if let Some(ref url) = mon.grafana.host_url {
                    host.insert("url".to_string(), url.to_string());
                }
                services.insert(
                    mon.grafana.container_name.clone(),
                    ServiceEndpoints { internal, host },
                );
            }
        }

        DeploymentEndpoints { services }
    }

    /// Collect endpoints for an L2 node (op-reth + kona-node + optional op-conductor).
    fn collect_l2_node_endpoints(
        services: &mut BTreeMap<String, ServiceEndpoints>,
        node: &L2NodeHandler,
        _label: &str,
    ) {
        // op-reth
        {
            let mut internal = BTreeMap::new();
            let mut host = BTreeMap::new();
            internal.insert("http".to_string(), node.op_reth.http_rpc_url.to_string());
            internal.insert("ws".to_string(), node.op_reth.ws_rpc_url.to_string());
            internal.insert("authrpc".to_string(), node.op_reth.authrpc_url.to_string());
            if let Some(ref url) = node.op_reth.http_host_url {
                host.insert("http".to_string(), url.to_string());
            }
            if let Some(ref url) = node.op_reth.ws_host_url {
                host.insert("ws".to_string(), url.to_string());
            }
            if let Some(ref url) = node.op_reth.flashblocks_ws_url {
                internal.insert("flashblocks_ws".to_string(), url.to_string());
            }
            services.insert(
                node.op_reth.container_name.clone(),
                ServiceEndpoints { internal, host },
            );
        }

        // kona-node
        {
            let mut internal = BTreeMap::new();
            let mut host = BTreeMap::new();
            internal.insert("rpc".to_string(), node.kona_node.rpc_url.to_string());
            if let Some(ref url) = node.kona_node.rpc_host_url {
                host.insert("rpc".to_string(), url.to_string());
            }
            if let Some(ref url) = node.kona_node.metrics_host_url {
                host.insert("metrics".to_string(), url.to_string());
            }
            if let Some(ref url) = node.kona_node.flashblocks_relay_url {
                internal.insert("flashblocks_relay".to_string(), url.to_string());
            }
            services.insert(
                node.kona_node.container_name.clone(),
                ServiceEndpoints { internal, host },
            );
        }

        // op-conductor
        if let Some(ref conductor) = node.op_conductor {
            let mut internal = BTreeMap::new();
            let mut host = BTreeMap::new();
            internal.insert("rpc".to_string(), conductor.rpc_url.to_string());
            if let Some(ref url) = conductor.rpc_host_url {
                host.insert("rpc".to_string(), url.to_string());
            }
            services.insert(
                conductor.container_name.clone(),
                ServiceEndpoints { internal, host },
            );
        }
    }
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
///
/// The type parameters allow swapping implementations:
/// - `L1` — the L1 chain type (default: `AnvilConfig`)
/// - `Node` — the L2 node type (default: `L2NodeBuilder`, which combines op-reth + kona-node)
/// - `B` — the batcher type (default: `OpBatcherBuilder`)
/// - `P` — the proposer type (default: `OpProposerBuilder`)
/// - `C` — the challenger type (default: `OpChallengerBuilder`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "L1: Serialize, Node: Serialize, B: Serialize, P: Serialize, C: Serialize",
    deserialize = "L1: serde::de::DeserializeOwned, Node: serde::de::DeserializeOwned, B: serde::de::DeserializeOwned, P: serde::de::DeserializeOwned, C: serde::de::DeserializeOwned"
))]
pub struct Deployer<
    L1 = AnvilConfig,
    Node = L2NodeBuilder,
    B = OpBatcherBuilder,
    P = OpProposerBuilder,
    C = OpChallengerBuilder,
> {
    /// The L1 chain ID.
    pub l1_chain_id: u64,
    /// The L2 chain ID.
    pub l2_chain_id: u64,
    /// Path to the output data directory.
    pub outdata: PathBuf,

    /// Configuration for the L1 chain.
    pub anvil: L1,
    /// Configuration for the OP Deployer.
    pub op_deployer: OpDeployerConfig,
    /// Configuration for the Docker client.
    pub docker: KupDockerConfig,
    /// Configuration for all L2 components for the op-stack.
    #[serde(flatten)]
    pub l2_stack: L2StackBuilder<Node, B, P, C>,
    /// Configuration for the monitoring stack.
    pub monitoring: MonitoringConfig,

    /// Path to the dashboards directory (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboards_path: Option<PathBuf>,

    /// Whether to run in detached mode (exit after deployment).
    #[serde(default)]
    pub detach: bool,

    /// Path to a snapshot directory for restoring from an existing op-reth database.
    #[serde(skip)]
    pub snapshot: Option<PathBuf>,

    /// When true, copy the snapshot reth database instead of symlinking it.
    #[serde(skip)]
    pub copy_snapshot: bool,

    /// Deployment target for OP Stack contracts (live or genesis).
    #[serde(default)]
    pub deployment_target: crate::DeploymentTarget,

    /// Whether to dump Anvil state via RPC before cleanup.
    #[serde(default = "default_dump_state")]
    pub dump_state: bool,

    /// Optional path to an external state file for Anvil to load via `--load-state`.
    /// Only valid in live mode; genesis mode will error if this is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_state: Option<PathBuf>,
}

fn default_dump_state() -> bool {
    true
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
    pub fn load_from_file(path: &Path) -> Result<Self> {
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

        let content = std::fs::read_to_string(&config_path)
            .context(format!("Failed to read config from {}", path.display()))?;

        // Parse TOML, resolve any `include` directives, then deserialize
        let mut value: toml::Value =
            toml::from_str(&content).context("Failed to parse config file as TOML")?;

        let base_dir = config_path.parent().unwrap_or(Path::new("."));
        crate::config_resolve::resolve_includes(&mut value, base_dir)
            .context("Failed to resolve include directives in config")?;

        let config: Self = value
            .try_into()
            .context("Failed to deserialize config after resolving includes")?;

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
        l2_nodes_data_path: &Path,
        version_file_path: &Path,
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

    /// Check if deployment is needed, and if so, save the version file after the
    /// provided deploy function succeeds. Returns whether deployment was executed.
    async fn with_deployment_check<F, Fut>(
        force_deploy: bool,
        l2_nodes_data_path: &Path,
        current_hash: &str,
        deploy_fn: F,
    ) -> Result<bool>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let version_file_path = l2_nodes_data_path.join(".deployment-version.json");
        let needs_deployment = Self::needs_contract_deployment(
            force_deploy,
            l2_nodes_data_path,
            &version_file_path,
            current_hash,
        );

        if needs_deployment {
            deploy_fn().await?;

            let version = DeploymentVersion::new(current_hash.to_string())?;
            version
                .save_to_file(&version_file_path)
                .context("Failed to save deployment version")?;

            tracing::info!(config_hash = %current_hash, "Deployment version saved");
        }

        Ok(needs_deployment)
    }

    /// Derive Anvil accounts from the default mnemonic.
    pub fn derive_accounts() -> Result<crate::AnvilAccounts> {
        let account_infos = crate::derive_accounts_from_mnemonic(
            crate::ANVIL_DEFAULT_MNEMONIC,
            crate::services::anvil::DEFAULT_ACCOUNT_COUNT,
        )
        .context("Failed to derive accounts from mnemonic")?;
        crate::anvil_accounts_from_infos(account_infos).context("Failed to create named accounts")
    }

    /// Deploy contracts at genesis and start Anvil with the resulting L1 state.
    ///
    /// 1. Derive accounts from mnemonic (Anvil isn't running yet)
    /// 2. Deploy contracts in-memory via op-deployer (if needed)
    /// 3. Extract L1 genesis from state dump
    /// 4. Start Anvil with `--init`
    /// 5. Patch rollup.json with Anvil's actual genesis block hash
    #[allow(clippy::too_many_arguments)]
    async fn deploy_with_genesis_target(
        docker: &mut KupDocker,
        anvil_config: AnvilConfig,
        op_deployer: &OpDeployerConfig,
        l2_nodes_data_path: &Path,
        anvil_data_path: &Path,
        l1_chain_id: u64,
        l2_chain_id: u64,
        force_deploy: bool,
        current_hash: &str,
    ) -> Result<(AnvilHandler, Duration)> {
        let accounts = Self::derive_accounts()?;

        let timestamp = match anvil_config.timestamp {
            Some(t) => t,
            None => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .context("System time is before Unix epoch")?
                .as_secs(),
        };

        let deploy_accounts = accounts.clone();
        let mut op_deployer_duration = Duration::ZERO;
        let op_deployer_start = Instant::now();
        let deployed =
            Self::with_deployment_check(force_deploy, l2_nodes_data_path, current_hash, || async {
                tracing::info!("Genesis deployment mode: deploying contracts at genesis...");

                op_deployer
                    .deploy_contracts_at_genesis(
                        docker,
                        l2_nodes_data_path,
                        &deploy_accounts,
                        l1_chain_id,
                        l2_chain_id,
                        timestamp,
                    )
                    .await
                    .context("Failed to deploy contracts at genesis")?;

                crate::l1_genesis::extract_l1_genesis(
                    &l2_nodes_data_path.join("state.json"),
                    l1_chain_id,
                    timestamp,
                    anvil_data_path,
                )
                .context("Failed to extract L1 genesis from state.json")?;

                Ok(())
            })
            .await?;
        // Capture contract deployment time (before Anvil starts)
        op_deployer_duration += op_deployer_start.elapsed();

        if !deployed {
            tracing::info!("Genesis deployment: reusing existing deployment artifacts");
        }

        let state_json_path = anvil_data_path.join("state.json");

        // If contracts were redeployed, any existing state.json is stale — remove it
        // so Anvil boots fresh from the new genesis.
        if deployed && state_json_path.exists() {
            std::fs::remove_file(&state_json_path)
                .context("Failed to remove stale anvil state.json after redeployment")?;
            tracing::info!("Removed stale anvil state.json after contract redeployment");
        }

        // If a persisted state exists (from a previous run), use --load-state to restore it.
        // The RPC-based state dump (configured in deploy()) handles persisting state
        // before cleanup. Otherwise, boot fresh from genesis with --init.
        let init_mode = if !deployed && state_json_path.exists() {
            tracing::info!("Found persisted Anvil state, restoring via --load-state");
            Some(crate::AnvilInitMode::LoadState(
                "/data/state.json".to_string(),
            ))
        } else {
            Some(crate::AnvilInitMode::Init(
                "/data/l1-genesis.json".to_string(),
            ))
        };

        tracing::info!(anvil_config = ?anvil_config, "Starting Anvil with genesis state...");
        let anvil = anvil_config
            .deploy(
                docker,
                anvil_data_path,
                AnvilInput {
                    chain_id: l1_chain_id,
                    init_mode,
                    accounts,
                },
            )
            .await
            .context("Failed to start Anvil with genesis state")?;

        if deployed {
            let l2_config_start = Instant::now();
            op_deployer
                .generate_l2_config_files(docker, l2_nodes_data_path, l2_chain_id)
                .await
                .context("Failed to generate L2 config files")?;
            op_deployer_duration += l2_config_start.elapsed();
        }

        // Patch rollup.json with Anvil's actual genesis block hash.
        // Anvil's --init flag doesn't include the alloc state root in the genesis
        // block header, so the hash differs from what op-deployer computed.
        // This must run on every start since Anvil recomputes the hash each time.
        let rollup_json_path = l2_nodes_data_path.join("rollup.json");
        let anvil_host_url = anvil
            .l1_host_url
            .as_ref()
            .context("Anvil host URL required for genesis hash patching")?;
        crate::l1_genesis::patch_rollup_l1_genesis_hash(&rollup_json_path, anvil_host_url)
            .await
            .context("Failed to patch rollup.json with actual L1 genesis hash")?;

        Ok((anvil, op_deployer_duration))
    }

    /// Start Anvil and deploy contracts to the live L1 (or restore from snapshot).
    #[allow(clippy::too_many_arguments)]
    async fn deploy_with_live_target(
        docker: &mut KupDocker,
        anvil_config: AnvilConfig,
        op_deployer: &OpDeployerConfig,
        l2_stack: &L2StackBuilder,
        l2_nodes_data_path: &Path,
        anvil_data_path: &Path,
        l1_chain_id: u64,
        l2_chain_id: u64,
        force_deploy: bool,
        current_hash: &str,
        snapshot: Option<&PathBuf>,
        copy_snapshot: bool,
        override_state: Option<&PathBuf>,
    ) -> Result<(AnvilHandler, Duration)> {
        let accounts = Self::derive_accounts()?;

        // Determine init mode for live deployment:
        // 1. --override-state takes precedence: copy file into anvil data dir, use --load-state
        // 2. Existing state.json from previous run: use --load-state
        // 3. Otherwise: fresh start (no init mode, Anvil starts empty or forks)
        let init_mode = match override_state {
            Some(override_path) => {
                let dest = anvil_data_path.join("override-state.json");
                std::fs::copy(override_path, &dest).with_context(|| {
                    format!(
                        "Failed to copy override state from {} to {}",
                        override_path.display(),
                        dest.display()
                    )
                })?;
                tracing::info!(
                    source = %override_path.display(),
                    "Loading external Anvil state via --load-state"
                );
                Some(crate::AnvilInitMode::LoadState(
                    "/data/override-state.json".to_string(),
                ))
            }
            None if anvil_data_path.join("state.json").exists() => {
                tracing::info!("Found persisted Anvil state, restoring via --load-state");
                Some(crate::AnvilInitMode::LoadState(
                    "/data/state.json".to_string(),
                ))
            }
            None => None,
        };

        tracing::info!(anvil_config = ?anvil_config, "Starting Anvil...");

        let anvil = anvil_config
            .deploy(
                docker,
                anvil_data_path,
                AnvilInput {
                    chain_id: l1_chain_id,
                    init_mode,
                    accounts,
                },
            )
            .await?;

        let op_deployer_start = Instant::now();
        if let Some(snapshot_path) = snapshot {
            let sequencer_name = l2_stack.sequencers[0].op_reth.container_name.clone();
            Self::restore_from_snapshot(
                docker,
                op_deployer,
                &l2_nodes_data_path.to_path_buf(),
                snapshot_path,
                l1_chain_id,
                l2_chain_id,
                &sequencer_name,
                copy_snapshot,
            )
            .await
            .context("Failed to restore from snapshot")?;
        } else {
            Self::with_deployment_check(force_deploy, l2_nodes_data_path, current_hash, || async {
                tracing::info!("Deploying L1 contracts...");

                op_deployer
                    .deploy_contracts(docker, l2_nodes_data_path, &anvil, l1_chain_id, l2_chain_id)
                    .await
            })
            .await?;
        }
        let op_deployer_duration = op_deployer_start.elapsed();

        Ok((anvil, op_deployer_duration))
    }

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

        if let Some(ref proposer) = l2_stack.op_proposer {
            targets.push(MetricsTarget {
                job_name: "op-proposer".to_string(),
                container_name: proposer.container_name.clone(),
                port: PROPOSER_METRICS_PORT,
                service_label: "op-proposer".to_string(),
                layer_label: "proposer".to_string(),
            });
        }

        if let Some(ref challenger) = l2_stack.op_challenger {
            targets.push(MetricsTarget {
                job_name: "op-challenger".to_string(),
                container_name: challenger.container_name.clone(),
                port: CHALLENGER_METRICS_PORT,
                service_label: "op-challenger".to_string(),
                layer_label: "challenger".to_string(),
            });
        }

        targets
    }

    /// Build metrics targets for Prometheus scraping from the deployer's builder config.
    ///
    /// This is used by node lifecycle operations (add/remove) where runtime handlers
    /// are not available but Prometheus scrape targets need to be regenerated.
    /// All values (ports, job names, labels) are derived from the saved config.
    pub fn build_metrics_targets_from_config(&self) -> Vec<MetricsTarget> {
        // Derive the network prefix from the Docker network name.
        // E.g., "kup-mynet-network" → "kup-mynet-"
        let network_prefix = self
            .docker
            .net_name
            .strip_suffix("-network")
            .unwrap_or(&self.docker.net_name);
        let network_prefix = format!("{}-", network_prefix);

        let job_name = |container_name: &str| -> String {
            container_name
                .strip_prefix(&network_prefix)
                .unwrap_or(container_name)
                .to_string()
        };

        let mut targets = Vec::new();

        for node in &self.l2_stack.sequencers {
            let role = node.role.to_string();

            targets.push(MetricsTarget {
                job_name: job_name(&node.op_reth.container_name),
                container_name: node.op_reth.container_name.clone(),
                port: node.op_reth.metrics_port,
                service_label: format!("op-reth-{}", role),
                layer_label: "execution".to_string(),
            });

            targets.push(MetricsTarget {
                job_name: job_name(&node.kona_node.container_name),
                container_name: node.kona_node.container_name.clone(),
                port: node.kona_node.metrics_port,
                service_label: format!("kona-node-{}", role),
                layer_label: "consensus".to_string(),
            });
        }

        for node in &self.l2_stack.validators {
            let role = node.role.to_string();

            targets.push(MetricsTarget {
                job_name: job_name(&node.op_reth.container_name),
                container_name: node.op_reth.container_name.clone(),
                port: node.op_reth.metrics_port,
                service_label: format!("op-reth-{}", role),
                layer_label: "execution".to_string(),
            });

            targets.push(MetricsTarget {
                job_name: job_name(&node.kona_node.container_name),
                container_name: node.kona_node.container_name.clone(),
                port: node.kona_node.metrics_port,
                service_label: format!("kona-node-{}", role),
                layer_label: "consensus".to_string(),
            });
        }

        targets.push(MetricsTarget {
            job_name: job_name(&self.l2_stack.op_batcher.container_name),
            container_name: self.l2_stack.op_batcher.container_name.clone(),
            port: self.l2_stack.op_batcher.metrics_port,
            service_label: "op-batcher".to_string(),
            layer_label: "batcher".to_string(),
        });

        if let Some(ref proposer) = self.l2_stack.op_proposer {
            targets.push(MetricsTarget {
                job_name: job_name(&proposer.container_name),
                container_name: proposer.container_name.clone(),
                port: proposer.metrics_port,
                service_label: "op-proposer".to_string(),
                layer_label: "proposer".to_string(),
            });
        }

        if let Some(ref challenger) = self.l2_stack.op_challenger {
            targets.push(MetricsTarget {
                job_name: job_name(&challenger.container_name),
                container_name: challenger.container_name.clone(),
                port: challenger.metrics_port,
                service_label: "op-challenger".to_string(),
                layer_label: "challenger".to_string(),
            });
        }

        targets
    }

    /// Print detached mode information including container names and stop command.
    fn print_detached_info(
        outdata: &Path,
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
        if let Some(ref proposer) = l2_stack.op_proposer {
            container_names.push(proposer.container_name.clone());
        }
        if let Some(ref challenger) = l2_stack.op_challenger {
            container_names.push(challenger.container_name.clone());
        }

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
        tracing::info!(
            "Configuration saved to: {}",
            outdata.join(KUPCONF_FILENAME).display()
        );
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

    /// Restore L2 config files from an existing op-reth snapshot.
    ///
    /// Steps:
    /// 1. Validate snapshot dir has `rollup.json` and a reth-data subdirectory
    /// 2. Create l2-stack directory
    /// 3. Obtain intent.toml (from snapshot or via `op-deployer init --intent-type standard-overrides`)
    /// 4. Generate genesis.json via `op-deployer inspect genesis`
    /// 5. Copy rollup.json from snapshot
    /// 6. Symlink (or copy) the reth database for the primary sequencer
    #[allow(clippy::too_many_arguments)]
    async fn restore_from_snapshot(
        docker: &mut KupDocker,
        op_deployer: &OpDeployerConfig,
        l2_nodes_data_path: &PathBuf,
        snapshot_path: &PathBuf,
        l1_chain_id: u64,
        l2_chain_id: u64,
        sequencer_container_name: &str,
        copy_snapshot: bool,
    ) -> Result<()> {
        // Validate required files
        let rollup_src = snapshot_path.join("rollup.json");
        if !rollup_src.exists() {
            anyhow::bail!(
                "Snapshot directory is missing rollup.json: {}",
                snapshot_path.display()
            );
        }

        // Find the reth database subdirectory (expect exactly one)
        let subdirs: Vec<_> = std::fs::read_dir(snapshot_path)
            .context("Failed to read snapshot directory")?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .collect();

        let reth_db_dir = match subdirs.len() {
            0 => anyhow::bail!(
                "Snapshot directory has no subdirectory (expected reth database): {}",
                snapshot_path.display()
            ),
            1 => subdirs
                .into_iter()
                .next()
                .map(|e| e.path())
                .context("unreachable")?,
            n => anyhow::bail!(
                "Snapshot directory has {} subdirectories, expected exactly one reth database directory. Found: {}",
                n,
                subdirs
                    .iter()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };

        tracing::info!(
            snapshot = %snapshot_path.display(),
            reth_db = %reth_db_dir.display(),
            "Restoring from snapshot"
        );

        // Create l2-stack directory
        fs::FsHandler::create_host_config_directory(l2_nodes_data_path)?;

        // Handle intent.toml: copy from snapshot if present, otherwise generate
        let intent_src = snapshot_path.join("intent.toml");
        if intent_src.exists() {
            tracing::info!("Copying intent.toml from snapshot");
            std::fs::copy(&intent_src, l2_nodes_data_path.join("intent.toml"))
                .context("Failed to copy intent.toml from snapshot")?;
        } else {
            tracing::info!("Generating intent.toml via op-deployer init (standard-overrides)");
            op_deployer
                .generate_intent_file_with_type(
                    docker,
                    l2_nodes_data_path,
                    l1_chain_id,
                    l2_chain_id,
                    "standard-overrides",
                )
                .await
                .context("Failed to generate intent.toml for snapshot restore")?;
        }

        // Generate genesis.json from intent
        tracing::info!("Generating genesis.json via op-deployer inspect genesis");
        op_deployer
            .inspect_config(docker, l2_nodes_data_path, l2_chain_id, "genesis")
            .await
            .context("Failed to generate genesis.json for snapshot restore")?;

        // Copy rollup.json from snapshot
        tracing::info!("Copying rollup.json from snapshot");
        std::fs::copy(&rollup_src, l2_nodes_data_path.join("rollup.json"))
            .context("Failed to copy rollup.json from snapshot")?;

        // Link or copy the reth database for the primary sequencer
        let reth_data_dst =
            l2_nodes_data_path.join(format!("reth-data-{}", sequencer_container_name));

        if copy_snapshot {
            tracing::info!(
                src = %reth_db_dir.display(),
                dst = %reth_data_dst.display(),
                "Copying reth database from snapshot (this may take a while)"
            );
            fs::FsHandler::copy_dir_recursive(&reth_db_dir, &reth_data_dst)
                .await
                .context("Failed to copy reth database from snapshot")?;
        } else {
            let canonical_src = reth_db_dir
                .canonicalize()
                .context("Failed to canonicalize snapshot reth-data path")?;

            tracing::info!(
                src = %canonical_src.display(),
                dst = %reth_data_dst.display(),
                "Symlinking reth database from snapshot"
            );

            #[cfg(unix)]
            std::os::unix::fs::symlink(&canonical_src, &reth_data_dst)
                .context("Failed to create symlink for reth database")?;

            #[cfg(not(unix))]
            anyhow::bail!(
                "Snapshot symlinks are only supported on Unix systems. Use --copy-snapshot instead."
            );
        }

        tracing::info!("Snapshot restore complete");
        Ok(())
    }

    pub async fn deploy(
        mut self,
        docker: &mut KupDocker,
        force_deploy: bool,
        wait_for_exit: bool,
    ) -> Result<DeploymentResult> {
        let deploy_start = Instant::now();
        let mut metrics = DeploymentMetrics::default();
        tracing::info!("Starting deployment process...");

        // Genesis mode is incompatible with --override-state
        if self.override_state.is_some() && self.deployment_target == DeploymentTarget::Genesis {
            anyhow::bail!(
                "--override-state is incompatible with genesis deployment mode. \
                 Genesis mode boots Anvil from a generated L1 genesis, not an external state file."
            );
        }

        // Compute hash of current deployment configuration before any moves occur
        let current_config = DeploymentConfigHash::from_deployer(&self);
        let current_hash = current_config
            .compute_hash()
            .context("Failed to compute deployment config hash")?;

        // Save values we'll need after self is consumed
        let detach = self.detach;
        let outdata = self.outdata.clone();

        let l2_nodes_data_path = self.outdata.join("l2-stack");
        let anvil_data_path = self.outdata.join("anvil");

        // Deploy contracts and start Anvil. The order depends on the deployment target:
        // - Live: Start Anvil first, then deploy contracts to the live L1
        // - Genesis: Deploy contracts first (in-memory), extract L1 genesis, start Anvil with --init
        let anvil_docker_image = self.anvil.docker_image.clone();
        let op_deployer_image = self.op_deployer.docker_image.clone();
        let op_deployer_name = self.op_deployer.container_name.clone();
        let anvil_start = Instant::now();
        let (anvil, op_deployer_duration) = match self.deployment_target {
            DeploymentTarget::Genesis => {
                Self::deploy_with_genesis_target(
                    docker,
                    self.anvil,
                    &self.op_deployer,
                    &l2_nodes_data_path,
                    &anvil_data_path,
                    self.l1_chain_id,
                    self.l2_chain_id,
                    force_deploy,
                    &current_hash,
                )
                .await?
            }
            DeploymentTarget::Live => {
                Self::deploy_with_live_target(
                    docker,
                    self.anvil,
                    &self.op_deployer,
                    &self.l2_stack,
                    &l2_nodes_data_path,
                    &anvil_data_path,
                    self.l1_chain_id,
                    self.l2_chain_id,
                    force_deploy,
                    &current_hash,
                    self.snapshot.as_ref(),
                    self.copy_snapshot,
                    self.override_state.as_ref(),
                )
                .await?
            }
        };

        // Record Anvil metrics (subtract op-deployer time from Anvil total)
        let anvil_total = anvil_start.elapsed().saturating_sub(op_deployer_duration);
        let anvil_size = get_image_size(docker, &anvil.container_id).await;
        metrics.record(
            anvil.container_name.clone(),
            ServiceMetrics::from_timings(
                anvil_total,
                &anvil.deploy_timings,
                anvil_size,
                &anvil_docker_image,
            ),
        );

        // Record op-deployer metrics separately
        metrics.record(
            op_deployer_name,
            ServiceMetrics::composite(op_deployer_duration, None, op_deployer_image.to_string()),
        );

        // Write anvil.json so that faucet/spam commands can read account info.
        // We write this ourselves rather than using Anvil's --config-out flag
        // because --config-out is incompatible with --init (genesis mode).
        anvil
            .accounts
            .write_anvil_json(&self.outdata)
            .context("Failed to write anvil.json")?;

        // Register RPC-based state dump so Anvil L1 state is persisted before
        // containers are stopped. Both modes use this unified approach.
        if self.dump_state
            && let Some(ref host_url) = anvil.l1_host_url
        {
            docker.anvil_state_dump = Some(crate::AnvilStateDumpConfig {
                rpc_url: host_url.to_string(),
                output_path: self.outdata.join("anvil/state.json"),
            });
        }

        // In snapshot mode, proposer and challenger are unavailable (no state.json)
        if self.snapshot.is_some() {
            self.l2_stack.op_proposer = None;
            self.l2_stack.op_challenger = None;
        }

        let node_count = self.l2_stack.node_count();
        let mut services = vec!["op-batcher"];
        if self.l2_stack.op_proposer.is_some() {
            services.push("op-proposer");
        }
        if self.l2_stack.op_challenger.is_some() {
            services.push("op-challenger");
        }
        let services_label = services.join(" + ");
        tracing::info!(
            node_count,
            sequencer_count = self.l2_stack.sequencers.len(),
            validator_count = self.l2_stack.validators.len(),
            "Starting L2 stack ({} node(s) + {})...",
            node_count,
            services_label,
        );

        let l2_stack = self
            .l2_stack
            .start(
                docker,
                l2_nodes_data_path.clone(),
                &anvil,
                self.l1_chain_id,
                &mut metrics,
            )
            .await
            .context("Failed to start L2 stack")?;

        // Persist P2P keys from handlers back to builders so they survive restarts
        // and can be used to compute enodes for adding nodes to a running network.
        self.l2_stack.persist_p2p_keys(&l2_stack);

        // Re-save the config with P2P keys by loading and patching.
        // We cannot call self.save_config() because self.anvil has been moved.
        {
            let config_path = outdata.join(KUPCONF_FILENAME);
            if config_path.exists()
                && let Ok(mut saved) = Deployer::load_from_file(&config_path)
            {
                saved.l2_stack = self.l2_stack.clone();
                if let Err(e) = saved.save_to_file(&config_path) {
                    tracing::warn!(error = %e, "Failed to re-save config with P2P keys");
                }
            }
        }

        // Start monitoring stack if enabled
        let monitoring = if self.monitoring.enabled {
            tracing::info!("Starting monitoring stack (Prometheus + Grafana)...");

            let monitoring_data_path = self.outdata.join("monitoring");
            let metrics_targets = Self::build_metrics_targets(&l2_stack);

            let mon_start = Instant::now();
            let mon_handler = self
                .monitoring
                .start(
                    docker,
                    monitoring_data_path,
                    metrics_targets,
                    self.dashboards_path,
                )
                .await
                .context("Failed to start monitoring stack")?;
            let mon_total = mon_start.elapsed();

            let prom_size = get_image_size(docker, &mon_handler.prometheus.container_id).await;
            metrics.record(
                mon_handler.prometheus.container_name.clone(),
                ServiceMetrics::composite(
                    mon_total,
                    prom_size,
                    self.monitoring.prometheus.docker_image.to_string(),
                ),
            );
            let grafana_size = get_image_size(docker, &mon_handler.grafana.container_id).await;
            metrics.record(
                mon_handler.grafana.container_name.clone(),
                ServiceMetrics::composite(
                    mon_total,
                    grafana_size,
                    self.monitoring.grafana.docker_image.to_string(),
                ),
            );

            Some(mon_handler)
        } else {
            None
        };

        // Finalize and log deployment metrics
        metrics.total = deploy_start.elapsed();
        metrics.log_summary();

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
            if let Some(ref conductor) = node.op_conductor
                && let Some(ref url) = conductor.rpc_host_url
            {
                tracing::info!("L2 {} (op-conductor) RPC:     {}", label, url);
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
        if let Some(ref proposer) = l2_stack.op_proposer {
            tracing::info!("Op Proposer RPC:      {}", proposer.rpc_url);
        }
        if let Some(ref challenger) = l2_stack.op_challenger {
            tracing::info!("Op Challenger metrics: {}", challenger.metrics_url);
        }

        tracing::info!("");

        // Register devnet in the global registry
        let network_name = docker
            .config
            .net_name
            .strip_suffix("-network")
            .unwrap_or(&docker.config.net_name);
        if let Err(e) =
            crate::DevnetRegistry::new().and_then(|r| r.register(network_name, &outdata))
        {
            tracing::warn!(error = %e, "Failed to register devnet in registry");
        }
        docker.registry_name = Some(network_name.to_string());

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

        Ok(DeploymentResult {
            anvil,
            l2_stack,
            metrics,
            monitoring,
        })
    }
}
