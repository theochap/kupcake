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

/// Runtime port information for op-reth containers.
pub enum OpRethContainerPorts {
    /// Host network mode - all communication via localhost with dynamically assigned ports.
    Host {
        /// Bound host ports for this container.
        bound_ports: OpRethHostPorts,
    },
    /// Bridge network mode - internal communication via container name, host access via mapped ports.
    Bridge {
        /// Container name for internal Docker network URLs.
        container_name: String,
        /// Bound host ports for this container (for host access).
        bound_ports: OpRethHostPorts,
    },
}

impl OpRethContainerPorts {
    /// Get the HTTP URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_http_url(&self, container_http_port: u16) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .http
                    .ok_or_else(|| anyhow::anyhow!("HTTP port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge { container_name, .. } => {
                format!("http://{}:{}/", container_name, container_http_port)
            }
        };
        Url::parse(&url_str).context("Failed to parse HTTP URL")
    }

    /// Get the WebSocket URL for internal container-to-container communication.
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_ws_url(&self, container_ws_port: u16) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .ws
                    .ok_or_else(|| anyhow::anyhow!("WebSocket port not bound"))?;
                format!("ws://localhost:{}/", port)
            }
            Self::Bridge { container_name, .. } => {
                format!("ws://{}:{}/", container_name, container_ws_port)
            }
        };
        Url::parse(&url_str).context("Failed to parse WebSocket URL")
    }

    /// Get the authenticated RPC URL for Engine API (internal communication).
    ///
    /// In host mode, returns localhost with the bound port.
    /// In bridge mode, returns the container name with the container port.
    pub fn internal_authrpc_url(&self, container_authrpc_port: u16) -> anyhow::Result<Url> {
        let url_str = match self {
            Self::Host { bound_ports } => {
                let port = bound_ports
                    .authrpc
                    .ok_or_else(|| anyhow::anyhow!("Authrpc port not bound"))?;
                format!("http://localhost:{}/", port)
            }
            Self::Bridge { container_name, .. } => {
                format!("http://{}:{}/", container_name, container_authrpc_port)
            }
        };
        Url::parse(&url_str).context("Failed to parse authrpc URL")
    }

    /// Get the HTTP URL for host access.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_http_url(&self) -> Option<anyhow::Result<Url>> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.http.map(|port| {
                    Url::parse(&format!("http://localhost:{}/", port))
                        .context("Failed to parse HTTP URL")
                })
            }
        }
    }

    /// Get the WebSocket URL for host access.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_ws_url(&self) -> Option<anyhow::Result<Url>> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.ws.map(|port| {
                    Url::parse(&format!("ws://localhost:{}/", port))
                        .context("Failed to parse WebSocket URL")
                })
            }
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
    /// Port for the HTTP JSON-RPC server (container port).
    pub http_port: u16,
    /// Port for the WebSocket JSON-RPC server (container port).
    pub ws_port: u16,
    /// Port for the authenticated Engine API (container port, used by kona-node).
    pub authrpc_port: u16,
    /// Port for P2P discovery (container port).
    pub discovery_port: u16,
    /// Port for listen (container port).
    pub listen_port: u16,
    /// Port for metrics (container port).
    pub metrics_port: u16,
    /// Host ports configuration. Only populated in Bridge mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ports: Option<OpRethHostPorts>,
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
            http_port: DEFAULT_HTTP_PORT,
            ws_port: DEFAULT_WS_PORT,
            authrpc_port: DEFAULT_AUTHRPC_PORT,
            discovery_port: DEFAULT_DISCOVERY_PORT,
            listen_port: DEFAULT_LISTEN_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            host_ports: Some(OpRethHostPorts::default()),
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
    /// Port for P2P discovery (container port).
    pub discovery_port: u16,
    /// The P2P listen port (used for enode URL construction).
    pub listen_port: u16,
    /// HTTP RPC port (container port).
    pub http_port: u16,
    /// WebSocket RPC port (container port).
    pub ws_port: u16,
    /// Authenticated RPC port for Engine API (container port).
    pub authrpc_port: u16,
    /// P2P keypair for this node (used for enode computation).
    pub p2p_keypair: P2pKeypair,
    /// Port information for this container.
    pub ports: OpRethContainerPorts,
}

impl OpRethHandler {
    /// Returns the enode URL for this node using the container name as hostname.
    ///
    /// This computes the enode from the precomputed P2P keypair, so it's available
    /// immediately after the container is started without querying the node.
    pub fn enode(&self) -> String {
        self.p2p_keypair
            .to_enode(&self.container_name, self.discovery_port)
    }

    /// Get the internal HTTP RPC URL for container-to-container communication.
    pub fn internal_http_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_http_url(self.http_port)
    }

    /// Get the internal WebSocket RPC URL for container-to-container communication.
    pub fn internal_ws_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_ws_url(self.ws_port)
    }

    /// Get the internal authenticated RPC URL for Engine API.
    pub fn internal_authrpc_url(&self) -> anyhow::Result<Url> {
        self.ports.internal_authrpc_url(self.authrpc_port)
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

        // For sequencer nodes, point to self. For validators, point to the sequencer.
        let sequencer_http = sequencer_rpc
            .map(|url| url.to_string())
            .unwrap_or_else(|| format!("http://{}:{}", self.container_name, self.http_port));

        let cmd = OpRethCmdBuilder::new(
            container_config_path.join("genesis.json"),
            container_config_path.join(format!("reth-data-{}", self.container_name)),
        )
        .http_port(self.http_port)
        .ws_port(self.ws_port)
        .authrpc_port(self.authrpc_port)
        .authrpc_jwtsecret(container_config_path.join(jwt_filename))
        .metrics("0.0.0.0", self.metrics_port)
        .discovery(true)
        .discovery_port(self.discovery_port)
        .sequencer_http(sequencer_http)
        .bootnodes(bootnodes.to_vec())
        .extra_args(self.extra_args.clone())
        .net_if(self.net_if.clone())
        .listen_port(self.listen_port)
        .nat_dns(self.container_name.clone())
        .p2p_secret_key(&p2p_keypair.private_key)
        .build();

        // Extract port values for PortMapping from host_ports
        let (http, ws, authrpc, metrics, listen, discovery) = self
            .host_ports
            .as_ref()
            .map(|hp| {
                (
                    hp.http,
                    hp.ws,
                    hp.authrpc,
                    hp.metrics,
                    hp.listen,
                    hp.discovery,
                )
            })
            .unwrap_or((None, None, None, None, None, None));

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(self.http_port, http),
            PortMapping::tcp_optional(self.ws_port, ws),
            PortMapping::tcp_optional(self.authrpc_port, authrpc),
            PortMapping::tcp_optional(self.metrics_port, metrics),
            // P2P listen port (TCP for devp2p)
            PortMapping::tcp_optional(self.listen_port, listen),
            // Discovery port (UDP for discv5)
            PortMapping::udp_optional(self.discovery_port, discovery),
        ]
        .into_iter()
        .flatten()
        .collect();

        let exposed_ports: Vec<ExposedPort> = [
            ExposedPort::tcp(self.authrpc_port),
            ExposedPort::tcp(self.metrics_port),
            ExposedPort::tcp(self.listen_port),
            ExposedPort::udp(self.discovery_port),
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

        // Convert HashMap bound_ports to OpRethHostPorts
        let bound_host_ports = OpRethHostPorts {
            http: service_handler.ports.get_tcp_host_port(self.http_port),
            ws: service_handler.ports.get_tcp_host_port(self.ws_port),
            authrpc: service_handler.ports.get_tcp_host_port(self.authrpc_port),
            discovery: service_handler.ports.get_udp_host_port(self.discovery_port),
            listen: service_handler.ports.get_tcp_host_port(self.listen_port),
            metrics: service_handler.ports.get_tcp_host_port(self.metrics_port),
        };

        // Create typed ContainerPorts
        let typed_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => OpRethContainerPorts::Host {
                bound_ports: bound_host_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => OpRethContainerPorts::Bridge {
                container_name: container_name.clone(),
                bound_ports: bound_host_ports,
            },
        };

        let http_host_url = typed_ports.host_http_url();

        tracing::info!(
            container_id = %service_handler.container_id,
            container_name = %service_handler.container_name,
            ?http_host_url,
            "op-reth container started"
        );

        Ok(OpRethHandler {
            container_id: service_handler.container_id,
            container_name: service_handler.container_name,
            listen_port: self.listen_port,
            discovery_port: self.discovery_port,
            http_port: self.http_port,
            ws_port: self.ws_port,
            authrpc_port: self.authrpc_port,
            p2p_keypair,
            ports: typed_ports,
        })
    }
}
