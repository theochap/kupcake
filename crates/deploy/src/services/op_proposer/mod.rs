//! op-proposer service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpProposerCmdBuilder;

use crate::docker::{
    CreateAndStartContainerOptions, DockerImage, ExposedPort, KupDocker, PortMapping,
    ServiceConfig,
};

use super::{anvil::AnvilHandler, kona_node::KonaNodeHandler};

/// Default ports for op-proposer.
pub const DEFAULT_RPC_PORT: u16 = 8560;
pub const DEFAULT_METRICS_PORT: u16 = 7302;

/// Configuration for the op-proposer component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpProposerBuilder {
    /// Docker image configuration for op-proposer.
    pub docker_image: DockerImage,
    /// Container name for op-proposer.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Port for the op-proposer RPC server (container port).
    pub rpc_port: u16,
    /// Port for metrics (container port).
    pub metrics_port: u16,
    /// Host port for RPC. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_host_port: Option<u16>,
    /// Host port for metrics. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_host_port: Option<u16>,
    /// Proposal interval.
    pub proposal_interval: String,
    /// Extra arguments to pass to op-proposer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

/// Default Docker image for op-proposer.
pub const DEFAULT_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-proposer";
/// Default Docker tag for op-proposer.
pub const DEFAULT_DOCKER_TAG: &str = "develop";

impl Default for OpProposerBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-proposer".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            rpc_host_port: None,
            metrics_host_port: None,
            proposal_interval: "12s".to_string(),
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running op-proposer instance.
pub struct OpProposerHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// The RPC URL for the op-proposer.
    pub rpc_url: Url,
}

impl OpProposerBuilder {
    /// Start the op-proposer.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        kona_node_handler: &KonaNodeHandler,
    ) -> Result<OpProposerHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // Ensure the Docker image is ready (pull or build if needed)
        docker
            .ensure_image_ready(&self.docker_image, "op-proposer")
            .await
            .context("Failed to ensure op-proposer image is ready")?;

        let proposer_private_key = &anvil_handler.accounts.proposer.private_key;

        // Read the DisputeGameFactory address from state.json
        let state_file_path = host_config_path.join("state.json");
        let state_content = tokio::fs::read_to_string(&state_file_path)
            .await
            .context("Failed to read state.json for DisputeGameFactory address")?;

        let state: serde_json::Value =
            serde_json::from_str(&state_content).context("Failed to parse state.json")?;

        let dgf_address = state["opChainDeployments"][0]["DisputeGameFactoryProxy"]
            .as_str()
            .context("DisputeGameFactory address not found in state.json")?;

        let cmd = OpProposerCmdBuilder::new(
            anvil_handler.l1_rpc_url.to_string(),
            kona_node_handler.rpc_url.to_string(),
            proposer_private_key.to_string(),
            dgf_address,
        )
        .game_type(254) // Permissioned game type
        .proposal_interval(&self.proposal_interval)
        .rpc_port(self.rpc_port)
        .metrics(true, "0.0.0.0", self.metrics_port)
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
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    ..Default::default()
                },
            )
            .await
            .context("Failed to start op-proposer container")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            "op-proposer container started"
        );

        let rpc_url = KupDocker::build_http_url(&handler.container_name, self.rpc_port)?;

        Ok(OpProposerHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            rpc_url,
        })
    }
}
