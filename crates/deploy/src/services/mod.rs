//! Service modules for the OP Stack deployment.
//!
//! Each service is in its own submodule with:
//! - `cmd.rs` - Command builder for generating Docker commands
//! - `mod.rs` - Config, Handler, and start logic

pub mod anvil;
pub mod grafana;
pub mod kona_node;
pub mod l2_node;
pub mod op_batcher;
pub mod op_challenger;
pub mod op_conductor;
pub mod op_deployer;
pub mod op_proposer;
pub mod op_reth;

// Re-export commonly used types
pub use anvil::{
    AnvilAccounts, AnvilConfig, AnvilHandler, DEFAULT_DOCKER_IMAGE as ANVIL_DEFAULT_IMAGE,
    DEFAULT_DOCKER_TAG as ANVIL_DEFAULT_TAG,
};
pub use grafana::{
    DEFAULT_GRAFANA_DOCKER_IMAGE as GRAFANA_DEFAULT_IMAGE,
    DEFAULT_GRAFANA_DOCKER_TAG as GRAFANA_DEFAULT_TAG,
    DEFAULT_PROMETHEUS_DOCKER_IMAGE as PROMETHEUS_DEFAULT_IMAGE,
    DEFAULT_PROMETHEUS_DOCKER_TAG as PROMETHEUS_DEFAULT_TAG, GrafanaConfig, MetricsTarget,
    MonitoringConfig, MonitoringHandler, PrometheusConfig,
};
pub use kona_node::{
    DEFAULT_DOCKER_IMAGE as KONA_NODE_DEFAULT_IMAGE, DEFAULT_DOCKER_TAG as KONA_NODE_DEFAULT_TAG,
    KonaNodeBuilder, KonaNodeHandler,
};
pub use l2_node::{L2NodeBuilder, L2NodeHandler, L2NodeRole};
pub use op_batcher::{
    DEFAULT_DOCKER_IMAGE as OP_BATCHER_DEFAULT_IMAGE, DEFAULT_DOCKER_TAG as OP_BATCHER_DEFAULT_TAG,
    OpBatcherBuilder, OpBatcherHandler,
};
pub use op_challenger::{
    DEFAULT_DOCKER_IMAGE as OP_CHALLENGER_DEFAULT_IMAGE,
    DEFAULT_DOCKER_TAG as OP_CHALLENGER_DEFAULT_TAG, OpChallengerBuilder, OpChallengerHandler,
};
pub use op_conductor::{
    DEFAULT_DOCKER_IMAGE as OP_CONDUCTOR_DEFAULT_IMAGE,
    DEFAULT_DOCKER_TAG as OP_CONDUCTOR_DEFAULT_TAG, OpConductorBuilder, OpConductorHandler,
};
pub use op_deployer::{
    DEFAULT_DOCKER_IMAGE as OP_DEPLOYER_DEFAULT_IMAGE,
    DEFAULT_DOCKER_TAG as OP_DEPLOYER_DEFAULT_TAG, OpDeployerConfig,
};
pub use op_proposer::{
    DEFAULT_DOCKER_IMAGE as OP_PROPOSER_DEFAULT_IMAGE,
    DEFAULT_DOCKER_TAG as OP_PROPOSER_DEFAULT_TAG, OpProposerBuilder, OpProposerHandler,
};
pub use op_reth::{
    DEFAULT_DOCKER_IMAGE as OP_RETH_DEFAULT_IMAGE, DEFAULT_DOCKER_TAG as OP_RETH_DEFAULT_TAG,
    OpRethBuilder, OpRethHandler,
};
