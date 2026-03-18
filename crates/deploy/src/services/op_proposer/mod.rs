//! op-proposer service.

mod cmd;

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpProposerCmdBuilder;

use crate::docker::{DockerImage, ExposedPort, KupDocker, PortMapping, ServiceConfig};
use crate::metrics::ContainerDeployTimings;
use crate::service::{self, KupcakeService};

/// Input parameters for deploying the op-proposer.
pub struct OpProposerInput {
    /// L1 RPC URL (e.g., Anvil).
    pub l1_rpc_url: String,
    /// Rollup (consensus) RPC URL (e.g., kona-node).
    pub rollup_rpc_url: String,
    /// Private key for the proposer account.
    pub proposer_private_key: String,
}

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
    /// Log level for op-proposer (e.g., "INFO", "DEBUG").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
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
            log_level: None,
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
    /// Deploy timings for metrics.
    pub deploy_timings: ContainerDeployTimings,
}

impl OpProposerBuilder {
    /// Build the Docker command arguments for op-proposer.
    pub fn build_cmd(
        &self,
        host_config_path: &Path,
        input: &OpProposerInput,
    ) -> Result<Vec<String>, anyhow::Error> {
        let dgf_address = super::read_dgf_address(host_config_path)?;

        let mut cmd_builder = OpProposerCmdBuilder::new(
            input.l1_rpc_url.to_string(),
            input.rollup_rpc_url.to_string(),
            input.proposer_private_key.to_string(),
            dgf_address,
        )
        .game_type(254) // Permissioned game type
        .proposal_interval(&self.proposal_interval)
        .rpc_port(self.rpc_port)
        .metrics(true, "0.0.0.0", self.metrics_port)
        .extra_args(self.extra_args.clone());

        if let Some(ref level) = self.log_level {
            cmd_builder = cmd_builder.log_level(level);
        }

        Ok(cmd_builder.build())
    }
}

impl KupcakeService for OpProposerBuilder {
    type Input = OpProposerInput;
    type Output = OpProposerHandler;

    fn container_name(&self) -> &str {
        &self.container_name
    }

    fn docker_image(&self) -> &DockerImage {
        &self.docker_image
    }

    async fn deploy<'a>(
        &'a self,
        docker: &'a mut KupDocker,
        host_config_path: &'a Path,
        input: OpProposerInput,
    ) -> Result<OpProposerHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        let cmd = self.build_cmd(host_config_path, &input)?;

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
            .expose(ExposedPort::tcp(self.rpc_port))
            .expose(ExposedPort::tcp(self.metrics_port))
            .bind(host_config_path, &container_config_path, "rw");

        let (handler, timings) = service::deploy_container(
            docker,
            &self.docker_image,
            &self.container_name,
            service_config,
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
            deploy_timings: timings,
        })
    }
}
