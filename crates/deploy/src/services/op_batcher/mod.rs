//! op-batcher service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpBatcherCmdBuilder;

use crate::docker::{
    CreateAndStartContainerOptions, DockerImage, ExposedPort, KupDocker, PortMapping,
    ServiceConfig,
};

use super::{anvil::AnvilHandler, kona_node::KonaNodeHandler, op_reth::OpRethHandler};

/// Default ports for op-batcher.
pub const DEFAULT_RPC_PORT: u16 = 8548;
pub const DEFAULT_METRICS_PORT: u16 = 7301;

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
    /// Host port for RPC. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_host_port: Option<u16>,
    /// Host port for metrics. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_host_port: Option<u16>,
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
            rpc_host_port: Some(0),
            metrics_host_port: None,
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
    /// The RPC URL for the op-batcher (internal Docker network).
    pub rpc_url: Url,
    /// The RPC URL accessible from host (if published). None if not published.
    pub rpc_host_url: Option<Url>,
    /// The metrics URL accessible from host (if published). None if not published.
    pub metrics_host_url: Option<Url>,
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

        // Ensure the Docker image is ready (pull or build if needed)
        docker
            .ensure_image_ready(&self.docker_image, "op-batcher")
            .await
            .context("Failed to ensure op-batcher image is ready")?;

        let batcher_private_key = &anvil_handler.accounts.batcher.private_key;

        let cmd = OpBatcherCmdBuilder::new(
            anvil_handler.l1_rpc_url.to_string(),
            op_reth_handler.http_rpc_url.to_string(),
            kona_node_handler.rpc_url.to_string(),
            batcher_private_key.to_string(),
        )
        .rpc_port(self.rpc_port)
        .metrics(true, "0.0.0.0", self.metrics_port)
        .data_availability_type("blobs")
        .extra_args(self.extra_args.clone())
        .build();

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(self.rpc_port, self.rpc_host_port),
            PortMapping::tcp_optional(self.metrics_port, self.metrics_host_port),
        ]
        .into_iter()
        .flatten()
        .collect();

        // Always expose all ports to the Docker network (regardless of publish_all_ports)
        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .expose(ExposedPort::tcp(self.rpc_port))
            .expose(ExposedPort::tcp(self.metrics_port))
            .bind(host_config_path, &container_config_path, "rw");

        let handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions::default(),
            )
            .await
            .context("Failed to start op-batcher container")?;

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
            "op-batcher container started"
        );

        Ok(OpBatcherHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            rpc_url,
            rpc_host_url,
            metrics_host_url,
        })
    }
}
