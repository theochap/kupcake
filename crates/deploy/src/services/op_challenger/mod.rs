//! op-challenger service.

mod cmd;

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::OpChallengerCmdBuilder;

use crate::docker::{
    ContainerPorts, CreateAndStartContainerOptions, DockerImage, KupDocker, PortMapping,
    ServiceConfig,
};

use super::{anvil::AnvilHandler, kona_node::KonaNodeHandler, op_reth::OpRethBuilder};

/// Default port for op-challenger (for internal URL reference only - op-challenger has no RPC server).
pub const DEFAULT_RPC_PORT: u16 = 8561;
/// Default metrics port for op-challenger.
pub const DEFAULT_METRICS_PORT: u16 = 7303;

/// Host port configuration for op-challenger (used in Bridge mode).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpChallengerHostPorts {
    /// Host port for metrics endpoint.
    pub metrics: Option<u16>,
}

impl Default for OpChallengerHostPorts {
    fn default() -> Self {
        Self {
            metrics: Some(0), // Let OS pick an available port
        }
    }
}

/// Runtime port information for op-challenger containers.
pub enum OpChallengerContainerPorts {
    /// Host network mode - all communication via localhost with dynamically assigned ports.
    Host {
        /// Bound host ports for this container.
        bound_ports: OpChallengerHostPorts,
    },
    /// Bridge network mode - internal communication via container name, host access via mapped ports.
    Bridge {
        /// Container name for internal Docker network URLs.
        container_name: String,
        /// Bound host ports for this container (for host access).
        bound_ports: OpChallengerHostPorts,
    },
}

impl OpChallengerContainerPorts {
    /// Get the HTTP URL for host access to metrics.
    ///
    /// Returns None if the port is not published to the host.
    pub fn host_metrics_url(&self) -> Option<anyhow::Result<Url>> {
        match self {
            Self::Host { bound_ports } | Self::Bridge { bound_ports, .. } => {
                bound_ports.metrics.map(|port| {
                    Url::parse(&format!("http://localhost:{}/", port))
                        .context("Failed to parse HTTP URL")
                })
            }
        }
    }
}

/// Default Docker image for op-challenger.
pub const DEFAULT_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-challenger";
/// Default Docker tag for op-challenger.
pub const DEFAULT_DOCKER_TAG: &str = "develop";

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
    /// Host ports configuration. Only populated in Bridge mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ports: Option<OpChallengerHostPorts>,
    /// Extra arguments to pass to op-challenger.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

impl Default for OpChallengerBuilder {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-challenger".to_string(),
            host: "0.0.0.0".to_string(),
            metrics_port: DEFAULT_METRICS_PORT,
            host_ports: Some(OpChallengerHostPorts::default()),
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
    /// Metrics port (container port).
    pub metrics_port: u16,
    /// Port information for this container.
    pub ports: OpChallengerContainerPorts,
}

impl OpChallengerHandler {
    /// Get the host-accessible metrics URL (if published).
    pub fn host_metrics_url(&self) -> Option<anyhow::Result<Url>> {
        self.ports.host_metrics_url()
    }
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

        let challenger_private_key = &anvil_handler.accounts.challenger.private_key;

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
            anvil_handler.internal_rpc_url()?.to_string(),
            format!(
                "http://{}:{}",
                op_reth_config.container_name, op_reth_config.http_port
            ),
            kona_node_handler.internal_rpc_url()?.to_string(),
            challenger_private_key.to_string(),
            dgf_address,
        )
        .trace_type("permissioned")
        .game_allowlist([254]) // Permissioned game type
        .metrics(true, "0.0.0.0", self.metrics_port)
        .extra_args(self.extra_args.clone())
        .build();

        self.docker_image.pull(docker).await?;

        // Extract port value for PortMapping from host_ports
        let metrics = self
            .host_ports
            .as_ref()
            .and_then(|hp| hp.metrics);

        // Build port mappings only for ports that should be published to host
        // op-challenger doesn't have an RPC server, only metrics
        let port_mappings: Vec<PortMapping> =
            [PortMapping::tcp_optional(self.metrics_port, metrics)]
                .into_iter()
                .flatten()
                .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .ports(port_mappings)
            .bind(host_config_path, &container_config_path, "rw");

        let service_handler = docker
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

        // Convert HashMap bound_ports to OpChallengerHostPorts
        let bound_host_ports = OpChallengerHostPorts {
            metrics: service_handler.ports.get_tcp_host_port(self.metrics_port),
        };

        // Create typed ContainerPorts
        let typed_ports = match &service_handler.ports {
            ContainerPorts::Host { .. } => OpChallengerContainerPorts::Host {
                bound_ports: bound_host_ports,
            },
            ContainerPorts::Bridge { container_name, .. } => OpChallengerContainerPorts::Bridge {
                container_name: container_name.clone(),
                bound_ports: bound_host_ports,
            },
        };

        let metrics_host_url = typed_ports.host_metrics_url();

        tracing::info!(
            container_id = %service_handler.container_id,
            container_name = %service_handler.container_name,
            ?metrics_host_url,
            "op-challenger container started"
        );

        Ok(OpChallengerHandler {
            container_id: service_handler.container_id,
            container_name: service_handler.container_name,
            metrics_port: self.metrics_port,
            ports: typed_ports,
        })
    }
}
