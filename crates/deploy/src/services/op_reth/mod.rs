//! op-reth execution client service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpRethCmdBuilder;

use crate::{
    ExposedPort,
    docker::{CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping, ServiceConfig},
    services::kona_node::P2pKeypair,
};

/// Default ports for op-reth.
pub const DEFAULT_HTTP_PORT: u16 = 9545;
pub const DEFAULT_WS_PORT: u16 = 9546;
pub const DEFAULT_AUTHRPC_PORT: u16 = 9551;
pub const DEFAULT_DISCOVERY_PORT: u16 = 30303;
pub const DEFAULT_LISTEN_PORT: u16 = 30303;
pub const DEFAULT_METRICS_PORT: u16 = 9001;

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
    /// Host port for HTTP JSON-RPC. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_host_port: Option<u16>,
    /// Host port for WebSocket JSON-RPC. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ws_host_port: Option<u16>,
    /// Host port for authenticated Engine API. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authrpc_host_port: Option<u16>,
    /// Host port for P2P discovery. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_host_port: Option<u16>,
    /// Host port for metrics. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_host_port: Option<u16>,
    /// Port for listen. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen_host_port: Option<u16>,
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
            // Default: publish HTTP and WS to host (port 0 = OS picks), others internal only
            http_host_port: Some(0),
            ws_host_port: Some(0),
            authrpc_host_port: None,
            metrics_host_port: None,
            listen_host_port: None,
            discovery_host_port: None,
            net_if: None,
            p2p_secret_key: None,
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running op-reth instance.
#[derive(Clone)]
pub struct OpRethHandler {
    /// Port for P2P discovery (container port).
    pub discovery_port: u16,
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// The P2P listen port (used for enode URL construction).
    pub listen_port: u16,
    /// P2P keypair for this node (used for enode computation).
    pub p2p_keypair: P2pKeypair,
    /// The HTTP RPC URL for the L2 execution client (internal Docker network).
    pub http_rpc_url: Url,
    /// The WebSocket RPC URL for the L2 execution client (internal Docker network).
    pub ws_rpc_url: Url,
    /// The authenticated RPC URL for Engine API (internal Docker network, used by kona-node).
    pub authrpc_url: Url,
    /// The HTTP RPC URL accessible from host (if published). None if not published.
    pub http_host_url: Option<Url>,
    /// The WebSocket RPC URL accessible from host (if published). None if not published.
    pub ws_host_url: Option<Url>,
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

        // Ensure the Docker image is ready (pull or build if needed)
        docker
            .ensure_image_ready(&self.docker_image, "op-reth")
            .await
            .context("Failed to ensure op-reth image is ready")?;

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

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(self.http_port, self.http_host_port),
            PortMapping::tcp_optional(self.ws_port, self.ws_host_port),
            PortMapping::tcp_optional(self.authrpc_port, self.authrpc_host_port),
            PortMapping::tcp_optional(self.metrics_port, self.metrics_host_port),
            // P2P listen port (TCP for devp2p)
            PortMapping::tcp_optional(self.listen_port, self.listen_host_port),
            // Discovery port (UDP for discv5)
            PortMapping::udp_optional(self.discovery_port, self.discovery_host_port),
        ]
        .into_iter()
        .flatten()
        .collect();

        let exposed_ports: Vec<ExposedPort> = [
            ExposedPort::tcp(self.http_port),
            ExposedPort::tcp(self.ws_port),
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

        let handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions {
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-reth container")?;

        // Build internal Docker network URLs
        let http_rpc_url = KupDocker::build_http_url(&handler.container_name, self.http_port)?;
        let ws_rpc_url = KupDocker::build_ws_url(&handler.container_name, self.ws_port)?;
        let authrpc_url = KupDocker::build_http_url(&handler.container_name, self.authrpc_port)?;

        // Build host-accessible URLs from bound ports
        let http_host_url = handler
            .get_tcp_host_port(self.http_port)
            .map(|port| Url::parse(&format!("http://localhost:{}/", port)))
            .transpose()
            .context("Failed to build HTTP host URL")?;

        let ws_host_url = handler
            .get_tcp_host_port(self.ws_port)
            .map(|port| Url::parse(&format!("ws://localhost:{}/", port)))
            .transpose()
            .context("Failed to build WebSocket host URL")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            ?http_host_url,
            ?ws_host_url,
            "op-reth container started"
        );

        Ok(OpRethHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            listen_port: self.listen_port,
            discovery_port: self.discovery_port,
            p2p_keypair,
            http_rpc_url,
            ws_rpc_url,
            authrpc_url,
            http_host_url,
            ws_host_url,
        })
    }
}
