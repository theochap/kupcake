//! kona-node consensus client service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::KonaNodeCmdBuilder;

use crate::docker::{
    CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping, ServiceConfig,
};

use super::{anvil::AnvilHandler, op_reth::OpRethHandler};

/// Default ports for kona-node.
pub const DEFAULT_RPC_PORT: u16 = 7545;
pub const DEFAULT_METRICS_PORT: u16 = 7300;

/// Configuration for the kona-node consensus client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KonaNodeBuilder {
    /// Docker image configuration for kona-node.
    pub docker_image: DockerImage,
    /// Container name for kona-node.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Port for the kona-node RPC server (container port).
    pub rpc_port: u16,
    /// Port for metrics (container port).
    pub metrics_port: u16,
    /// Host port for RPC. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_host_port: Option<u16>,
    /// Host port for metrics. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_host_port: Option<u16>,
    /// L1 slot duration in seconds (block time).
    pub l1_slot_duration: u64,
    /// Extra arguments to pass to kona-node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

/// Default Docker image for kona-node.
pub const DEFAULT_DOCKER_IMAGE: &str = "kona-node";
/// Default Docker tag for kona-node.
pub const DEFAULT_DOCKER_TAG: &str = "local";

impl Default for KonaNodeBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-kona-node".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            rpc_host_port: Some(0),
            metrics_host_port: None,
            l1_slot_duration: 12,
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running kona-node instance.
pub struct KonaNodeHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// The RPC URL for the kona-node (internal Docker network).
    pub rpc_url: Url,
    /// The RPC URL accessible from host (if published). None if not published.
    pub rpc_host_url: Option<Url>,
    /// The metrics URL accessible from host (if published). None if not published.
    pub metrics_host_url: Option<Url>,
}

impl KonaNodeBuilder {
    /// Start the kona-node consensus client.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        op_reth_handler: &OpRethHandler,
    ) -> Result<KonaNodeHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        let cmd = KonaNodeCmdBuilder::new(
            anvil_handler.l1_rpc_url.to_string(),
            op_reth_handler.authrpc_url.to_string(),
            container_config_path.join("rollup.json"),
            container_config_path.join("jwt.hex"),
        )
        .mode("sequencer")
        .unsafe_block_signer_key(anvil_handler.account_infos[6].private_key.clone())
        .l1_slot_duration(self.l1_slot_duration)
        .rpc_port(self.rpc_port)
        .metrics(true, self.metrics_port)
        .discovery(false)
        .extra_args(self.extra_args.clone())
        .build();

        self.docker_image.pull(docker).await?;

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(self.rpc_port, self.rpc_host_port),
            PortMapping::tcp_optional(self.metrics_port, self.metrics_host_port),
        ]
        .into_iter()
        .flatten()
        .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
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
            .context("Failed to start kona-node container")?;

        // Build internal Docker network URL
        let rpc_url = KupDocker::build_http_url(&handler.container_name, self.rpc_port)?;

        // Build host-accessible URLs from bound ports
        let rpc_host_url = handler
            .get_tcp_host_port(self.rpc_port)
            .map(|port| Url::parse(&format!("http://localhost:{}/", port)))
            .transpose()
            .context("Failed to build RPC host URL")?;

        let metrics_host_url = handler
            .get_tcp_host_port(self.metrics_port)
            .map(|port| Url::parse(&format!("http://localhost:{}/", port)))
            .transpose()
            .context("Failed to build metrics host URL")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            ?rpc_host_url,
            ?metrics_host_url,
            "kona-node container started"
        );

        Ok(KonaNodeHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            rpc_url,
            rpc_host_url,
            metrics_host_url,
        })
    }
}
