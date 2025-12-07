//! op-proposer service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpProposerCmdBuilder;

use crate::docker::{
    CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping, ServiceConfig,
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
    /// Port for the op-proposer RPC server.
    pub rpc_port: u16,
    /// Port for metrics.
    pub metrics_port: u16,
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

        // The proposer account is at index 8 in the Anvil accounts
        let proposer_private_key = &anvil_handler.account_infos[8].private_key;

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

        self.docker_image.pull(docker).await?;

        let service_config = ServiceConfig::new(self.docker_image.clone())
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
