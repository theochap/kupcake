//! kupcake-deploy - Deployment library for the OP Stack.
//!
//! This crate provides the deployment functionality for bootstrapping a rust-based
//! OP Stack chain.

use alloy_core::primitives::Bytes;

mod builder;
pub use builder::{DeployerBuilder, OutDataPath};

mod deployer;
pub use deployer::Deployer;

mod deployment_hash;
pub use deployment_hash::{DeploymentConfigHash, DeploymentVersion};

mod docker;
mod fs;
pub mod services;

pub use docker::{
    CleanupResult, DockerImage, ExposedPort, KupDocker, KupDockerConfig, PortMapping, PortProtocol,
    ServiceConfig, ServiceHandler, cleanup_by_prefix,
};
pub use services::{
    // Docker image defaults
    ANVIL_DEFAULT_IMAGE,
    ANVIL_DEFAULT_TAG,
    AnvilAccounts,
    AnvilConfig,
    AnvilHandler,
    GRAFANA_DEFAULT_IMAGE,
    GRAFANA_DEFAULT_TAG,
    GrafanaConfig,
    KONA_NODE_DEFAULT_IMAGE,
    KONA_NODE_DEFAULT_TAG,
    KonaNodeBuilder,
    KonaNodeHandler,
    // L2 Node types
    ConductorContext,
    L2NodeBuilder,
    L2NodeHandler,
    L2NodeRole,
    MetricsTarget,
    MonitoringConfig,
    OP_BATCHER_DEFAULT_IMAGE,
    OP_BATCHER_DEFAULT_TAG,
    OP_CHALLENGER_DEFAULT_IMAGE,
    OP_CHALLENGER_DEFAULT_TAG,
    OP_CONDUCTOR_DEFAULT_IMAGE,
    OP_CONDUCTOR_DEFAULT_TAG,
    OP_DEPLOYER_DEFAULT_IMAGE,
    OP_DEPLOYER_DEFAULT_TAG,
    OP_PROPOSER_DEFAULT_IMAGE,
    OP_PROPOSER_DEFAULT_TAG,
    OP_RETH_DEFAULT_IMAGE,
    OP_RETH_DEFAULT_TAG,
    OpBatcherBuilder,
    OpBatcherHandler,
    OpChallengerBuilder,
    OpChallengerHandler,
    OpConductorBuilder,
    OpConductorHandler,
    OpDeployerConfig,
    OpProposerBuilder,
    OpProposerHandler,
    OpRethBuilder,
    OpRethHandler,
    PROMETHEUS_DEFAULT_IMAGE,
    PROMETHEUS_DEFAULT_TAG,
    PrometheusConfig,
};

mod l2_stack;
pub use deployer::{DeploymentResult, L2StackHandler};
pub use l2_stack::L2StackBuilder;

/// Account information from Anvil.
#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub address: Bytes,
    pub private_key: Bytes,
}
