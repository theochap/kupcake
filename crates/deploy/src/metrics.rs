//! Deployment metrics instrumentation.
//!
//! Captures per-service deploy duration and Docker image size to identify
//! deployment bottlenecks.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;

use crate::docker::{DockerImage, KupDocker};

/// Metrics for a single deployed service (container).
pub struct ServiceMetrics {
    /// Time to pull or build the Docker image.
    pub pull: Duration,
    /// Time to create and start the Docker container.
    pub setup: Duration,
    /// Time for post-startup work (RPC readiness, port binding, etc.).
    pub work: Duration,
    /// Total end-to-end deploy time for this service.
    pub total: Duration,
    /// Docker image size in bytes.
    pub image_size_bytes: Option<u64>,
    /// Docker image reference (e.g. "ghcr.io/paradigmxyz/op-reth:v1.0").
    pub image_ref: String,
}

impl ServiceMetrics {
    /// Build from deploy timings and a `DockerImage` reference.
    pub fn from_timings(
        total: Duration,
        timings: &ContainerDeployTimings,
        image_size_bytes: Option<u64>,
        image: &DockerImage,
    ) -> Self {
        Self::from_timings_with_ref(total, timings, image_size_bytes, image.to_string())
    }

    /// Build from deploy timings and a pre-resolved image reference string.
    pub fn from_timings_with_ref(
        total: Duration,
        timings: &ContainerDeployTimings,
        image_size_bytes: Option<u64>,
        image_ref: String,
    ) -> Self {
        let work = total.saturating_sub(timings.pull + timings.setup);
        Self {
            pull: timings.pull,
            setup: timings.setup,
            work,
            total,
            image_size_bytes,
            image_ref,
        }
    }

    /// Build for composite services with no pull/setup breakdown.
    pub fn composite(total: Duration, image_size_bytes: Option<u64>, image_ref: String) -> Self {
        Self {
            pull: Duration::ZERO,
            setup: Duration::ZERO,
            work: Duration::ZERO,
            total,
            image_size_bytes,
            image_ref,
        }
    }
}

/// Timings returned by `deploy_container()` for the pull and setup phases.
pub struct ContainerDeployTimings {
    /// Time spent pulling or building the Docker image.
    pub pull: Duration,
    /// Time spent creating and starting the container.
    pub setup: Duration,
}

/// Deployment metrics — a map of container name → service metrics,
/// plus the total deployment wall-clock time.
#[derive(Default)]
pub struct DeploymentMetrics {
    /// Per-service metrics keyed by container name.
    pub services: BTreeMap<String, ServiceMetrics>,
    /// Total deployment wall-clock time.
    pub total: Duration,
}

/// TOML-serializable representation of per-service metrics.
#[derive(Serialize)]
struct ServiceMetricsToml {
    image: String,
    pull_ms: u64,
    setup_ms: u64,
    work_ms: u64,
    total_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_size_bytes: Option<u64>,
}

/// TOML-serializable representation of deployment metrics.
#[derive(Serialize)]
struct DeploymentMetricsToml {
    total_ms: u64,
    services: BTreeMap<String, ServiceMetricsToml>,
}

impl DeploymentMetrics {
    /// Record metrics for a single service.
    pub fn record(&mut self, name: String, metrics: ServiceMetrics) {
        self.services.insert(name, metrics);
    }

    /// Format the metrics summary as a human-readable table.
    pub fn format_summary(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();

        writeln!(out, "=== Deployment Metrics ===").ok();
        writeln!(
            out,
            "  {:<30} {:<50} {:>6} {:>6} {:>6} {:>6}   {:>10}",
            "Service", "Image", "Pull", "Setup", "Work", "Total", "Size"
        )
        .ok();

        for (name, m) in &self.services {
            writeln!(
                out,
                "  {:<30} {:<50} {:>6} {:>6} {:>6} {:>6}   {:>10}",
                name,
                truncate_image_ref(&m.image_ref, 50),
                format_duration(m.pull),
                format_duration(m.setup),
                format_duration(m.work),
                format_duration(m.total),
                format_size(m.image_size_bytes),
            )
            .ok();
        }

        writeln!(out, "  Total: {}", format_duration(self.total)).ok();
        out
    }

    /// Emit a formatted summary table via `tracing::info!`.
    pub fn log_summary(&self) {
        tracing::info!("\n{}", self.format_summary());
    }

    /// Serialize to TOML.
    pub fn to_toml(&self) -> Result<String, anyhow::Error> {
        use anyhow::Context;

        let toml_data = DeploymentMetricsToml {
            total_ms: self.total.as_millis() as u64,
            services: self
                .services
                .iter()
                .map(|(name, m)| {
                    (
                        name.clone(),
                        ServiceMetricsToml {
                            image: m.image_ref.clone(),
                            pull_ms: m.pull.as_millis() as u64,
                            setup_ms: m.setup.as_millis() as u64,
                            work_ms: m.work.as_millis() as u64,
                            total_ms: m.total.as_millis() as u64,
                            image_size_bytes: m.image_size_bytes,
                        },
                    )
                })
                .collect(),
        };

        toml::to_string_pretty(&toml_data).context("Failed to serialize metrics to TOML")
    }

    /// Write metrics to a TOML file.
    pub fn write_to_file(&self, path: &std::path::Path) -> Result<(), anyhow::Error> {
        use anyhow::Context;

        let content = self.to_toml()?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write metrics to {}", path.display()))?;

        tracing::info!(path = %path.display(), "Deployment metrics written to file");
        Ok(())
    }
}

/// Image info retrieved from Docker inspect.
pub(crate) struct ContainerImageInfo {
    /// Docker image size in bytes.
    pub size: Option<u64>,
    /// Image name/reference (e.g. "ghcr.io/paradigmxyz/op-reth:v1.0").
    pub image_ref: String,
}

/// Best-effort image info lookup via container inspect → image inspect.
pub(crate) async fn get_image_info(docker: &KupDocker, container_id: &str) -> ContainerImageInfo {
    let default = ContainerImageInfo {
        size: None,
        image_ref: String::new(),
    };

    let Ok(container_info) = docker.inspect_container(container_id, None).await else {
        return default;
    };

    let image_ref = container_info
        .config
        .as_ref()
        .and_then(|c| c.image.clone())
        .unwrap_or_default();

    let Some(image_id) = container_info.image else {
        return ContainerImageInfo {
            size: None,
            image_ref,
        };
    };

    let size = docker
        .inspect_image(&image_id)
        .await
        .ok()
        .and_then(|info| info.size)
        .and_then(|s| u64::try_from(s).ok());

    ContainerImageInfo { size, image_ref }
}

/// Best-effort image size lookup via container inspect → image inspect.
pub(crate) async fn get_image_size(docker: &KupDocker, container_id: &str) -> Option<u64> {
    get_image_info(docker, container_id).await.size
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 0.1 {
        format!("{:.0}ms", d.as_millis())
    } else {
        format!("{:.1}s", secs)
    }
}

fn format_size(bytes: Option<u64>) -> String {
    match bytes {
        None => "-".to_string(),
        Some(b) if b >= 1_000_000_000 => format!("{:.1} GB", b as f64 / 1_000_000_000.0),
        Some(b) if b >= 1_000_000 => format!("{:.0} MB", b as f64 / 1_000_000.0),
        Some(b) => format!("{:.0} KB", b as f64 / 1_000.0),
    }
}

fn truncate_image_ref(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("…{}", &s[s.len() - (max - 1)..])
    }
}
