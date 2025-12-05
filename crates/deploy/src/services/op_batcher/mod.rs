//! op-batcher service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpBatcherCmdBuilder;

use crate::docker::{
    CreateAndStartContainerOptions, DockerImageBuilder, KupDocker, PortMapping, ServiceConfig,
};

use super::{anvil::AnvilHandler, kona_node::KonaNodeHandler, op_reth::OpRethHandler};

/// Default ports for op-batcher.
pub const DEFAULT_RPC_PORT: u16 = 8548;
pub const DEFAULT_METRICS_PORT: u16 = 7301;

/// Configuration for the op-batcher component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpBatcherConfig {
    /// Docker image configuration for op-batcher.
    pub docker_image: DockerImageBuilder,
    /// Container name for op-batcher.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Port for the op-batcher RPC server.
    pub rpc_port: u16,
    /// Port for metrics.
    pub metrics_port: u16,
    /// Max L1 tx size in bytes (default 120000).
    pub max_l1_tx_size_bytes: u64,
    /// Target number of frames per channel.
    pub target_num_frames: u64,
    /// Sub-safety margin (number of L1 blocks).
    pub sub_safety_margin: u64,
    /// Batch submission interval.
    pub poll_interval: String,
    /// Extra arguments to pass to op-batcher.
    pub extra_args: Vec<String>,
}

/// Default Docker image for op-batcher.
pub const DEFAULT_DOCKER_IMAGE: &str = "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-batcher";
/// Default Docker tag for op-batcher.
pub const DEFAULT_DOCKER_TAG: &str = "v1.16.2";

impl Default for OpBatcherConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImageBuilder::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-batcher".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
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
    /// The RPC URL for the op-batcher.
    pub rpc_url: Url,
}

impl OpBatcherConfig {
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

        // The batcher account is at index 7 in the Anvil accounts
        let batcher_private_key = &anvil_handler.account_infos[7].private_key;

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

        let image = self.docker_image.build(docker).await?;

        let service_config = ServiceConfig::new(image)
            .cmd(cmd)
            .ports([
                PortMapping::tcp_same(self.rpc_port),
                PortMapping::tcp_same(self.metrics_port),
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
            .context("Failed to start op-batcher container")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            "op-batcher container started"
        );

        let rpc_url = KupDocker::build_http_url(&handler.container_name, self.rpc_port)?;

        Ok(OpBatcherHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            rpc_url,
        })
    }
}
