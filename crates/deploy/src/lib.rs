//! kupcake-deploy - Deployment library for the OP Stack.
//!
//! This crate provides the deployment functionality for bootstrapping a rust-based
//! OP Stack chain.

use alloy_core::primitives::Bytes;

mod builder;
pub use builder::{DeployerBuilder, OutDataPath};

mod deployer;
pub use deployer::Deployer;

mod docker;
mod fs;
pub mod services;

pub use docker::{
    DockerImage, DockerImageBuilder, KupDocker, KupDockerConfig, PortMapping, PortProtocol,
    ServiceConfig, ServiceHandler,
};
pub use services::{
    AnvilConfig, AnvilHandler, GrafanaConfig, KonaNodeBuilder, KonaNodeHandler, MetricsTarget,
    MonitoringConfig, OpBatcherBuilder, OpBatcherHandler, OpChallengerBuilder, OpChallengerHandler,
    OpDeployerConfig, OpProposerBuilder, OpProposerHandler, OpRethBuilder, OpRethHandler,
    PrometheusConfig,
};

mod l2_stack;
pub use l2_stack::L2StackBuilder;

/// Account information from Anvil.
pub struct AccountInfo {
    pub address: Bytes,
    pub private_key: Bytes,
}
