//! op-challenger service.

mod cmd;

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpChallengerCmdBuilder;

use crate::docker::{DockerImage, ExposedPort, KupDocker, PortMapping, ServiceConfig};
use crate::metrics::ContainerDeployTimings;
use crate::service::{self, KupcakeService};

/// Input parameters for deploying the op-challenger.
pub struct OpChallengerInput {
    /// L1 RPC URL (e.g., Anvil).
    pub l1_rpc_url: String,
    /// L2 execution client RPC URL (e.g., op-reth HTTP).
    pub l2_rpc_url: String,
    /// Rollup (consensus) RPC URL (e.g., kona-node).
    pub rollup_rpc_url: String,
    /// Private key for the challenger account.
    pub challenger_private_key: String,
}

/// Default port for op-challenger (for internal URL reference only - op-challenger has no RPC server).
pub const DEFAULT_RPC_PORT: u16 = 8561;
/// Default metrics port for op-challenger.
pub const DEFAULT_METRICS_PORT: u16 = 7303;

/// Configuration for the op-challenger component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpChallengerBuilder {
    /// Docker image configuration for op-challenger.
    pub docker_image: DockerImage,
    /// Container name for op-challenger.
    pub container_name: String,
    /// Host for the metrics endpoint.
    pub host: String,
    /// Port for metrics (container port).
    pub metrics_port: u16,
    /// Host port for metrics. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics_host_port: Option<u16>,
    /// Log level for op-challenger (e.g., "INFO", "DEBUG").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
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
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-challenger".to_string(),
            host: "0.0.0.0".to_string(),
            metrics_port: DEFAULT_METRICS_PORT,
            metrics_host_port: Some(0),
            log_level: None,
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
    /// The metrics URL for the op-challenger (op-challenger has no RPC server).
    pub metrics_url: Url,
    /// Deploy timings for metrics.
    pub deploy_timings: ContainerDeployTimings,
}

impl OpChallengerBuilder {
    /// Build the Docker command arguments for op-challenger.
    pub fn build_cmd(
        &self,
        host_config_path: &Path,
        input: &OpChallengerInput,
    ) -> Result<Vec<String>, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        let dgf_address = super::read_dgf_address(host_config_path)?;

        let mut cmd_builder = OpChallengerCmdBuilder::new(
            input.l1_rpc_url.to_string(),
            input.l2_rpc_url.to_string(),
            input.rollup_rpc_url.to_string(),
            input.challenger_private_key.to_string(),
            dgf_address,
            container_config_path.to_string_lossy(),
        )
        .rollup_config(
            container_config_path.join("rollup.json").to_string_lossy(),
            container_config_path.join("genesis.json").to_string_lossy(),
        )
        .trace_type("permissioned")
        .game_allowlist([254]) // Permissioned game type
        .metrics(true, "0.0.0.0", self.metrics_port)
        .extra_args(self.extra_args.clone());

        if let Some(ref level) = self.log_level {
            cmd_builder = cmd_builder.log_level(level);
        }

        Ok(cmd_builder.build())
    }
}

impl KupcakeService for OpChallengerBuilder {
    type Input = OpChallengerInput;
    type Output = OpChallengerHandler;

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
        input: OpChallengerInput,
    ) -> Result<OpChallengerHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        let cmd = self.build_cmd(host_config_path, &input)?;

        // op-challenger doesn't have an RPC server, only metrics
        let port_mappings: Vec<PortMapping> = [PortMapping::tcp_optional(
            self.metrics_port,
            self.metrics_host_port,
        )]
        .into_iter()
        .flatten()
        .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .expose(ExposedPort::tcp(self.metrics_port))
            .bind(host_config_path, &container_config_path, "rw");

        let (handler, timings) = service::deploy_container(
            docker,
            &self.docker_image,
            &self.container_name,
            service_config,
        )
        .await
        .context("Failed to start op-challenger container")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            "op-challenger container started"
        );

        // op-challenger doesn't have an RPC server, only metrics
        let metrics_url = KupDocker::build_http_url(&handler.container_name, self.metrics_port)?;

        Ok(OpChallengerHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            metrics_url,
            deploy_timings: timings,
        })
    }
}
