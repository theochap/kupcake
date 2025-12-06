//! op-challenger service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpChallengerCmdBuilder;

use crate::docker::{
    CreateAndStartContainerOptions, DockerImageBuilder, KupDocker, PortMapping, ServiceConfig,
};

use super::{anvil::AnvilHandler, kona_node::KonaNodeHandler, op_reth::OpRethBuilder};

/// Default ports for op-challenger.
pub const DEFAULT_RPC_PORT: u16 = 8561;
pub const DEFAULT_METRICS_PORT: u16 = 7303;

/// Configuration for the op-challenger component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpChallengerBuilder {
    /// Docker image configuration for op-challenger.
    pub docker_image: DockerImageBuilder,
    /// Container name for op-challenger.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Port for the op-challenger RPC server.
    pub rpc_port: u16,
    /// Port for metrics.
    pub metrics_port: u16,
    /// Extra arguments to pass to op-challenger.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

/// Default Docker image for op-challenger.
pub const DEFAULT_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-challenger";
/// Default Docker tag for op-challenger.
pub const DEFAULT_DOCKER_TAG: &str = "develop";

impl Default for OpChallengerBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImageBuilder::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-challenger".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            metrics_port: DEFAULT_METRICS_PORT,
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running op-challenger instance.
pub struct OpChallengerHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// The RPC URL for the op-challenger.
    pub rpc_url: Url,
}

impl OpChallengerBuilder {
    /// Start the op-challenger.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        kona_node_handler: &KonaNodeHandler,
        op_reth_config: &OpRethBuilder,
    ) -> Result<OpChallengerHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        // The challenger account is at index 9 in the Anvil accounts
        let challenger_private_key = &anvil_handler.account_infos[9].private_key;

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

        let cmd = OpChallengerCmdBuilder::new(
            anvil_handler.l1_rpc_url.to_string(),
            format!(
                "http://{}:{}",
                op_reth_config.container_name, op_reth_config.http_port
            ),
            kona_node_handler.rpc_url.to_string(),
            challenger_private_key.to_string(),
            dgf_address,
        )
        .trace_type("permissioned")
        .game_allowlist([254]) // Permissioned game type
        .rpc_port(self.rpc_port)
        .metrics(true, "0.0.0.0", self.metrics_port)
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
            .context("Failed to start op-challenger container")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            "op-challenger container started"
        );

        let rpc_url = KupDocker::build_http_url(&handler.container_name, self.rpc_port)?;

        Ok(OpChallengerHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            rpc_url,
        })
    }
}
