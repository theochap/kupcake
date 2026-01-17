//! op-batcher service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpBatcherCmdBuilder;

use crate::docker::{
    ContainerPorts, CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping,
    ServiceConfig,
};

use super::{anvil::AnvilHandler, kona_node::KonaNodeHandler, op_reth::OpRethHandler};

/// Default ports for op-batcher.
pub const DEFAULT_RPC_PORT: u16 = 8548;
pub const DEFAULT_METRICS_PORT: u16 = 7301;

/// Host port configuration for op-batcher (used in Bridge mode).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpBatcherHostPorts {
    /// Host port for RPC endpoint.
    pub rpc: Option<u16>,
    /// Host port for metrics.
    pub metrics: Option<u16>,
}

impl Default for OpBatcherHostPorts {
    fn default() -> Self {
        Self {
            rpc: Some(0),
            metrics: None,
        }
    }
}

/// Runtime port information for op-batcher containers.
pub enum OpBatcherContainerPorts {
    /// Host network mode - all communication via localhost with dynamically assigned ports.
    Host {
        /// Bound host ports for this container.
        bound_ports: OpBatcherHostPorts,
    },
    /// Bridge network mode - internal communication via container name, host access via mapped ports.
    Bridge {
        /// Container name for internal Docker network URLs.
        container_name: String,
        /// Bound host ports for this container (for host access).
        bound_ports: OpBatcherHostPorts,
    },
}

impl OpBatcherContainerPorts {
    /// Get the HTTP URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_http_url(&self, container_rpc_port: u16) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .rpc
                    .ok_or_else(|| anyhow::anyhow!("RPC port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge { container_name, .. } => {
                format!("http://{}:{}/", container_name, container_rpc_port)
            }
        };
        Url::parse(&url_str).context("Failed to parse HTTP URL")
    }

    /// Get the HTTP URL for host access to RPC.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_rpc_url(&self) -> Option<anyhow::Result<Url>> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.rpc.map(|port| {
                    Url::parse(&format!("http://localhost:{}/", port))
                        .context("Failed to parse HTTP URL")
                })
            }
        }
    }

    /// Get the HTTP URL for host access to metrics.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_metrics_url(&self) -> Option<anyhow::Result<Url>> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.metrics.map(|port| {
                    Url::parse(&format!("http://localhost:{}/", port))
                        .context("Failed to parse HTTP URL")
                })
            }
        }
    }
}

/// Default Docker image for op-batcher.
pub const DEFAULT_DOCKER_IMAGE: &str = "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-batcher";
/// Default Docker tag for op-batcher.
pub const DEFAULT_DOCKER_TAG: &str = "v1.15.0";

/// Configuration for the op-batcher component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpBatcherBuilder {
    /// Docker image configuration for op-batcher.
    pub docker_image: DockerImage,
    /// Container name for op-batcher.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Port for the op-batcher RPC server (container port).
    pub rpc_port: u16,
    /// Port for metrics (container port).
    pub metrics_port: u16,
    /// Host ports configuration. Only populated in Bridge mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ports: Option<OpBatcherHostPorts>,
    /// Max L1 tx size in bytes (default 120000).
    pub max_l1_tx_size_bytes: u64,
    /// Target number of frames per channel.
    pub target_num_frames: u64,
    /// Sub-safety margin (number of L1 blocks).
    pub sub_safety_margin: u64,
    /// Batch submission interval.
    pub poll_interval: String,
    /// Extra arguments to pass to op-batcher.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

impl Default for OpBatcherBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-batcher".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            host_ports: Some(OpBatcherHostPorts::default()),
            max_l1_tx_size_bytes: 120000,
            target_num_frames: 1,
            sub_safety_margin: 10,
            poll_interval: "1s".to_string(),
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running op-batcher instance.
pub struct OpBatcherHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// RPC port (container port).
    pub rpc_port: u16,
    /// Port information for this container.
    pub ports: OpBatcherContainerPorts,
}

impl OpBatcherHandler {
    /// Get the internal RPC URL for container-to-container communication.
    pub fn internal_rpc_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_http_url(self.rpc_port)
    }

    /// Get the host-accessible RPC URL (if published).
    pub fn host_rpc_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_rpc_url()
    }

    /// Get the host-accessible metrics URL (if published).
    pub fn host_metrics_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_metrics_url()
    }
}

impl OpBatcherBuilder {
    /// Start the op-batcher.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        op_reth_handler: &OpRethHandler,
        kona_node_handler: &KonaNodeHandler,
    ) -> Result<OpBatcherHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        let batcher_private_key = &anvil_handler.accounts.batcher.private_key;

        let cmd = OpBatcherCmdBuilder::new(
            anvil_handler.internal_rpc_url()?.to_string(),
            op_reth_handler.internal_http_url()?.to_string(),
            kona_node_handler.internal_rpc_url()?.to_string(),
            batcher_private_key.to_string(),
        )
        .rpc_port(self.rpc_port)
        .metrics(true, "0.0.0.0", self.metrics_port)
        .data_availability_type("blobs")
        .extra_args(self.extra_args.clone())
        .build();

        self.docker_image.pull(docker).await?;

        // Extract port values for PortMapping from host_ports
        let (rpc, metrics) = self
            .host_ports
            .as_ref()
            .map(|hp| (hp.rpc, hp.metrics))
            .unwrap_or((None, None));

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(self.rpc_port, rpc),
            PortMapping::tcp_optional(self.metrics_port, metrics),
        ]
        .into_iter()
        .flatten()
        .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .bind(host_config_path, &container_config_path, "rw");

        let service_handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-batcher container")?;

        // Convert HashMap bound_ports to OpBatcherHostPorts
        let bound_host_ports = OpBatcherHostPorts {
            rpc: service_handler.ports.get_tcp_host_port(self.rpc_port),
            metrics: service_handler.ports.get_tcp_host_port(self.metrics_port),
        };

        // Create typed ContainerPorts
        let typed_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => OpBatcherContainerPorts::Host {
                bound_ports: bound_host_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => OpBatcherContainerPorts::Bridge {
                container_name: container_name.clone(),
                bound_ports: bound_host_ports,
            },
        };

        let rpc_host_url = typed_ports.host_rpc_url();
        let metrics_host_url = typed_ports.host_metrics_url();

        tracing::info!(
            container_id = %service_handler.container_id,
            container_name = %service_handler.container_name,
            ?rpc_host_url,
            ?metrics_host_url,
            "op-batcher container started"
        );

        Ok(OpBatcherHandler {
            container_id: service_handler.container_id,
            container_name: service_handler.container_name,
            rpc_port: self.rpc_port,
            ports: typed_ports,
        })
    }
}
