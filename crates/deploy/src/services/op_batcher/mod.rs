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

/// Container port configuration for op-batcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpBatcherContainerPorts {
    pub rpc: u16,
    pub metrics: u16,
}

impl Default for OpBatcherContainerPorts {
    fn default() -> Self {
        Self {
            rpc: DEFAULT_RPC_PORT,
            metrics: DEFAULT_METRICS_PORT,
        }
    }
}

/// Bound host port configuration for op-batcher.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpBatcherBoundPorts {
    pub rpc: Option<u16>,
    pub metrics: Option<u16>,
}

impl Default for OpBatcherBoundPorts {
    fn default() -> Self {
        Self {
            rpc: Some(0),
            metrics: None,
        }
    }
}

/// Unified port configuration for op-batcher.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum OpBatcherPorts {
    Host { bound_ports: OpBatcherBoundPorts },
    Bridge {
        container_name: String,
        container_ports: OpBatcherContainerPorts,
        bound_ports: OpBatcherBoundPorts,
    },
}

impl OpBatcherPorts {
    /// Get the HTTP URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_http_url(&self) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .rpc
                    .ok_or_else(|| anyhow::anyhow!("RPC port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge { container_name, container_ports, .. } => {
                format!("http://{}:{}/", container_name, container_ports.rpc)
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
    /// Unified port configuration.
    pub ports: OpBatcherPorts,
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
            ports: OpBatcherPorts::Bridge {
                container_name: "kupcake-op-batcher".to_string(),
                container_ports: OpBatcherContainerPorts::default(),
                bound_ports: OpBatcherBoundPorts::default(),
            },
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
    /// Port information for this container.
    pub ports: OpBatcherPorts,
}

impl OpBatcherHandler {
    /// Get the internal RPC URL for container-to-container communication.
    pub fn internal_rpc_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_http_url()
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

        // Extract ports from self.ports
        let (container_ports, bound_ports) = match &self.ports {
            OpBatcherPorts::Host { bound_ports } => (OpBatcherContainerPorts::default(), bound_ports.clone()),
            OpBatcherPorts::Bridge { container_ports, bound_ports, .. } => (*container_ports, bound_ports.clone()),
        };

        let batcher_private_key = &anvil_handler.accounts.batcher.private_key;

        let cmd = OpBatcherCmdBuilder::new(
            anvil_handler.internal_rpc_url()?.to_string(),
            op_reth_handler.internal_http_url()?.to_string(),
            kona_node_handler.internal_rpc_url()?.to_string(),
            batcher_private_key.to_string(),
        )
        .rpc_port(container_ports.rpc)
        .metrics(true, "0.0.0.0", container_ports.metrics)
        .data_availability_type("blobs")
        .extra_args(self.extra_args.clone())
        .build();

        self.docker_image.pull(docker).await?;

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(container_ports.rpc, bound_ports.rpc),
            PortMapping::tcp_optional(container_ports.metrics, bound_ports.metrics),
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

        // Build runtime ports with actual bound ports
        let actual_bound_ports = OpBatcherBoundPorts {
            rpc: service_handler.ports.get_tcp_host_port(container_ports.rpc)
                .or(match &service_handler.ports {
                    ContainerPorts::Host { .. } => Some(container_ports.rpc),
                    _ => None,
                }),
            metrics: service_handler.ports.get_tcp_host_port(container_ports.metrics)
                .or(match &service_handler.ports {
                    ContainerPorts::Host { .. } => Some(container_ports.metrics),
                    _ => None,
                }),
        };

        let runtime_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => OpBatcherPorts::Host {
                bound_ports: actual_bound_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => OpBatcherPorts::Bridge {
                container_name: container_name.clone(),
                container_ports,
                bound_ports: actual_bound_ports,
            },
        };

        let rpc_host_url = runtime_ports.host_rpc_url();
        let metrics_host_url = runtime_ports.host_metrics_url();

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
            ports: runtime_ports,
        })
    }
}
