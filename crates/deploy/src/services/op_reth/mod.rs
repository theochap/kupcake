//! op-reth execution client service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpRethCmdBuilder;

use crate::docker::{
    CreateAndStartContainerOptions, DockerImageBuilder, KupDocker, PortMapping, ServiceConfig,
};

/// Default ports for op-reth.
pub const DEFAULT_HTTP_PORT: u16 = 9545;
pub const DEFAULT_WS_PORT: u16 = 9546;
pub const DEFAULT_AUTHRPC_PORT: u16 = 9551;
pub const DEFAULT_DISCOVERY_PORT: u16 = 30303;
pub const DEFAULT_METRICS_PORT: u16 = 9001;

/// Configuration for the op-reth execution client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpRethConfig {
    /// Docker image configuration for op-reth.
    pub docker_image: DockerImageBuilder,
    /// Container name for op-reth.
    pub container_name: String,
    /// Host for the HTTP RPC endpoint.
    pub host: String,
    /// Port for the HTTP JSON-RPC server.
    pub http_port: u16,
    /// Port for the WebSocket JSON-RPC server.
    pub ws_port: u16,
    /// Port for the authenticated Engine API (used by kona-node).
    pub authrpc_port: u16,
    /// Port for P2P discovery.
    pub discovery_port: u16,
    /// Port for metrics.
    pub metrics_port: u16,
    /// Extra arguments to pass to op-reth.
    pub extra_args: Vec<String>,
}

/// Default Docker image for op-reth.
pub const DEFAULT_DOCKER_IMAGE: &str = "ghcr.io/paradigmxyz/op-reth";
/// Default Docker tag for op-reth.
pub const DEFAULT_DOCKER_TAG: &str = "latest";

impl Default for OpRethConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImageBuilder::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-reth".to_string(),
            host: "0.0.0.0".to_string(),
            http_port: DEFAULT_HTTP_PORT,
            ws_port: DEFAULT_WS_PORT,
            authrpc_port: DEFAULT_AUTHRPC_PORT,
            discovery_port: DEFAULT_DISCOVERY_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
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
    /// The HTTP RPC URL for the L2 execution client.
    pub http_rpc_url: Url,
    /// The WebSocket RPC URL for the L2 execution client.
    pub ws_rpc_url: Url,
    /// The authenticated RPC URL for Engine API (used by kona-node).
    pub authrpc_url: Url,
}

impl OpRethConfig {
    /// Start the op-reth execution client.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
    ) -> Result<OpRethHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        let cmd = OpRethCmdBuilder::new(
            container_config_path.join("genesis.json"),
            container_config_path.join("reth-data"),
        )
        .http_port(self.http_port)
        .ws_port(self.ws_port)
        .authrpc_port(self.authrpc_port)
        .authrpc_jwtsecret(container_config_path.join("jwt.hex"))
        .metrics("0.0.0.0", self.metrics_port)
        .discovery(false)
        .sequencer_http(format!("http://{}:{}", self.container_name, self.http_port))
        .extra_args(self.extra_args.clone())
        .build();

        let image = self.docker_image.build(docker).await?;

        let service_config = ServiceConfig::new(image)
            .cmd(cmd)
            .ports([
                PortMapping::tcp_same(self.http_port),
                PortMapping::tcp_same(self.ws_port),
                PortMapping::tcp_same(self.authrpc_port),
                PortMapping::tcp_same(self.metrics_port),
                PortMapping::tcp_same(self.discovery_port),
                PortMapping::udp_same(self.discovery_port),
            ])
            .bind(host_config_path, &container_config_path, "rw");

        let handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-reth container")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            "op-reth container started"
        );

        let http_rpc_url = KupDocker::build_http_url(&handler.container_name, self.http_port)?;
        let ws_rpc_url = KupDocker::build_ws_url(&handler.container_name, self.ws_port)?;
        let authrpc_url = KupDocker::build_http_url(&handler.container_name, self.authrpc_port)?;

        Ok(OpRethHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            http_rpc_url,
            ws_rpc_url,
            authrpc_url,
        })
    }
}
