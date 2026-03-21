//! Layered configuration resolution using figment.
//!
//! Merges configuration from multiple sources (lowest to highest priority):
//! 1. Hardcoded defaults
//! 2. Environment variables (`KUP_*` prefix)
//! 3. CLI arguments (only explicitly provided values)
//!
//! The resolved [`DeployConfig`] is then converted to a [`DeployerBuilder`]
//! for deployment.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::parser::ValueSource;
use figment::{
    Figment,
    providers::{Env, Serialized},
};
use serde::{Deserialize, Serialize};

use kupcake_deploy::{DeployerBuilder, DeploymentTarget, OutDataPath};

/// Flat deployment configuration struct.
///
/// All fields are `Option<T>` to support sparse layered merging.
/// `None` means "use the default value" — no explicit override was provided.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeployConfig {
    // ── Network Configuration ──
    pub network: Option<String>,
    pub l1: Option<String>,
    pub l2_chain: Option<u64>,
    pub block_time: Option<u64>,
    pub genesis_timestamp: Option<u64>,

    // ── L2 Nodes ──
    pub l2_nodes: Option<usize>,
    #[serde(alias = "sequencers")]
    pub sequencer_count: Option<usize>,
    pub flashblocks: Option<bool>,
    pub proofs_validators: Option<usize>,

    // ── Deployment ──
    pub deployment_target: Option<String>,
    pub no_proposer: Option<bool>,
    pub no_challenger: Option<bool>,

    // ── State & Storage ──
    pub outdata: Option<String>,
    pub dump_state: Option<bool>,
    pub override_state: Option<String>,
    pub snapshot: Option<String>,
    pub copy_snapshot: Option<bool>,

    // ── Runtime Behavior ──
    pub no_cleanup: Option<bool>,
    pub detach: Option<bool>,
    pub publish_all_ports: Option<bool>,

    // ── Logging & Monitoring ──
    pub log_max_size: Option<String>,
    pub log_max_file: Option<String>,
    pub quiet_services: Option<bool>,
    pub stream_logs: Option<bool>,
    pub long_running: Option<bool>,

    // ── Docker Images ──
    pub anvil_image: Option<String>,
    pub anvil_tag: Option<String>,
    pub op_reth_image: Option<String>,
    pub op_reth_tag: Option<String>,
    pub kona_node_image: Option<String>,
    pub kona_node_tag: Option<String>,
    pub op_batcher_image: Option<String>,
    pub op_batcher_tag: Option<String>,
    pub op_proposer_image: Option<String>,
    pub op_proposer_tag: Option<String>,
    pub op_challenger_image: Option<String>,
    pub op_challenger_tag: Option<String>,
    pub op_conductor_image: Option<String>,
    pub op_conductor_tag: Option<String>,
    pub op_deployer_image: Option<String>,
    pub op_deployer_tag: Option<String>,
    pub prometheus_image: Option<String>,
    pub prometheus_tag: Option<String>,
    pub grafana_image: Option<String>,
    pub grafana_tag: Option<String>,
    pub op_rbuilder_image: Option<String>,
    pub op_rbuilder_tag: Option<String>,

    // ── Binary Overrides ──
    pub op_reth_binary: Option<String>,
    pub kona_node_binary: Option<String>,
    pub op_batcher_binary: Option<String>,
    pub op_proposer_binary: Option<String>,
    pub op_challenger_binary: Option<String>,
    pub op_conductor_binary: Option<String>,
    pub op_rbuilder_binary: Option<String>,
}

/// Resolve a [`DeployConfig`] by merging layers: defaults → env vars → CLI args.
///
/// The `matches` parameter is used to determine which CLI args were explicitly
/// provided by the user (vs. using default values).
pub fn resolve_deploy_config(
    args: &crate::cli::DeployArgs,
    matches: &clap::ArgMatches,
) -> Result<DeployConfig> {
    // Build CLI-only overrides (explicit args only, skipping defaults)
    let cli_overrides = build_cli_overrides(args, matches);

    let figment = Figment::new()
        // Layer 1: env vars (KUP_* prefix, stripped and lowercased)
        .merge(Env::prefixed("KUP_"))
        // Layer 2: CLI args (highest priority — only explicitly provided values)
        .merge(Serialized::defaults(cli_overrides));

    figment
        .extract()
        .context("Failed to resolve deployment configuration")
}

impl DeployConfig {
    /// Resolve `--long-running` shorthand into individual logging fields.
    pub fn resolve_long_running(&mut self) {
        if !self.long_running.unwrap_or(false) {
            return;
        }
        if self.log_max_size.is_none() {
            self.log_max_size = Some("10m".to_string());
        }
        if self.log_max_file.is_none() {
            self.log_max_file = Some("3".to_string());
        }
        if self.quiet_services.is_none() {
            self.quiet_services = Some(true);
        }
    }
}

/// Convert a resolved [`DeployConfig`] into a [`DeployerBuilder`].
///
/// This replaces the manual 130-line mapping block that was previously in `run_deploy()`.
/// The `l1_chain_id` and `l1_rpc_url` are resolved externally (requires async RPC call).
pub fn deploy_config_to_builder(
    config: &DeployConfig,
    l1_chain_id: u64,
    l1_rpc_url: Option<String>,
) -> DeployerBuilder {
    // Force no_cleanup when detach is set
    let no_cleanup = config.no_cleanup.unwrap_or(false) || config.detach.unwrap_or(false);

    let mut builder = DeployerBuilder::new(l1_chain_id)
        .maybe_l2_chain_id(config.l2_chain)
        .maybe_network_name(config.network.clone())
        .maybe_outdata(config.outdata.as_ref().map(|o| {
            if o == "tempdir" {
                OutDataPath::TempDir
            } else {
                OutDataPath::Path(PathBuf::from(o))
            }
        }))
        .maybe_l1_rpc_url(l1_rpc_url)
        .no_cleanup(no_cleanup)
        .dump_state(config.dump_state.unwrap_or(true))
        .maybe_override_state(config.override_state.as_ref().map(PathBuf::from))
        .detach(config.detach.unwrap_or(false))
        .publish_all_ports(config.publish_all_ports.unwrap_or(false))
        .block_time(config.block_time.unwrap_or(4))
        .maybe_genesis_timestamp(config.genesis_timestamp)
        .l2_node_count(config.l2_nodes.unwrap_or(5))
        .sequencer_count(config.sequencer_count.unwrap_or(2))
        .maybe_log_max_size(config.log_max_size.clone())
        .maybe_log_max_file(config.log_max_file.clone())
        .quiet_services(config.quiet_services.unwrap_or(false))
        .stream_logs(config.stream_logs.unwrap_or(false))
        .no_proposer(config.no_proposer.unwrap_or(false))
        .no_challenger(config.no_challenger.unwrap_or(false))
        .flashblocks(config.flashblocks.unwrap_or(false))
        .proofs_validators(config.proofs_validators.unwrap_or(0))
        .maybe_snapshot(config.snapshot.as_ref().map(PathBuf::from))
        .copy_snapshot(config.copy_snapshot.unwrap_or(false))
        .deployment_target(parse_deployment_target(
            config.deployment_target.as_deref().unwrap_or("live"),
        ));

    // Docker images — use defaults from the builder if not overridden
    if let Some(ref v) = config.anvil_image {
        builder = builder.anvil_image(v.clone());
    }
    if let Some(ref v) = config.anvil_tag {
        builder = builder.anvil_tag(v.clone());
    }
    if let Some(ref v) = config.op_reth_image {
        builder = builder.op_reth_image(v.clone());
    }
    if let Some(ref v) = config.op_reth_tag {
        builder = builder.op_reth_tag(v.clone());
    }
    if let Some(ref v) = config.kona_node_image {
        builder = builder.kona_node_image(v.clone());
    }
    if let Some(ref v) = config.kona_node_tag {
        builder = builder.kona_node_tag(v.clone());
    }
    if let Some(ref v) = config.op_batcher_image {
        builder = builder.op_batcher_image(v.clone());
    }
    if let Some(ref v) = config.op_batcher_tag {
        builder = builder.op_batcher_tag(v.clone());
    }
    if let Some(ref v) = config.op_proposer_image {
        builder = builder.op_proposer_image(v.clone());
    }
    if let Some(ref v) = config.op_proposer_tag {
        builder = builder.op_proposer_tag(v.clone());
    }
    if let Some(ref v) = config.op_challenger_image {
        builder = builder.op_challenger_image(v.clone());
    }
    if let Some(ref v) = config.op_challenger_tag {
        builder = builder.op_challenger_tag(v.clone());
    }
    if let Some(ref v) = config.op_conductor_image {
        builder = builder.op_conductor_image(v.clone());
    }
    if let Some(ref v) = config.op_conductor_tag {
        builder = builder.op_conductor_tag(v.clone());
    }
    if let Some(ref v) = config.op_deployer_image {
        builder = builder.op_deployer_image(v.clone());
    }
    if let Some(ref v) = config.op_deployer_tag {
        builder = builder.op_deployer_tag(v.clone());
    }
    if let Some(ref v) = config.prometheus_image {
        builder = builder.prometheus_image(v.clone());
    }
    if let Some(ref v) = config.prometheus_tag {
        builder = builder.prometheus_tag(v.clone());
    }
    if let Some(ref v) = config.grafana_image {
        builder = builder.grafana_image(v.clone());
    }
    if let Some(ref v) = config.grafana_tag {
        builder = builder.grafana_tag(v.clone());
    }
    if let Some(ref v) = config.op_rbuilder_image {
        builder = builder.op_rbuilder_image(v.clone());
    }
    if let Some(ref v) = config.op_rbuilder_tag {
        builder = builder.op_rbuilder_tag(v.clone());
    }

    // Binary overrides
    if let Some(ref path) = config.op_reth_binary {
        builder = builder.with_op_reth_binary(path);
    }
    if let Some(ref path) = config.kona_node_binary {
        builder = builder.with_kona_node_binary(path);
    }
    if let Some(ref path) = config.op_batcher_binary {
        builder = builder.with_op_batcher_binary(path);
    }
    if let Some(ref path) = config.op_proposer_binary {
        builder = builder.with_op_proposer_binary(path);
    }
    if let Some(ref path) = config.op_challenger_binary {
        builder = builder.with_op_challenger_binary(path);
    }
    if let Some(ref path) = config.op_conductor_binary {
        builder = builder.with_op_conductor_binary(path);
    }
    if let Some(ref path) = config.op_rbuilder_binary {
        builder = builder.with_op_rbuilder_binary(path);
    }

    builder
}

fn parse_deployment_target(s: &str) -> DeploymentTarget {
    if s.eq_ignore_ascii_case("genesis") {
        DeploymentTarget::Genesis
    } else {
        DeploymentTarget::Live
    }
}

/// Build a sparse [`DeployConfig`] containing only the CLI args that were explicitly
/// provided by the user (not default values).
///
/// Uses clap's `value_source()` to detect which args the user explicitly set
/// on the command line (as opposed to defaults or env vars handled by figment).
fn build_cli_overrides(args: &crate::cli::DeployArgs, matches: &clap::ArgMatches) -> DeployConfig {
    // Only include values the user explicitly passed on the command line.
    // Env vars are handled by figment's Env provider, so we skip EnvVariable source.
    let is_explicit = |name: &str| -> bool {
        matches
            .value_source(name)
            .is_some_and(|s| s == ValueSource::CommandLine)
    };

    let mut config = DeployConfig::default();

    // Network Configuration
    if is_explicit("network") {
        config.network = args.network.clone();
    }
    if is_explicit("l1") {
        config.l1 = args.l1.as_ref().map(|s| s.rpc_url());
    }
    if is_explicit("l2_chain") {
        config.l2_chain = args.l2_chain.map(|c| c.chain_id());
    }
    if is_explicit("block_time") {
        config.block_time = Some(args.block_time);
    }
    if is_explicit("genesis_timestamp") {
        config.genesis_timestamp = args.genesis_timestamp;
    }

    // L2 Nodes
    if is_explicit("l2_nodes") {
        config.l2_nodes = Some(args.l2_nodes);
    }
    if is_explicit("sequencer_count") {
        config.sequencer_count = Some(args.sequencer_count);
    }
    if is_explicit("flashblocks") {
        config.flashblocks = Some(args.flashblocks);
    }
    if is_explicit("proofs_validators") {
        config.proofs_validators = Some(args.proofs_validators);
    }

    // Deployment
    if is_explicit("deployment_target") {
        config.deployment_target = match args.deployment_target {
            crate::cli::DeploymentTargetArg::Live => Some("live".to_string()),
            crate::cli::DeploymentTargetArg::Genesis => Some("genesis".to_string()),
        };
    }
    if is_explicit("no_proposer") {
        config.no_proposer = Some(args.no_proposer);
    }
    if is_explicit("no_challenger") {
        config.no_challenger = Some(args.no_challenger);
    }

    // State & Storage
    if is_explicit("outdata") {
        config.outdata = args.outdata.as_ref().map(|o| match o {
            crate::cli::OutData::TempDir => "tempdir".to_string(),
            crate::cli::OutData::Path(s) => s.clone(),
        });
    }
    if is_explicit("dump_state") {
        config.dump_state = Some(args.dump_state);
    }
    if is_explicit("override_state") {
        config.override_state = args.override_state.clone();
    }
    if is_explicit("snapshot") {
        config.snapshot = args.snapshot.clone();
    }
    if is_explicit("copy_snapshot") {
        config.copy_snapshot = Some(args.copy_snapshot);
    }

    // Runtime Behavior
    if is_explicit("no_cleanup") {
        config.no_cleanup = Some(args.no_cleanup);
    }
    if is_explicit("detach") {
        config.detach = Some(args.detach);
    }
    if is_explicit("publish_all_ports") {
        config.publish_all_ports = Some(args.publish_all_ports);
    }

    // Logging & Monitoring
    if is_explicit("log_max_size") {
        config.log_max_size = args.log_max_size.clone();
    }
    if is_explicit("log_max_file") {
        config.log_max_file = args.log_max_file.clone();
    }
    if is_explicit("quiet_services") {
        config.quiet_services = Some(args.quiet_services);
    }
    if is_explicit("stream_logs") {
        config.stream_logs = Some(args.stream_logs);
    }
    if is_explicit("long_running") {
        config.long_running = Some(args.long_running);
    }

    // Docker Images
    if is_explicit("anvil_image") {
        config.anvil_image = Some(args.docker_images.anvil_image.clone());
    }
    if is_explicit("anvil_tag") {
        config.anvil_tag = Some(args.docker_images.anvil_tag.clone());
    }
    if is_explicit("op_reth_image") {
        config.op_reth_image = Some(args.docker_images.op_reth_image.clone());
    }
    if is_explicit("op_reth_tag") {
        config.op_reth_tag = Some(args.docker_images.op_reth_tag.clone());
    }
    if is_explicit("kona_node_image") {
        config.kona_node_image = Some(args.docker_images.kona_node_image.clone());
    }
    if is_explicit("kona_node_tag") {
        config.kona_node_tag = Some(args.docker_images.kona_node_tag.clone());
    }
    if is_explicit("op_batcher_image") {
        config.op_batcher_image = Some(args.docker_images.op_batcher_image.clone());
    }
    if is_explicit("op_batcher_tag") {
        config.op_batcher_tag = Some(args.docker_images.op_batcher_tag.clone());
    }
    if is_explicit("op_proposer_image") {
        config.op_proposer_image = Some(args.docker_images.op_proposer_image.clone());
    }
    if is_explicit("op_proposer_tag") {
        config.op_proposer_tag = Some(args.docker_images.op_proposer_tag.clone());
    }
    if is_explicit("op_challenger_image") {
        config.op_challenger_image = Some(args.docker_images.op_challenger_image.clone());
    }
    if is_explicit("op_challenger_tag") {
        config.op_challenger_tag = Some(args.docker_images.op_challenger_tag.clone());
    }
    if is_explicit("op_conductor_image") {
        config.op_conductor_image = Some(args.docker_images.op_conductor_image.clone());
    }
    if is_explicit("op_conductor_tag") {
        config.op_conductor_tag = Some(args.docker_images.op_conductor_tag.clone());
    }
    if is_explicit("op_deployer_image") {
        config.op_deployer_image = Some(args.docker_images.op_deployer_image.clone());
    }
    if is_explicit("op_deployer_tag") {
        config.op_deployer_tag = Some(args.docker_images.op_deployer_tag.clone());
    }
    if is_explicit("prometheus_image") {
        config.prometheus_image = Some(args.docker_images.prometheus_image.clone());
    }
    if is_explicit("prometheus_tag") {
        config.prometheus_tag = Some(args.docker_images.prometheus_tag.clone());
    }
    if is_explicit("grafana_image") {
        config.grafana_image = Some(args.docker_images.grafana_image.clone());
    }
    if is_explicit("grafana_tag") {
        config.grafana_tag = Some(args.docker_images.grafana_tag.clone());
    }
    if is_explicit("op_rbuilder_image") {
        config.op_rbuilder_image = Some(args.docker_images.op_rbuilder_image.clone());
    }
    if is_explicit("op_rbuilder_tag") {
        config.op_rbuilder_tag = Some(args.docker_images.op_rbuilder_tag.clone());
    }

    // Binary Overrides
    if is_explicit("op_reth_binary") {
        config.op_reth_binary = args.docker_images.op_reth_binary.clone();
    }
    if is_explicit("kona_node_binary") {
        config.kona_node_binary = args.docker_images.kona_node_binary.clone();
    }
    if is_explicit("op_batcher_binary") {
        config.op_batcher_binary = args.docker_images.op_batcher_binary.clone();
    }
    if is_explicit("op_proposer_binary") {
        config.op_proposer_binary = args.docker_images.op_proposer_binary.clone();
    }
    if is_explicit("op_challenger_binary") {
        config.op_challenger_binary = args.docker_images.op_challenger_binary.clone();
    }
    if is_explicit("op_conductor_binary") {
        config.op_conductor_binary = args.docker_images.op_conductor_binary.clone();
    }
    if is_explicit("op_rbuilder_binary") {
        config.op_rbuilder_binary = args.docker_images.op_rbuilder_binary.clone();
    }

    config
}

/// Apply CLI overrides to an existing [`kupcake_deploy::Deployer`] loaded from config.
///
/// Only fields that were explicitly provided on the command line are applied.
/// This enables `--config Kupcake.toml --block-time 2` to work correctly.
pub fn apply_cli_overrides(deployer: &mut kupcake_deploy::Deployer, config: &DeployConfig) {
    // Runtime behavior overrides (safe to apply to a loaded Deployer)
    if let Some(v) = config.no_cleanup {
        deployer.docker.no_cleanup = v;
    }
    if let Some(v) = config.detach {
        deployer.detach = v;
    }
    if let Some(v) = config.dump_state {
        deployer.dump_state = v;
    }
    if let Some(v) = config.publish_all_ports {
        deployer.docker.publish_all_ports = v;
    }
    if let Some(v) = config.stream_logs {
        deployer.docker.stream_logs = v;
    }

    // Logging overrides
    if config.log_max_size.is_some() {
        deployer.docker.log_max_size = config.log_max_size.clone();
    }
    if config.log_max_file.is_some() {
        deployer.docker.log_max_file = config.log_max_file.clone();
    }

    // Block time affects anvil config
    if let Some(block_time) = config.block_time {
        deployer.anvil.block_time = block_time;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deploy_config_default_is_all_none() {
        let config = DeployConfig::default();
        assert!(config.network.is_none());
        assert!(config.block_time.is_none());
        assert!(config.l2_nodes.is_none());
        assert!(config.anvil_image.is_none());
    }

    #[test]
    fn test_deploy_config_to_builder_defaults() {
        let config = DeployConfig::default();
        let builder = deploy_config_to_builder(&config, 11155111, None);
        // Builder should be created with defaults — just verify it doesn't panic
        let _ = builder;
    }

    #[test]
    fn test_deploy_config_to_builder_with_overrides() {
        let config = DeployConfig {
            block_time: Some(2),
            l2_nodes: Some(3),
            sequencer_count: Some(1),
            flashblocks: Some(true),
            ..Default::default()
        };
        let builder = deploy_config_to_builder(&config, 11155111, None);
        let _ = builder;
    }

    #[test]
    fn test_resolve_long_running() {
        let mut config = DeployConfig {
            long_running: Some(true),
            ..Default::default()
        };
        config.resolve_long_running();
        assert_eq!(config.log_max_size.as_deref(), Some("10m"));
        assert_eq!(config.log_max_file.as_deref(), Some("3"));
        assert_eq!(config.quiet_services, Some(true));
    }

    #[test]
    fn test_resolve_long_running_explicit_overrides() {
        let mut config = DeployConfig {
            long_running: Some(true),
            log_max_size: Some("50m".to_string()),
            quiet_services: Some(false),
            ..Default::default()
        };
        config.resolve_long_running();
        // Explicit values should not be overwritten
        assert_eq!(config.log_max_size.as_deref(), Some("50m"));
        assert_eq!(config.log_max_file.as_deref(), Some("3")); // default filled in
        assert_eq!(config.quiet_services, Some(false)); // explicit false preserved
    }

    #[test]
    fn test_parse_deployment_target() {
        assert!(matches!(
            parse_deployment_target("live"),
            DeploymentTarget::Live
        ));
        assert!(matches!(
            parse_deployment_target("genesis"),
            DeploymentTarget::Genesis
        ));
        assert!(matches!(
            parse_deployment_target("GENESIS"),
            DeploymentTarget::Genesis
        ));
        assert!(matches!(
            parse_deployment_target("unknown"),
            DeploymentTarget::Live
        ));
    }

    #[test]
    fn test_figment_env_override() {
        // Simulate figment merging with an env var
        let figment = Figment::new()
            .merge(Serialized::defaults(DeployConfig::default()))
            .merge(Serialized::defaults(DeployConfig {
                block_time: Some(8),
                ..Default::default()
            }));

        let config: DeployConfig = figment.extract().unwrap();
        assert_eq!(config.block_time, Some(8));
        // Other fields should remain None
        assert!(config.network.is_none());
    }
}
