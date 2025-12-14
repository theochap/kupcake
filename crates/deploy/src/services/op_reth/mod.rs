//! op-reth execution client service.

mod cmd;

use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpRethCmdBuilder;

use crate::{
    ExposedPort,
    docker::{CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping, ServiceConfig},
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
    /// Extra arguments to pass to op-reth.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

/// Default Docker image for op-reth.
pub const DEFAULT_DOCKER_IMAGE: &str = "op-reth";
/// Default Docker tag for op-reth.
pub const DEFAULT_DOCKER_TAG: &str = "nightly";

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
    /// The P2P listen port (used for enode URL construction).
    pub listen_port: u16,
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

/// Foundry Docker image used for cast commands.
const FOUNDRY_IMAGE: &str = "ghcr.io/foundry-rs/foundry";
const FOUNDRY_TAG: &str = "latest";

/// Response from `admin_nodeInfo` RPC endpoint.
#[derive(Debug, Deserialize)]
struct AdminNodeInfo {
    /// The enode (Ethereum Node Record) for the node.
    enode: Option<String>,
}

impl OpRethHandler {
    /// Query the node's enode from the `admin_nodeInfo` RPC endpoint.
    ///
    /// This uses cast inside a Docker container connected to the same network
    /// to query the internal RPC URL.
    ///
    /// Retries with exponential backoff since the node may not be ready
    /// to respond immediately after starting.
    pub async fn fetch_enode(&self, docker: &mut KupDocker) -> Result<String, anyhow::Error> {
        let rpc_url = self.http_rpc_url.to_string();

        // Retry with exponential backoff
        let max_retries = 10;
        let mut delay = Duration::from_millis(500);
        let max_delay = Duration::from_secs(5);

        for attempt in 1..=max_retries {
            let config = ServiceConfig::new(DockerImage::new(FOUNDRY_IMAGE, FOUNDRY_TAG))
                .cmd(vec![
                    "rpc".to_string(),
                    "admin_nodeInfo".to_string(),
                    "--rpc-url".to_string(),
                    rpc_url.clone(),
                ])
                .entrypoint(vec!["cast".to_string()])
                .env(vec!["FOUNDRY_DISABLE_NIGHTLY_WARNING=1".to_string()]);

            match docker.run_command(config).await {
                Ok(output) => {
                    let node_info: AdminNodeInfo = serde_json::from_str(&output)
                        .context("Failed to parse admin_nodeInfo response")?;

                    if let Some(enode) = node_info.enode {
                        return Ok(enode);
                    }
                    return Err(anyhow::anyhow!("Node returned empty enode"));
                }
                Err(e) => {
                    if attempt == max_retries {
                        return Err(e).context("Failed to fetch enode after max retries");
                    }
                    tracing::debug!(
                        attempt,
                        max_retries,
                        error = %e,
                        delay_ms = delay.as_millis(),
                        "Failed to fetch op-reth enode, retrying..."
                    );
                    tokio::time::sleep(delay).await;
                    delay = std::cmp::min(delay * 2, max_delay);
                }
            }
        }

        unreachable!()
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
    /// * `bootnodes` - List of ENR strings for P2P peer discovery
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        sequencer_rpc: Option<&Url>,
        jwt_filename: &str,
        bootnodes: &[String],
    ) -> Result<OpRethHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

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
        .nat_dns(format!("{}:0", self.container_name.clone()))
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
            http_rpc_url,
            ws_rpc_url,
            authrpc_url,
            http_host_url,
            ws_host_url,
        })
    }
}
