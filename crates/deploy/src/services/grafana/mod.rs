//! Grafana and Prometheus deployment for metrics collection and visualization.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    docker::{CreateAndStartContainerOptions, DockerImage, ExposedPort, KupDocker, PortMapping, ServiceConfig},
    fs::FsHandler,
};

/// Default ports for monitoring components.
pub const DEFAULT_PROMETHEUS_PORT: u16 = 9099;
pub const DEFAULT_GRAFANA_PORT: u16 = 3019;

/// Configuration for Prometheus.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrometheusConfig {
    /// Docker image configuration for Prometheus.
    pub docker_image: DockerImage,

    /// Container name for Prometheus.
    pub container_name: String,

    /// Host for the Prometheus server.
    pub host: String,

    /// Port for the Prometheus server (container port).
    pub port: u16,

    /// Host port for Prometheus. If None, not published to host. If Some(0), OS picks port.
    #[serde(
        default = "default_prometheus_host_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub host_port: Option<u16>,

    /// Scrape interval in seconds.
    pub scrape_interval: u64,
}

fn default_prometheus_host_port() -> Option<u16> {
    Some(0) // Let OS pick an available port
}

/// Default Docker image for Prometheus.
pub const DEFAULT_PROMETHEUS_DOCKER_IMAGE: &str = "prom/prometheus";
/// Default Docker tag for Prometheus.
pub const DEFAULT_PROMETHEUS_DOCKER_TAG: &str = "latest";

impl Default for PrometheusConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(
                DEFAULT_PROMETHEUS_DOCKER_IMAGE,
                DEFAULT_PROMETHEUS_DOCKER_TAG,
            ),
            container_name: "kupcake-prometheus".to_string(),
            host: "0.0.0.0".to_string(),
            port: DEFAULT_PROMETHEUS_PORT,
            host_port: Some(0), // Let OS pick an available port
            scrape_interval: 15,
        }
    }
}

/// Configuration for Grafana.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GrafanaConfig {
    /// Docker image configuration for Grafana.
    pub docker_image: DockerImage,

    /// Container name for Grafana.
    pub container_name: String,

    /// Host for the Grafana server.
    pub host: String,

    /// Port for the Grafana server (container port).
    pub port: u16,

    /// Host port for Grafana. If None, not published to host. If Some(0), OS picks port.
    #[serde(
        default = "default_grafana_host_port",
        skip_serializing_if = "Option::is_none"
    )]
    pub host_port: Option<u16>,

    /// Admin username.
    pub admin_user: String,

    /// Admin password.
    pub admin_password: String,
}

fn default_grafana_host_port() -> Option<u16> {
    Some(0) // Let OS pick an available port
}

/// Default Docker image for Grafana.
pub const DEFAULT_GRAFANA_DOCKER_IMAGE: &str = "grafana/grafana";
/// Default Docker tag for Grafana.
pub const DEFAULT_GRAFANA_DOCKER_TAG: &str = "latest";

impl Default for GrafanaConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(
                DEFAULT_GRAFANA_DOCKER_IMAGE,
                DEFAULT_GRAFANA_DOCKER_TAG,
            ),
            container_name: "kupcake-grafana".to_string(),
            host: "0.0.0.0".to_string(),
            port: DEFAULT_GRAFANA_PORT,
            host_port: Some(0), // Let OS pick an available port
            admin_user: "admin".to_string(),
            admin_password: "admin".to_string(),
        }
    }
}

/// Combined configuration for monitoring stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Configuration for Prometheus.
    pub prometheus: PrometheusConfig,

    /// Configuration for Grafana.
    pub grafana: GrafanaConfig,

    /// Whether monitoring is enabled.
    pub enabled: bool,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            prometheus: PrometheusConfig::default(),
            grafana: GrafanaConfig::default(),
            enabled: true,
        }
    }
}

/// Handler for Prometheus.
pub struct PrometheusHandler {
    pub container_id: String,
    pub container_name: String,

    /// The URL for the Prometheus server (internal Docker network).
    pub url: Url,

    /// The URL accessible from host (if published). None if not published.
    pub host_url: Option<Url>,
}

/// Handler for Grafana.
pub struct GrafanaHandler {
    pub container_id: String,
    pub container_name: String,

    /// The URL for the Grafana server (internal Docker network).
    pub url: Url,

    /// The URL accessible from host (if published). None if not published.
    pub host_url: Option<Url>,
}

/// Handler for the complete monitoring stack.
pub struct MonitoringHandler {
    pub prometheus: PrometheusHandler,
    pub grafana: GrafanaHandler,
}

/// Metrics target for Prometheus scraping.
pub struct MetricsTarget {
    pub job_name: String,
    pub container_name: String,
    pub port: u16,
    pub service_label: String,
    pub layer_label: String,
}

impl MonitoringConfig {
    /// Generate the Prometheus configuration file based on running services.
    async fn generate_prometheus_config(
        &self,
        host_config_path: &PathBuf,
        targets: &[MetricsTarget],
    ) -> Result<PathBuf, anyhow::Error> {
        let mut scrape_configs = String::new();
        for target in targets {
            scrape_configs.push_str(&format!(
                r#"
  - job_name: '{}'
    metrics_path: '/metrics'
    scrape_interval: {}s
    static_configs:
      - targets: ['{}:{}']
        labels:
          service: '{}'
          layer: '{}'"#,
                target.job_name,
                self.prometheus.scrape_interval,
                target.container_name,
                target.port,
                target.service_label,
                target.layer_label,
            ));
        }

        // Add Prometheus self-monitoring
        scrape_configs.push_str(&format!(
            r#"

  - job_name: 'prometheus'
    metrics_path: '/metrics'
    scrape_interval: 30s
    static_configs:
      - targets: ['localhost:{}']
        labels:
          service: 'prometheus'"#,
            self.prometheus.port
        ));

        let config_content = format!(
            r#"# Prometheus configuration for kupcake OP Stack

global:
  scrape_interval: {}s
  evaluation_interval: {}s
  scrape_timeout: 10s

  external_labels:
    cluster: 'kupcake-op-stack'
    environment: 'dev'

scrape_configs:{}"#,
            self.prometheus.scrape_interval, self.prometheus.scrape_interval, scrape_configs
        );

        let config_path = host_config_path.join("prometheus.yml");
        tokio::fs::write(&config_path, config_content)
            .await
            .context("Failed to write Prometheus config file")?;

        tracing::debug!(path = ?config_path, "Prometheus config written");
        Ok(config_path)
    }

    /// Generate the Grafana datasource configuration.
    async fn generate_grafana_datasource(
        &self,
        host_config_path: &PathBuf,
    ) -> Result<PathBuf, anyhow::Error> {
        let datasource_content = format!(
            r#"apiVersion: 1

datasources:
  - name: Prometheus
    type: prometheus
    access: proxy
    url: http://{}:{}
    isDefault: true
    editable: true
    jsonData:
      timeInterval: '{}s'
      httpMethod: 'POST'"#,
            self.prometheus.container_name, self.prometheus.port, self.prometheus.scrape_interval
        );

        let datasources_dir = host_config_path.join("grafana/provisioning/datasources");
        tokio::fs::create_dir_all(&datasources_dir)
            .await
            .context("Failed to create Grafana datasources directory")?;

        let config_path = datasources_dir.join("prometheus.yml");
        tokio::fs::write(&config_path, datasource_content)
            .await
            .context("Failed to write Grafana datasource config")?;

        tracing::debug!(path = ?config_path, "Grafana datasource config written");
        Ok(config_path)
    }

    /// Generate the Grafana dashboard provisioning configuration.
    async fn generate_grafana_dashboard_provisioning(
        &self,
        host_config_path: &PathBuf,
    ) -> Result<PathBuf, anyhow::Error> {
        let dashboard_provisioning = r#"apiVersion: 1

providers:
  - name: 'Kupcake Dashboards'
    orgId: 1
    folder: ''
    type: file
    disableDeletion: false
    editable: true
    options:
      path: /etc/grafana/provisioning/dashboards"#;

        let dashboards_dir = host_config_path.join("grafana/provisioning/dashboards");
        tokio::fs::create_dir_all(&dashboards_dir)
            .await
            .context("Failed to create Grafana dashboards directory")?;

        let config_path = dashboards_dir.join("dashboards.yml");
        tokio::fs::write(&config_path, dashboard_provisioning)
            .await
            .context("Failed to write Grafana dashboard provisioning config")?;

        tracing::debug!(path = ?config_path, "Grafana dashboard provisioning config written");
        Ok(config_path)
    }

    /// Copy dashboard files to the Grafana provisioning directory.
    async fn copy_dashboards(
        &self,
        host_config_path: &PathBuf,
        dashboards_source: &PathBuf,
    ) -> Result<(), anyhow::Error> {
        let dashboards_dest = host_config_path.join("grafana/provisioning/dashboards");

        // Read all JSON files from the source directory
        let mut entries = tokio::fs::read_dir(dashboards_source)
            .await
            .context("Failed to read dashboards source directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let file_name = path.file_name().unwrap();
                let dest_path = dashboards_dest.join(file_name);
                tokio::fs::copy(&path, &dest_path)
                    .await
                    .context(format!("Failed to copy dashboard: {:?}", file_name))?;
                tracing::debug!(src = ?path, dest = ?dest_path, "Dashboard copied");
            }
        }

        Ok(())
    }

    /// Start Prometheus container.
    async fn start_prometheus(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
    ) -> Result<PrometheusHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/etc/prometheus");

        // Build the Prometheus command
        let cmd = vec![
            "--config.file=/etc/prometheus/prometheus.yml".to_string(),
            "--storage.tsdb.path=/prometheus".to_string(),
            format!("--web.listen-address=0.0.0.0:{}", self.prometheus.port),
            "--web.enable-lifecycle".to_string(),
        ];

        self.prometheus.docker_image.pull(docker).await?;

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> =
            PortMapping::tcp_optional(self.prometheus.port, self.prometheus.host_port)
                .into_iter()
                .collect();

        let service_config = ServiceConfig::new(self.prometheus.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .expose(ExposedPort::tcp(self.prometheus.port))
            .bind_str(format!(
                "{}:{}:ro",
                host_config_path.join("prometheus.yml").display(),
                container_config_path.join("prometheus.yml").display()
            ));

        let handler = docker
            .start_service(
                &self.prometheus.container_name,
                service_config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start Prometheus container")?;

        // Build internal Docker network URL
        let url = KupDocker::build_http_url(&handler.container_name, self.prometheus.port)?;

        // Build host-accessible URL from bound port
        let host_url = handler
            .get_tcp_host_port(self.prometheus.port)
            .map(|port| Url::parse(&format!("http://localhost:{}/", port)))
            .transpose()
            .context("Failed to build Prometheus host URL")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            ?host_url,
            "Prometheus container started"
        );

        Ok(PrometheusHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            url,
            host_url,
        })
    }

    /// Start Grafana container.
    async fn start_grafana(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
    ) -> Result<GrafanaHandler, anyhow::Error> {
        // Grafana listens on port 3000 inside the container by default
        const GRAFANA_INTERNAL_PORT: u16 = 3000;

        let grafana_provisioning_path = host_config_path.join("grafana/provisioning");

        let env = vec![
            format!("GF_SECURITY_ADMIN_USER={}", self.grafana.admin_user),
            format!("GF_SECURITY_ADMIN_PASSWORD={}", self.grafana.admin_password),
            "GF_USERS_ALLOW_SIGN_UP=false".to_string(),
            "GF_AUTH_ANONYMOUS_ENABLED=true".to_string(),
            "GF_AUTH_ANONYMOUS_ORG_ROLE=Viewer".to_string(),
        ];

        self.grafana.docker_image.pull(docker).await?;

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> =
            PortMapping::tcp_optional(GRAFANA_INTERNAL_PORT, self.grafana.host_port)
                .into_iter()
                .collect();

        let service_config = ServiceConfig::new(self.grafana.docker_image.clone())
            .ports(port_mappings)
            .expose(ExposedPort::tcp(GRAFANA_INTERNAL_PORT))
            .bind_str(format!(
                "{}:/etc/grafana/provisioning:ro",
                grafana_provisioning_path.display()
            ))
            .env(env);

        let handler = docker
            .start_service(
                &self.grafana.container_name,
                service_config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start Grafana container")?;

        // Build internal Docker network URL
        let url = KupDocker::build_http_url(&handler.container_name, GRAFANA_INTERNAL_PORT)?;

        // Build host-accessible URL from bound port
        let host_url = handler
            .get_tcp_host_port(GRAFANA_INTERNAL_PORT)
            .map(|port| Url::parse(&format!("http://localhost:{}/", port)))
            .transpose()
            .context("Failed to build Grafana host URL")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            ?host_url,
            "Grafana container started"
        );

        Ok(GrafanaHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            url,
            host_url,
        })
    }

    /// Start the complete monitoring stack (Prometheus + Grafana).
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        metrics_targets: Vec<MetricsTarget>,
        dashboards_source: Option<PathBuf>,
    ) -> Result<MonitoringHandler, anyhow::Error> {
        if !self.enabled {
            anyhow::bail!("Monitoring is disabled");
        }

        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Generate Prometheus configuration
        self.generate_prometheus_config(&host_config_path, &metrics_targets)
            .await?;

        // Generate Grafana configurations
        self.generate_grafana_datasource(&host_config_path).await?;
        self.generate_grafana_dashboard_provisioning(&host_config_path)
            .await?;

        // Copy dashboards if source is provided
        if let Some(dashboards_path) = dashboards_source
            && dashboards_path.exists() {
                self.copy_dashboards(&host_config_path, &dashboards_path)
                    .await?;
            }

        tracing::info!("Starting Prometheus...");
        let prometheus_handler = self.start_prometheus(docker, &host_config_path).await?;

        tracing::info!("Starting Grafana...");
        let grafana_handler = self.start_grafana(docker, &host_config_path).await?;

        tracing::info!(
            prometheus_url = %prometheus_handler.url,
            grafana_url = %grafana_handler.url,
            "Monitoring stack started successfully"
        );

        Ok(MonitoringHandler {
            prometheus: prometheus_handler,
            grafana: grafana_handler,
        })
    }
}

// KupcakeService trait implementation
impl crate::traits::KupcakeService for MonitoringConfig {
    type Stage = crate::traits::MonitoringStage;
    type Handler = Option<MonitoringHandler>;
    type Context<'a> = crate::traits::MonitoringContext<'a>;

    const SERVICE_NAME: &'static str = "monitoring";

    async fn deploy<'a>(self, ctx: Self::Context<'a>) -> anyhow::Result<Self::Handler>
    where
        Self: 'a,
    {
        if !self.enabled {
            return Ok(None);
        }

        tracing::info!("Starting monitoring stack (Prometheus + Grafana)...");

        let host_config_path = ctx.outdata.join("monitoring");
        let metrics_targets = ctx.l2_stack.metrics_targets();

        let handler = self
            .start(ctx.docker, host_config_path, metrics_targets, ctx.dashboards_path)
            .await
            .context("Failed to start monitoring stack")?;

        Ok(Some(handler))
    }
}

