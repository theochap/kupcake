//! op-conductor service for sequencer consensus.
//!
//! The op-conductor manages multiple sequencer nodes using Raft consensus
//! to provide high availability for sequencing.

mod cmd;

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpConductorCmdBuilder;

use crate::docker::{DockerImage, ExposedPort, KupDocker, PortMapping, ServiceConfig};
use crate::metrics::ContainerDeployTimings;
use crate::service::{self, KupcakeService};

/// Input parameters for deploying op-conductor.
pub struct OpConductorInput {
    /// Unique ID for this sequencer in the Raft cluster.
    pub server_id: String,
    /// The execution client RPC URL (e.g., op-reth HTTP).
    pub execution_rpc_url: String,
    /// The kona-node RPC URL for this sequencer.
    pub kona_node_rpc_url: String,
    /// Whether to bootstrap the Raft cluster (true for leader, false for follower).
    pub bootstrap: bool,
}

/// Default ports for op-conductor.
pub const DEFAULT_RPC_PORT: u16 = 8547;
pub const DEFAULT_CONSENSUS_PORT: u16 = 50050;

/// Default Docker image for op-conductor.
pub const DEFAULT_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-conductor";
/// Default Docker tag for op-conductor.
pub const DEFAULT_DOCKER_TAG: &str = "v0.9.0";

/// Information about a sequencer node that the conductor manages.
#[derive(Debug, Clone)]
pub struct SequencerInfo {
    /// Unique ID for this sequencer in the Raft cluster.
    pub server_id: String,
    /// The kona-node RPC URL for this sequencer.
    pub node_rpc: Url,
    /// The op-reth RPC URL for this sequencer.
    pub execution_rpc: Url,
}

/// Configuration for the op-conductor component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpConductorBuilder {
    /// Docker image configuration for op-conductor.
    pub docker_image: DockerImage,
    /// Container name for op-conductor.
    pub container_name: String,
    /// Host for the RPC endpoint.
    pub host: String,
    /// Port for the op-conductor RPC server (container port).
    pub rpc_port: u16,
    /// Port for Raft consensus (container port).
    pub consensus_port: u16,
    /// Host port for RPC. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc_host_port: Option<u16>,
    /// Host port for consensus. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consensus_host_port: Option<u16>,
    /// Health check interval.
    pub healthcheck_interval: String,
    /// Unsafe interval - interval allowed between unsafe head and now measured in seconds.
    pub healthcheck_unsafe_interval: String,
    /// Minimum number of peers required to be considered healthy.
    pub healthcheck_min_peer_count: String,
    /// Log level for op-conductor (e.g., "INFO", "DEBUG").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    /// Extra arguments to pass to op-conductor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

impl Default for OpConductorBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-conductor".to_string(),
            host: "0.0.0.0".to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            consensus_port: DEFAULT_CONSENSUS_PORT,
            rpc_host_port: Some(0),
            consensus_host_port: None,
            healthcheck_interval: "5".to_string(),
            healthcheck_unsafe_interval: "600".to_string(),
            healthcheck_min_peer_count: "1".to_string(),
            log_level: None,
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running op-conductor instance.
pub struct OpConductorHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// The RPC URL for the op-conductor (internal Docker network).
    pub rpc_url: Url,
    /// The RPC URL accessible from host (if published). None if not published.
    pub rpc_host_url: Option<Url>,
    /// Deploy timings for metrics.
    pub deploy_timings: ContainerDeployTimings,
}

impl OpConductorBuilder {
    /// Build the Docker command arguments for op-conductor.
    pub fn build_cmd(
        &self,
        _host_config_path: &Path,
        input: &OpConductorInput,
    ) -> Result<Vec<String>, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");
        let raft_storage_dir = container_config_path.join("raft");
        let rollup_config_path = container_config_path.join("rollup.json");

        Ok(OpConductorCmdBuilder::new(
            input.kona_node_rpc_url.clone(),
            input.execution_rpc_url.clone(),
            input.server_id.clone(),
            raft_storage_dir.display().to_string(),
            rollup_config_path.display().to_string(),
        )
        .raft_bootstrap(input.bootstrap)
        .rpc_addr(&self.host)
        .rpc_port(self.rpc_port)
        .consensus_addr(&self.container_name)
        .consensus_port(self.consensus_port)
        .healthcheck_interval(&self.healthcheck_interval)
        .healthcheck_unsafe_interval(&self.healthcheck_unsafe_interval)
        .healthcheck_min_peer_count(&self.healthcheck_min_peer_count)
        .log_level(self.log_level.as_deref().unwrap_or("DEBUG"))
        .extra_args(self.extra_args.clone())
        .build())
    }
}

impl KupcakeService for OpConductorBuilder {
    type Input = OpConductorInput;
    type Output = OpConductorHandler;

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
        input: OpConductorInput,
    ) -> Result<OpConductorHandler, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        let cmd = self.build_cmd(host_config_path, &input)?;

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> = [
            PortMapping::tcp_optional(self.rpc_port, self.rpc_host_port),
            PortMapping::tcp_optional(self.consensus_port, self.consensus_host_port),
        ]
        .into_iter()
        .flatten()
        .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .expose(ExposedPort::tcp(self.rpc_port))
            .expose(ExposedPort::tcp(self.consensus_port))
            .bind(host_config_path, &container_config_path, "rw");

        let (handler, timings) = service::deploy_container(
            docker,
            &self.docker_image,
            &self.container_name,
            service_config,
        )
        .await
        .context("Failed to start op-conductor container")?;

        // Build internal Docker network URL
        let rpc_url = KupDocker::build_http_url(&handler.container_name, self.rpc_port)?;

        // Build host-accessible URLs from bound ports
        let rpc_host_url = handler.build_host_url(self.rpc_port, "http")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            server_id = %input.server_id,
            bootstrap = input.bootstrap,
            ?rpc_host_url,
            "op-conductor container started"
        );

        Ok(OpConductorHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            rpc_url,
            rpc_host_url,
            deploy_timings: timings,
        })
    }
}
