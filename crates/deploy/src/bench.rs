//! Benchmarking utility for deployment metrics.
//!
//! Runs repeated deployments and aggregates per-service statistics
//! (min/max/mean/median/p95/stddev) into a TOML report.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::metrics::DeploymentMetrics;
use crate::{DeployerBuilder, DeploymentTarget, KupDocker, KupDockerConfig, cleanup_by_prefix};

/// Configuration for a benchmark run.
pub struct BenchConfig {
    /// Number of measured iterations.
    pub iterations: usize,
    /// Warmup iterations (results discarded).
    pub warmup: usize,
    /// Human-readable label for this run.
    pub label: Option<String>,
    /// Deployment target (live or genesis).
    pub deployment_target: DeploymentTarget,
    /// Total L2 node count.
    pub l2_node_count: usize,
    /// Sequencer count.
    pub sequencer_count: usize,
    /// Block time in seconds.
    pub block_time: u64,
    /// Enable flashblocks.
    pub flashblocks: bool,
    /// Skip op-proposer.
    pub no_proposer: bool,
    /// Skip op-challenger.
    pub no_challenger: bool,
    /// Optional closure to customize the deployer builder (e.g. Docker image overrides).
    pub deployer_customizer: Option<Box<dyn Fn(DeployerBuilder) -> DeployerBuilder + Send>>,
}

/// Aggregate statistics for a series of duration measurements (in milliseconds).
#[derive(Serialize)]
pub struct Stats {
    pub min_ms: u64,
    pub max_ms: u64,
    pub mean_ms: f64,
    pub median_ms: u64,
    pub p95_ms: u64,
    pub stddev_ms: f64,
}

/// Per-service aggregate stats across iterations.
#[derive(Serialize)]
pub struct ServiceBenchResult {
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size_bytes: Option<u64>,
    pub total: Stats,
    pub pull: Stats,
    pub setup: Stats,
    pub work: Stats,
}

/// Metadata about the benchmark run.
#[derive(Serialize)]
pub struct BenchMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub iterations: usize,
    pub warmup: usize,
    pub unix_timestamp: u64,
    pub deployment_target: String,
    pub l2_nodes: usize,
    pub sequencer_count: usize,
}

/// Full benchmark result.
#[derive(Serialize)]
pub struct BenchResult {
    pub meta: BenchMeta,
    pub total_deploy: Stats,
    pub services: BTreeMap<String, ServiceBenchResult>,
    pub iteration_totals_ms: Vec<u64>,
}

/// Run a benchmark: deploy N times, collect metrics, aggregate.
pub async fn run_bench(config: BenchConfig) -> Result<BenchResult> {
    let total_iterations = config.warmup + config.iterations;
    let mut warmup_metrics: Vec<DeploymentMetrics> = Vec::with_capacity(config.warmup);
    let mut measured_metrics: Vec<DeploymentMetrics> = Vec::with_capacity(config.iterations);
    let mut failed_count = 0usize;
    let tmp_dir = std::env::temp_dir();

    for i in 0..total_iterations {
        let is_warmup = i < config.warmup;
        let label = if is_warmup {
            format!("warmup {}/{}", i + 1, config.warmup)
        } else {
            format!("iteration {}/{}", i - config.warmup + 1, config.iterations)
        };

        tracing::info!(%label, "Starting bench iteration");

        let prefix = format!("kup-bench-{i}");
        let outdata = tmp_dir.join(format!("kup-bench-{i}"));

        match run_single_iteration(&config, &prefix, &outdata).await {
            Ok(metrics) => {
                tracing::info!(
                    %label,
                    total_ms = metrics.total.as_millis(),
                    "Iteration completed"
                );
                // Normalize service keys by stripping the iteration-specific prefix
                // (e.g. "kup-bench-0-anvil" → "anvil")
                let normalized = normalize_metrics(metrics, &prefix);
                if is_warmup {
                    warmup_metrics.push(normalized);
                } else {
                    measured_metrics.push(normalized);
                }
            }
            Err(e) => {
                failed_count += 1;
                tracing::warn!(%label, error = %e, "Iteration failed, skipping");
            }
        }

        // Belt-and-suspenders cleanup
        if let Err(e) = cleanup_by_prefix(&prefix).await {
            tracing::debug!(error = %e, "Cleanup after iteration failed (may be expected)");
        }
        if outdata.exists() {
            let _ = std::fs::remove_dir_all(&outdata);
        }
    }

    drop(warmup_metrics); // Explicitly discard warmup results

    // Check failure rate
    let max_failures = total_iterations.div_ceil(2); // >50%
    if failed_count > max_failures {
        anyhow::bail!(
            "Too many iterations failed: {failed_count}/{total_iterations} (>{max_failures} allowed)"
        );
    }

    if measured_metrics.is_empty() {
        anyhow::bail!("No successful measured iterations to aggregate");
    }

    let result = aggregate_metrics(&config, &measured_metrics);
    Ok(result)
}

/// Run a single deployment iteration and return its metrics.
async fn run_single_iteration(
    config: &BenchConfig,
    prefix: &str,
    outdata: &Path,
) -> Result<DeploymentMetrics> {
    let l1_chain_id = rand::Rng::random_range(&mut rand::rng(), 10000..=99999u64);

    let mut builder = DeployerBuilder::new(l1_chain_id)
        .network_name(prefix)
        .outdata_path(outdata)
        .no_cleanup(false)
        .dump_state(false)
        .monitoring_enabled(false)
        .detach(false)
        .deployment_target(config.deployment_target)
        .l2_node_count(config.l2_node_count)
        .sequencer_count(config.sequencer_count)
        .block_time(config.block_time)
        .flashblocks(config.flashblocks)
        .no_proposer(config.no_proposer)
        .no_challenger(config.no_challenger);

    if let Some(ref customizer) = config.deployer_customizer {
        builder = customizer(builder);
    }

    let deployer = builder.build().await.context("Failed to build deployer")?;

    let docker_config = KupDockerConfig {
        net_name: format!("{prefix}-network"),
        no_cleanup: false,
        publish_all_ports: false,
        log_max_size: None,
        log_max_file: None,
        stream_logs: false,
    };

    let mut docker = KupDocker::new(docker_config)
        .await
        .context("Failed to create Docker client")?;

    let result = deployer
        .deploy(&mut docker, true, false)
        .await
        .context("Deployment failed")?;

    Ok(result.metrics)
}

/// Normalize service keys by stripping the iteration-specific prefix.
///
/// E.g. "kup-bench-0-anvil" with prefix "kup-bench-0" → "anvil"
fn normalize_metrics(mut metrics: DeploymentMetrics, prefix: &str) -> DeploymentMetrics {
    let strip_prefix = format!("{prefix}-");
    let normalized_services = metrics
        .services
        .into_iter()
        .map(|(name, m)| {
            let normalized = name
                .strip_prefix(&strip_prefix)
                .unwrap_or(&name)
                .to_string();
            (normalized, m)
        })
        .collect();
    metrics.services = normalized_services;
    metrics
}

/// Aggregate metrics from multiple iterations into a `BenchResult`.
fn aggregate_metrics(config: &BenchConfig, metrics: &[DeploymentMetrics]) -> BenchResult {
    let iteration_totals_ms: Vec<u64> =
        metrics.iter().map(|m| m.total.as_millis() as u64).collect();

    let total_deploy = compute_stats(&iteration_totals_ms);

    // Collect all service names across iterations
    let all_service_names: BTreeSet<String> = metrics
        .iter()
        .flat_map(|m| m.services.keys().cloned())
        .collect();

    let mut services = BTreeMap::new();
    for service_name in &all_service_names {
        let service_data: Vec<_> = metrics
            .iter()
            .filter_map(|m| m.services.get(service_name))
            .collect();

        if service_data.is_empty() {
            continue;
        }

        let totals: Vec<u64> = service_data
            .iter()
            .map(|s| s.total.as_millis() as u64)
            .collect();
        let pulls: Vec<u64> = service_data
            .iter()
            .map(|s| s.pull.as_millis() as u64)
            .collect();
        let setups: Vec<u64> = service_data
            .iter()
            .map(|s| s.setup.as_millis() as u64)
            .collect();
        let works: Vec<u64> = service_data
            .iter()
            .map(|s| s.work.as_millis() as u64)
            .collect();
        let image = service_data[0].image_ref.clone();
        let image_size = service_data.iter().find_map(|s| s.image_size_bytes);

        services.insert(
            service_name.clone(),
            ServiceBenchResult {
                image,
                image_size_bytes: image_size,
                total: compute_stats(&totals),
                pull: compute_stats(&pulls),
                setup: compute_stats(&setups),
                work: compute_stats(&works),
            },
        );
    }

    let unix_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    BenchResult {
        meta: BenchMeta {
            label: config.label.clone(),
            iterations: metrics.len(),
            warmup: config.warmup,
            unix_timestamp,
            deployment_target: config.deployment_target.to_string(),
            l2_nodes: config.l2_node_count,
            sequencer_count: config.sequencer_count,
        },
        total_deploy,
        services,
        iteration_totals_ms,
    }
}

/// Compute aggregate statistics from a slice of u64 values.
fn compute_stats(values: &[u64]) -> Stats {
    if values.is_empty() {
        return Stats {
            min_ms: 0,
            max_ms: 0,
            mean_ms: 0.0,
            median_ms: 0,
            p95_ms: 0,
            stddev_ms: 0.0,
        };
    }

    let mut sorted = values.to_vec();
    sorted.sort_unstable();

    let n = sorted.len();
    let min_ms = sorted[0];
    let max_ms = sorted[n - 1];
    let sum: u64 = sorted.iter().sum();
    let mean_ms = sum as f64 / n as f64;

    let median_ms = if n.is_multiple_of(2) {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2
    } else {
        sorted[n / 2]
    };

    // p95: index = ceil(0.95 * n) - 1, clamped
    let p95_idx = ((0.95 * n as f64).ceil() as usize)
        .saturating_sub(1)
        .min(n - 1);
    let p95_ms = sorted[p95_idx];

    let variance = sorted
        .iter()
        .map(|&v| {
            let diff = v as f64 - mean_ms;
            diff * diff
        })
        .sum::<f64>()
        / n as f64;
    let stddev_ms = variance.sqrt();

    // Round mean and stddev to 1 decimal
    let mean_ms = (mean_ms * 10.0).round() / 10.0;
    let stddev_ms = (stddev_ms * 10.0).round() / 10.0;

    Stats {
        min_ms,
        max_ms,
        mean_ms,
        median_ms,
        p95_ms,
        stddev_ms,
    }
}

/// Serialize a `BenchResult` to TOML.
pub fn to_toml(result: &BenchResult) -> Result<String> {
    toml::to_string_pretty(result).context("Failed to serialize bench result to TOML")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_stats_basic() {
        let values = vec![10, 20, 30, 40, 50];
        let stats = compute_stats(&values);
        assert_eq!(stats.min_ms, 10);
        assert_eq!(stats.max_ms, 50);
        assert_eq!(stats.mean_ms, 30.0);
        assert_eq!(stats.median_ms, 30);
        assert_eq!(stats.p95_ms, 50);
    }

    #[test]
    fn test_compute_stats_single() {
        let values = vec![42];
        let stats = compute_stats(&values);
        assert_eq!(stats.min_ms, 42);
        assert_eq!(stats.max_ms, 42);
        assert_eq!(stats.mean_ms, 42.0);
        assert_eq!(stats.median_ms, 42);
        assert_eq!(stats.p95_ms, 42);
        assert_eq!(stats.stddev_ms, 0.0);
    }

    #[test]
    fn test_compute_stats_empty() {
        let values: Vec<u64> = vec![];
        let stats = compute_stats(&values);
        assert_eq!(stats.min_ms, 0);
        assert_eq!(stats.max_ms, 0);
        assert_eq!(stats.mean_ms, 0.0);
    }

    #[test]
    fn test_compute_stats_even_count() {
        let values = vec![10, 20, 30, 40];
        let stats = compute_stats(&values);
        assert_eq!(stats.median_ms, 25); // (20+30)/2
    }
}
