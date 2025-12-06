//! kupcake-deploy - Deployment library for the OP Stack.
//!
//! This crate provides the deployment functionality for bootstrapping a rust-based
//! OP Stack chain.

use alloy_core::primitives::Bytes;

mod deployer;
pub use deployer::Deployer;

mod docker;
mod fs;
pub mod services;

pub use docker::{
    DockerImage, DockerImageBuilder, KupDocker, KupDockerConfig, PortMapping, PortProtocol,
    ServiceConfig, ServiceHandler,
};
use serde::{Deserialize, Serialize};
pub use services::{
    AnvilConfig, AnvilHandler, GrafanaConfig, KonaNodeConfig, KonaNodeHandler, MetricsTarget,
    MonitoringConfig, OpBatcherConfig, OpBatcherHandler, OpChallengerConfig, OpChallengerHandler,
    OpDeployerConfig, OpProposerConfig, OpProposerHandler, OpRethConfig, OpRethHandler,
    PrometheusConfig,
};

/// Account information from Anvil.
pub struct AccountInfo {
    pub address: Bytes,
    pub private_key: Bytes,
}

/// Combined configuration for all L2 components for the op-stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2StackConfig {
    /// Configuration for op-reth execution client.
    pub op_reth: OpRethConfig,
    /// Configuration for kona-node consensus client.
    pub kona_node: KonaNodeConfig,
    /// Configuration for op-batcher.
    pub op_batcher: OpBatcherConfig,
    /// Configuration for op-proposer.
    pub op_proposer: OpProposerConfig,
    /// Configuration for op-challenger.
    pub op_challenger: OpChallengerConfig,
}

impl Default for L2StackConfig {
    fn default() -> Self {
        Self {
            op_reth: OpRethConfig::default(),
            kona_node: KonaNodeConfig::default(),
            op_batcher: OpBatcherConfig::default(),
            op_proposer: OpProposerConfig::default(),
            op_challenger: OpChallengerConfig::default(),
        }
    }
}
