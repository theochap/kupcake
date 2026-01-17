//! Grafana and Prometheus deployment for metrics collection and visualization.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    docker::{
        ContainerPorts, CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping,
        ServiceConfig,
    },
    fs::FsHandler,
};

/// Default ports for monitoring components.
pub const DEFAULT_PROMETHEUS_PORT: u16 = 9099;
pub const DEFAULT_GRAFANA_PORT: u16 = 3019;

/// Host port configuration for Prometheus (used in Bridge mode).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrometheusHostPorts {
    /// Host port for server endpoint.
    pub server: Option<u16>,
}

impl Default for PrometheusHostPorts {
    fn default() -> Self {
        Self {
            server: Some(0), // Let OS pick an available port
        }
    }
}

/// Runtime port information for Prometheus containers.
pub enum PrometheusContainerPorts {
    /// Host network mode - all communication via localhost with dynamically assigned ports.
    Host {
        /// Bound host ports for this container.
        bound_ports: PrometheusHostPorts,
    },
    /// Bridge network mode - internal communication via container name, host access via mapped ports.
    Bridge {
        /// Container name for internal Docker network URLs.
        container_name: String,
        /// Bound host ports for this container (for host access).
        bound_ports: PrometheusHostPorts,
    },
}

impl PrometheusContainerPorts {
    /// Get the HTTP URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_http_url(&self, container_port: u16) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .server
                    .ok_or_else(|| anyhow::anyhow!("Server port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge { container_name, .. } => {
                format!("http://{}:{}/", container_name, container_port)
            }
        };
        Url::parse(&url_str).context("Failed to parse HTTP URL")
    }

    /// Get the HTTP URL for host access.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_http_url(&self) -> Option<anyhow::Result<Url>> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.server.map(|port| {
                    Url::parse(&format!("http://localhost:{}/", port))
                        .context("Failed to parse HTTP URL")
                })
            }
        }
    }
}

/// Default Docker image for Prometheus.
pub const DEFAULT_PROMETHEUS_DOCKER_IMAGE: &str = "prom/prometheus";
/// Default Docker tag for Prometheus.
pub const DEFAULT_PROMETHEUS_DOCKER_TAG: &str = "latest";

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

    /// Host ports configuration. Only populated in Bridge mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ports: Option<PrometheusHostPorts>,

    /// Scrape interval in seconds.
    pub scrape_interval: u64,
}

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
            host_ports: Some(PrometheusHostPorts::default()),
            scrape_interval: 15,
        }
    }
}

/// Host port configuration for Grafana (used in Bridge mode).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GrafanaHostPorts {
    /// Host port for server endpoint.
    pub server: Option<u16>,
}

impl Default for GrafanaHostPorts {
    fn default() -> Self {
        Self {
            server: Some(0), // Let OS pick an available port
        }
    }
}

/// Runtime port information for Grafana containers.
pub enum GrafanaContainerPorts {
    /// Host network mode - all communication via localhost with dynamically assigned ports.
    Host {
        /// Bound host ports for this container.
        bound_ports: GrafanaHostPorts,
    },
    /// Bridge network mode - internal communication via container name, host access via mapped ports.
    Bridge {
        /// Container name for internal Docker network URLs.
        container_name: String,
        /// Bound host ports for this container (for host access).
        bound_ports: GrafanaHostPorts,
    },
}

impl GrafanaContainerPorts {
    /// Get the HTTP URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_http_url(&self, container_port: u16) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .server
                    .ok_or_else(|| anyhow::anyhow!("Server port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge { container_name, .. } => {
                format!("http://{}:{}/", container_name, container_port)
            }
        };
        Url::parse(&url_str).context("Failed to parse HTTP URL")
    }

    /// Get the HTTP URL for host access.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_http_url(&self) -> Option<anyhow::Result<Url>> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.server.map(|port| {
                    Url::parse(&format!("http://localhost:{}/", port))
                        .context("Failed to parse HTTP URL")
                })
            }
        }
    }
}

/// Default Docker image for Grafana.
pub const DEFAULT_GRAFANA_DOCKER_IMAGE: &str = "grafana/grafana";
/// Default Docker tag for Grafana.
pub const DEFAULT_GRAFANA_DOCKER_TAG: &str = "latest";

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

    /// Host ports configuration. Only populated in Bridge mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ports: Option<GrafanaHostPorts>,

    /// Admin username.
    pub admin_user: String,

    /// Admin password.
    pub admin_password: String,
}

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
            host_ports: Some(GrafanaHostPorts::default()),
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

    /// Port for the Prometheus server (container port).
    pub port: u16,

    /// Port information for this container.
    pub ports: PrometheusContainerPorts,
}

impl PrometheusHandler {
    /// Get the internal URL for container-to-container communication.
    pub fn internal_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_http_url(self.port)
    }

    /// Get the host-accessible URL (if published).
    pub fn host_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_http_url()
    }
}

/// Handler for Grafana.
pub struct GrafanaHandler {
    pub container_id: String,
    pub container_name: String,

    /// Port for the Grafana server (container port, always 3000 internally).
    pub port: u16,

    /// Port information for this container.
    pub ports: GrafanaContainerPorts,
}

impl GrafanaHandler {
    /// Get the internal URL for container-to-container communication.
    pub fn internal_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_http_url(self.port)
    }

    /// Get the host-accessible URL (if published).
    pub fn host_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_http_url()
    }
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

        // Extract port value for PortMapping from host_ports
        let server = self
            .prometheus
            .host_ports
            .as_ref()
            .and_then(|hp| hp.server);

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> =
            PortMapping::tcp_optional(self.prometheus.port, server)
                .into_iter()
                .collect();

        let service_config = ServiceConfig::new(self.prometheus.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .bind_str(format!(
                "{}:{}:ro",
                host_config_path.join("prometheus.yml").display(),
                container_config_path.join("prometheus.yml").display()
            ));

        let service_handler = docker
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

        // Convert HashMap bound_ports to PrometheusHostPorts
        let bound_host_ports = PrometheusHostPorts {
            server: service_handler.ports.get_tcp_host_port(self.prometheus.port),
        };

        // Create typed ContainerPorts
        let typed_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => PrometheusContainerPorts::Host {
                bound_ports: bound_host_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => PrometheusContainerPorts::Bridge {
                container_name: container_name.clone(),
                bound_ports: bound_host_ports,
            },
        };

        let host_url = typed_ports.host_http_url();

        tracing::info!(
            container_id = %service_handler.container_id,
            container_name = %service_handler.container_name,
            ?host_url,
            "Prometheus container started"
        );

        Ok(PrometheusHandler {
            container_id: service_handler.container_id,
            container_name: service_handler.container_name,
            port: self.prometheus.port,
            ports: typed_ports,
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

        // Extract port value for PortMapping from host_ports
        let server = self
            .grafana
            .host_ports
            .as_ref()
            .and_then(|hp| hp.server);

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> =
            PortMapping::tcp_optional(GRAFANA_INTERNAL_PORT, server)
                .into_iter()
                .collect();

        let service_config = ServiceConfig::new(self.grafana.docker_image.clone())
            .ports(port_mappings)
            .bind_str(format!(
                "{}:/etc/grafana/provisioning:ro",
                grafana_provisioning_path.display()
            ))
            .env(env);

        let service_handler = docker
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

        // Convert HashMap bound_ports to GrafanaHostPorts
        let bound_host_ports = GrafanaHostPorts {
            server: service_handler.ports.get_tcp_host_port(GRAFANA_INTERNAL_PORT),
        };

        // Create typed ContainerPorts
        let typed_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => GrafanaContainerPorts::Host {
                bound_ports: bound_host_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => GrafanaContainerPorts::Bridge {
                container_name: container_name.clone(),
                bound_ports: bound_host_ports,
            },
        };

        let host_url = typed_ports.host_http_url();

        tracing::info!(
            container_id = %service_handler.container_id,
            container_name = %service_handler.container_name,
            ?host_url,
            "Grafana container started"
        );

        Ok(GrafanaHandler {
            container_id: service_handler.container_id,
            container_name: service_handler.container_name,
            port: GRAFANA_INTERNAL_PORT,
            ports: typed_ports,
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

        let prometheus_url = prometheus_handler.internal_url();
        let grafana_url = grafana_handler.internal_url();

        tracing::info!(
            ?prometheus_url,
            ?grafana_url,
            "Monitoring stack started successfully"
        );

        Ok(MonitoringHandler {
            prometheus: prometheus_handler,
            grafana: grafana_handler,
        })
    }
}
