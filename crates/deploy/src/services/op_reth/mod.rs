//! op-reth execution client service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpRethCmdBuilder;

use crate::{
    ExposedPort,
    docker::{
        ContainerPorts, CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping,
        ServiceConfig,
    },
    services::kona_node::P2pKeypair,
};

/// Default ports for op-reth.
pub const DEFAULT_HTTP_PORT: u16 = 9545;
pub const DEFAULT_WS_PORT: u16 = 9546;
pub const DEFAULT_AUTHRPC_PORT: u16 = 9551;
pub const DEFAULT_DISCOVERY_PORT: u16 = 30303;
pub const DEFAULT_LISTEN_PORT: u16 = 30303;
pub const DEFAULT_METRICS_PORT: u16 = 9001;

/// Container port configuration for op-reth.
/// These are the ports used inside the container (only relevant in Bridge mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpRethContainerPorts {
    /// Port for HTTP JSON-RPC server.
    pub http: u16,
    /// Port for WebSocket JSON-RPC server.
    pub ws: u16,
    /// Port for authenticated Engine API.
    pub authrpc: u16,
    /// Port for P2P discovery (UDP).
    pub discovery: u16,
    /// Port for P2P listen (TCP).
    pub listen: u16,
    /// Port for metrics.
    pub metrics: u16,
}

impl Default for OpRethContainerPorts {
    fn default() -> Self {
        Self {
            http: DEFAULT_HTTP_PORT,
            ws: DEFAULT_WS_PORT,
            authrpc: DEFAULT_AUTHRPC_PORT,
            discovery: DEFAULT_DISCOVERY_PORT,
            listen: DEFAULT_LISTEN_PORT,
            metrics: DEFAULT_METRICS_PORT,
        }
    }
}

/// Bound host port configuration for op-reth.
/// These are the actual ports bound on the host (Some = published, None = internal only).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpRethBoundPorts {
    /// Host port for HTTP JSON-RPC.
    pub http: Option<u16>,
    /// Host port for WebSocket JSON-RPC.
    pub ws: Option<u16>,
    /// Host port for authenticated Engine API.
    pub authrpc: Option<u16>,
    /// Host port for P2P discovery (UDP).
    pub discovery: Option<u16>,
    /// Host port for P2P listen (TCP).
    pub listen: Option<u16>,
    /// Host port for metrics.
    pub metrics: Option<u16>,
}

impl Default for OpRethBoundPorts {
    fn default() -> Self {
        Self {
            // Default: publish HTTP and WS to host (port 0 = OS picks), others internal only
            http: Some(0),
            ws: Some(0),
            authrpc: None,
            discovery: None,
            listen: None,
            metrics: None,
        }
    }
}

/// Unified port configuration for op-reth.
/// This is the single source of truth for all port information.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum OpRethPorts {
    /// Host network mode - only bound ports matter.
    Host {
        /// Bound host ports for this container.
        bound_ports: OpRethBoundPorts,
    },
    /// Bridge network mode - needs both container ports and bound ports.
    Bridge {
        /// Container name for internal Docker network URLs.
        container_name: String,
        /// Container ports used inside the container.
        container_ports: OpRethContainerPorts,
        /// Bound host ports for this container (for host access).
        bound_ports: OpRethBoundPorts,
    },
}

impl OpRethPorts {
    /// Get the HTTP URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_http_url(&self) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .http
                    .ok_or_else(|| anyhow::anyhow!("HTTP port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge {
                container_name,
                container_ports,
                ..
            } => {
                format!("http://{}:{}/", container_name, container_ports.http)
            }
        };
        Url::parse(&url_str).context("Failed to parse HTTP URL")
    }

    /// Get the WebSocket URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_ws_url(&self) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .ws
                    .ok_or_else(|| anyhow::anyhow!("WebSocket port not bound"))?;
                format!("ws://localhost:{}/", port)
            }
            Self::Bridge {
                container_name,
                container_ports,
                ..
            } => {
                format!("ws://{}:{}/", container_name, container_ports.ws)
            }
        };
        Url::parse(&url_str).context("Failed to parse WebSocket URL")
    }

    /// Get the authenticated RPC URL for Engine API (internal communication).
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_authrpc_url(&self) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .authrpc
                    .ok_or_else(|| anyhow::anyhow!("Authrpc port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge {
                container_name,
                container_ports,
                ..
            } => {
                format!("http://{}:{}/", container_name, container_ports.authrpc)
            }
        };
        Url::parse(&url_str).context("Failed to parse authrpc URL")
    }

    /// Get the HTTP URL for host access.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_http_url(&self) -> Option<anyhow::Result<Url>> {
        let bound_port = match self {
            Self::Host { bound_ports } => bound_ports.http,
            Self::Bridge { bound_ports, .. } => bound_ports.http,
        };

        bound_port.map(|port| {
            Url::parse(&format!("http://localhost:{}/", port))
                .context("Failed to parse HTTP URL")
        })
    }

    /// Get the WebSocket URL for host access.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_ws_url(&self) -> Option<anyhow::Result<Url>> {
        let bound_port = match self {
            Self::Host { bound_ports } => bound_ports.ws,
            Self::Bridge { bound_ports, .. } => bound_ports.ws,
        };

        bound_port.map(|port| {
            Url::parse(&format!("ws://localhost:{}/", port))
                .context("Failed to parse WebSocket URL")
        })
    }

    /// Get the container name if in bridge mode.
    ///
    /// Returns None in host mode.
    pub fn container_name(&self) -> Option<&str> {
        match self {
            Self::Host { .. } => None,
            Self::Bridge { container_name, .. } => Some(container_name),
        }
    }

    /// Get the hostname for enode URL construction.
    ///
    /// In bridge mode, returns the container name.
    /// In host mode, returns "localhost".
    pub fn enode_hostname(&self) -> &str {
        match self {
            Self::Host { .. } => "localhost",
            Self::Bridge { container_name, .. } => container_name,
        }
    }

    /// Get the discovery port for enode URL construction.
    ///
    /// In bridge mode, returns the container discovery port.
    /// In host mode, returns the bound discovery port.
    pub fn enode_discovery_port(&self) -> anyhow::Result<u16> {
        match self {
            Self::Host { bound_ports } => bound_ports
                .discovery
                .ok_or_else(|| anyhow::anyhow!("Discovery port not bound")),
            Self::Bridge {
                container_ports, ..
            } => Ok(container_ports.discovery),
        }
    }
}

/// Host port configuration for op-reth (used in Bridge mode).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpRethHostPorts {
    /// Host port for HTTP JSON-RPC.
    pub http: Option<u16>,
    /// Host port for WebSocket JSON-RPC.
    pub ws: Option<u16>,
    /// Host port for authenticated Engine API.
    pub authrpc: Option<u16>,
    /// Host port for P2P discovery (UDP).
    pub discovery: Option<u16>,
    /// Host port for P2P listen (TCP).
    pub listen: Option<u16>,
    /// Host port for metrics.
    pub metrics: Option<u16>,
}

impl Default for OpRethHostPorts {
    fn default() -> Self {
        Self {
            // Default: publish HTTP and WS to host (port 0 = OS picks), others internal only
            http: Some(0),
            ws: Some(0),
            authrpc: None,
            discovery: None,
            listen: None,
            metrics: None,
        }
    }
}
/// Configuration for the op-reth execution client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpRethBuilder {
    /// Docker image configuration for op-reth.
    pub docker_image: DockerImage,
    /// Container name for op-reth.
    pub container_name: String,
    /// Name of the network interface
    pub net_if: Option<String>,
    /// Host for the HTTP RPC endpoint.
    pub host: String,
    /// Unified port configuration.
    pub ports: OpRethPorts,
    /// P2P secret key (32 bytes hex-encoded) for deterministic node identity.
    /// If None, a random key will be generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p2p_secret_key: Option<String>,
    /// Extra arguments to pass to op-reth.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

/// Default Docker image for op-reth.
pub const DEFAULT_DOCKER_IMAGE: &str = "op-reth";
/// Default Docker tag for op-reth.
pub const DEFAULT_DOCKER_TAG: &str = "local";

impl Default for OpRethBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-reth".to_string(),
            host: "0.0.0.0".to_string(),
            ports: OpRethPorts::Bridge {
                container_name: "kupcake-op-reth".to_string(),
                container_ports: OpRethContainerPorts::default(),
                bound_ports: OpRethBoundPorts::default(),
            },
            net_if: None,
            p2p_secret_key: None,
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running op-reth instance.
pub struct OpRethHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// P2P keypair for this node (used for enode computation).
    pub p2p_keypair: P2pKeypair,
    /// Unified port information for this container.
    pub ports: OpRethPorts,
}

impl OpRethHandler {
    /// Returns the enode URL for this node using the container name as hostname.
    ///
    /// This computes the enode from the precomputed P2P keypair, so it's available
    /// immediately after the container is started without querying the node.
    pub fn enode(&self) -> anyhow::Result<String> {
        let hostname = self.ports.enode_hostname();
        let discovery_port = self.ports.enode_discovery_port()?;
        Ok(self.p2p_keypair.to_enode(hostname, discovery_port))
    }

    /// Get the internal HTTP RPC URL for container-to-container communication.
    pub fn internal_http_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_http_url()
    }

    /// Get the internal WebSocket RPC URL for container-to-container communication.
    pub fn internal_ws_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_ws_url()
    }

    /// Get the internal authenticated RPC URL for Engine API.
    pub fn internal_authrpc_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_authrpc_url()
    }

    /// Get the host-accessible HTTP RPC URL (if published).
    pub fn host_http_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_http_url()
    }

    /// Get the host-accessible WebSocket RPC URL (if published).
    pub fn host_ws_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_ws_url()
    }
}

impl OpRethBuilder {
    /// Start the op-reth execution client.
    ///
    /// # Arguments
    /// * `docker` - Docker client
    /// * `host_config_path` - Path on host for config files
    /// * `sequencer_rpc` - Optional URL of the sequencer's op-reth HTTP RPC.
    ///   If None (for sequencer nodes), uses self as sequencer.
    ///   If Some (for validator nodes), connects to the specified sequencer.
    /// * `jwt_filename` - The JWT secret filename (shared with kona-node)
    /// * `bootnodes` - List of enode URLs for P2P peer discovery
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        sequencer_rpc: Option<&Url>,
        jwt_filename: &str,
        bootnodes: &[String],
    ) -> Result<OpRethHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // Create or use the provided P2P keypair
        let p2p_keypair = match &self.p2p_secret_key {
            Some(key) => P2pKeypair::from_private_key(key)
                .context("Failed to create P2P keypair from provided secret key")?,
            None => P2pKeypair::generate(),
        };

        tracing::debug!(
            container_name = %self.container_name,
            node_id = %p2p_keypair.node_id,
            "Using P2P keypair for op-reth"
        );

        // Extract container ports and bound ports from the unified ports enum
        let (container_ports, bound_ports) = match &self.ports {
            OpRethPorts::Host { bound_ports } => {
                // In host mode, use default container ports
                (OpRethContainerPorts::default(), bound_ports.clone())
            }
            OpRethPorts::Bridge {
                container_ports,
                bound_ports,
                ..
            } => (*container_ports, bound_ports.clone()),
        };

        // For sequencer nodes, point to self. For validators, point to the sequencer.
        let sequencer_http = sequencer_rpc
            .map(|url| url.to_string())
            .unwrap_or_else(|| format!("http://{}:{}", self.container_name, container_ports.http));

        let cmd = OpRethCmdBuilder::new(
            container_config_path.join("genesis.json"),
            container_config_path.join(format!("reth-data-{}", self.container_name)),
        )
        .http_port(container_ports.http)
        .ws_port(container_ports.ws)
        .authrpc_port(container_ports.authrpc)
        .authrpc_jwtsecret(container_config_path.join(jwt_filename))
        .metrics("0.0.0.0", container_ports.metrics)
        .discovery(true)
        .discovery_port(container_ports.discovery)
        .sequencer_http(sequencer_http)
        .bootnodes(bootnodes.to_vec())
        .extra_args(self.extra_args.clone())
        .net_if(self.net_if.clone())
        .listen_port(container_ports.listen)
        .nat_dns(self.container_name.clone())
        .p2p_secret_key(&p2p_keypair.private_key)
        .build();

        // Build port mappings based on network mode
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(container_ports.http, bound_ports.http),
            PortMapping::tcp_optional(container_ports.ws, bound_ports.ws),
            PortMapping::tcp_optional(container_ports.authrpc, bound_ports.authrpc),
            PortMapping::tcp_optional(container_ports.metrics, bound_ports.metrics),
            // P2P listen port (TCP for devp2p)
            PortMapping::tcp_optional(container_ports.listen, bound_ports.listen),
            // Discovery port (UDP for discv5)
            PortMapping::udp_optional(container_ports.discovery, bound_ports.discovery),
        ]
        .into_iter()
        .flatten()
        .collect();

        let exposed_ports: Vec<ExposedPort> = [
            ExposedPort::tcp(container_ports.authrpc),
            ExposedPort::tcp(container_ports.metrics),
            ExposedPort::tcp(container_ports.listen),
            ExposedPort::udp(container_ports.discovery),
        ]
        .into_iter()
        .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .expose_ports(exposed_ports)
            .bind(host_config_path, &container_config_path, "rw");

        let service_handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions {
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-reth container")?;

        // Build OpRethBoundPorts with actual bound ports from Docker
        let actual_bound_ports = OpRethBoundPorts {
            http: service_handler.ports.get_tcp_host_port(container_ports.http),
            ws: service_handler.ports.get_tcp_host_port(container_ports.ws),
            authrpc: service_handler.ports.get_tcp_host_port(container_ports.authrpc),
            discovery: service_handler.ports.get_udp_host_port(container_ports.discovery),
            listen: service_handler.ports.get_tcp_host_port(container_ports.listen),
            metrics: service_handler.ports.get_tcp_host_port(container_ports.metrics),
        };

        // Create runtime OpRethPorts with actual bound ports
        let runtime_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => OpRethPorts::Host {
                bound_ports: actual_bound_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => OpRethPorts::Bridge {
                container_name: container_name.clone(),
                container_ports,
                bound_ports: actual_bound_ports,
            },
        };

        let http_host_url = runtime_ports.host_http_url();

        tracing::info!(
            container_id = %service_handler.container_id,
            container_name = %service_handler.container_name,
            ?http_host_url,
            "op-reth container started"
        );

        Ok(OpRethHandler {
            container_id: service_handler.container_id,
            container_name: service_handler.container_name,
            p2p_keypair,
            ports: runtime_ports,
        })
    }
}
