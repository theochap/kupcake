//! op-conductor service for sequencer consensus.
//!
//! The op-conductor manages multiple sequencer nodes using Raft consensus
//! to provide high availability for sequencing.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpConductorCmdBuilder;

use crate::docker::{
    ContainerPorts, CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping,
    ServiceConfig,
};

use super::op_reth::OpRethHandler;

/// Default ports for op-conductor.
pub const DEFAULT_RPC_PORT: u16 = 8547;
pub const DEFAULT_CONSENSUS_PORT: u16 = 50050;

/// Default Docker image for op-conductor.
pub const DEFAULT_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-conductor";
/// Default Docker tag for op-conductor.
pub const DEFAULT_DOCKER_TAG: &str = "v0.9.0";

/// Information about a sequencer node that the conductor manages.
#[derive(Debug, Clone)]
pub struct SequencerInfo {
    /// Unique ID for this sequencer in the Raft cluster.
    pub server_id: String,
    /// The kona-node RPC URL for this sequencer.
    pub node_rpc: Url,
    /// The op-reth RPC URL for this sequencer.
    pub execution_rpc: Url,
}

/// Host port configuration for op-conductor (used in Bridge mode).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpConductorHostPorts {
    /// Host port for RPC endpoint.
    pub rpc: Option<u16>,
    /// Host port for consensus.
    pub consensus: Option<u16>,
}

impl Default for OpConductorHostPorts {
    fn default() -> Self {
        Self {
            rpc: Some(0),
            consensus: None,
        }
    }
}

/// Runtime port information for op-conductor containers.
pub enum OpConductorContainerPorts {
    /// Host network mode - all communication via localhost with dynamically assigned ports.
    Host {
        /// Bound host ports for this container.
        bound_ports: OpConductorHostPorts,
    },
    /// Bridge network mode - internal communication via container name, host access via mapped ports.
    Bridge {
        /// Container name for internal Docker network URLs.
        container_name: String,
        /// Bound host ports for this container (for host access).
        bound_ports: OpConductorHostPorts,
    },
}

impl OpConductorContainerPorts {
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

    /// Get the host port for consensus.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_consensus_port(&self) -> Option<u16> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.consensus
            }
        }
    }
}

/// Configuration for the op-conductor component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpConductorBuilder {
    /// Docker image configuration for op-conductor.
    pub docker_image: DockerImage,
    /// Container name for op-conductor.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Port for the op-conductor RPC server (container port).
    pub rpc_port: u16,
    /// Port for Raft consensus (container port).
    pub consensus_port: u16,
    /// Host ports configuration. Only populated in Bridge mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ports: Option<OpConductorHostPorts>,
    /// Health check interval.
    pub healthcheck_interval: String,
    /// Unsafe interval - interval allowed between unsafe head and now measured in seconds.
    pub healthcheck_unsafe_interval: String,
    /// Minimum number of peers required to be considered healthy.
    pub healthcheck_min_peer_count: String,
    /// Extra arguments to pass to op-conductor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

impl Default for OpConductorBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-conductor".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            consensus_port: DEFAULT_CONSENSUS_PORT,
            host_ports: Some(OpConductorHostPorts::default()),
            healthcheck_interval: "5".to_string(),
            healthcheck_unsafe_interval: "600".to_string(),
            healthcheck_min_peer_count: "1".to_string(),
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running op-conductor instance.
pub struct OpConductorHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// RPC port (container port).
    pub rpc_port: u16,
    /// Port information for this container.
    pub ports: OpConductorContainerPorts,
}

impl OpConductorHandler {
    /// Get the internal RPC URL for container-to-container communication.
    pub fn internal_rpc_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_http_url(self.rpc_port)
    }

    /// Get the host-accessible RPC URL (if published).
    pub fn host_rpc_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_rpc_url()
    }

    /// Get the host-accessible consensus port (if published).
    pub fn host_consensus_port(&self) -> Option<u16> {
        self.ports.host_consensus_port()
    }
}

impl OpConductorBuilder {
    /// Start the op-conductor for a single sequencer (leader/bootstrap mode).
    ///
    /// This is used when starting the first conductor in a cluster.
    pub async fn start_leader(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        server_id: &str,
        op_reth_handler: &OpRethHandler,
        kona_node_rpc_url: &str,
    ) -> Result<OpConductorHandler, anyhow::Error> {
        self.start_internal(
            docker,
            host_config_path,
            server_id,
            op_reth_handler,
            kona_node_rpc_url,
            true, // bootstrap
        )
        .await
    }

    /// Start the op-conductor for a follower sequencer.
    ///
    /// This is used when adding additional sequencers to an existing cluster.
    pub async fn start_follower(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        server_id: &str,
        op_reth_handler: &OpRethHandler,
        kona_node_rpc_url: &str,
    ) -> Result<OpConductorHandler, anyhow::Error> {
        self.start_internal(
            docker,
            host_config_path,
            server_id,
            op_reth_handler,
            kona_node_rpc_url,
            false, // not bootstrap
        )
        .await
    }

    /// Internal method to start the op-conductor.
    async fn start_internal(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        server_id: &str,
        op_reth_handler: &OpRethHandler,
        kona_node_rpc_url: &str,
        bootstrap: bool,
    ) -> Result<OpConductorHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");
        let raft_storage_dir = container_config_path.join("raft");
        let rollup_config_path = container_config_path.join("rollup.json");

        let cmd = OpConductorCmdBuilder::new(
            kona_node_rpc_url.to_string(),
            op_reth_handler.internal_http_url()?.to_string(),
            server_id,
            raft_storage_dir.display().to_string(),
            rollup_config_path.display().to_string(),
        )
        .raft_bootstrap(bootstrap)
        .rpc_addr(&self.host)
        .rpc_port(self.rpc_port)
        .consensus_addr(&self.container_name) // Must be resolvable by other nodes
        .consensus_port(self.consensus_port)
        .healthcheck_interval(&self.healthcheck_interval)
        .healthcheck_unsafe_interval(&self.healthcheck_unsafe_interval)
        .healthcheck_min_peer_count(&self.healthcheck_min_peer_count)
        .extra_args(self.extra_args.clone())
        .build();

        // Extract port values for PortMapping from host_ports
        let (rpc, consensus) = self
            .host_ports
            .as_ref()
            .map(|hp| (hp.rpc, hp.consensus))
            .unwrap_or((None, None));

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(self.rpc_port, rpc),
            PortMapping::tcp_optional(self.consensus_port, consensus),
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
            .context("Failed to start op-conductor container")?;

        // Convert HashMap bound_ports to OpConductorHostPorts
        let bound_host_ports = OpConductorHostPorts {
            rpc: service_handler.ports.get_tcp_host_port(self.rpc_port)
                .or(match &service_handler.ports {
                    ContainerPorts::Host { .. } => Some(self.rpc_port),
                    _ => None,
                }),
            consensus: service_handler.ports.get_tcp_host_port(self.consensus_port)
                .or(match &service_handler.ports {
                    ContainerPorts::Host { .. } => Some(self.consensus_port),
                    _ => None,
                }),
        };

        // Create typed ContainerPorts
        let typed_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => OpConductorContainerPorts::Host {
                bound_ports: bound_host_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => OpConductorContainerPorts::Bridge {
                container_name: container_name.clone(),
                bound_ports: bound_host_ports,
            },
        };

        let rpc_host_url = typed_ports.host_rpc_url();

        tracing::info!(
            container_id = %service_handler.container_id,
            container_name = %service_handler.container_name,
            server_id = %server_id,
            bootstrap = bootstrap,
            ?rpc_host_url,
            "op-conductor container started"
        );

        Ok(OpConductorHandler {
            container_id: service_handler.container_id,
            container_name: service_handler.container_name,
            rpc_port: self.rpc_port,
            ports: typed_ports,
        })
    }
}
