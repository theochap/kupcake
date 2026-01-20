//! Standard deployer type aliases replicating current Kupcake behavior.

use crate::{AnvilConfig, MonitoringConfig, OpBatcherBuilder, OpChallengerBuilder, OpDeployerConfig, OpProposerBuilder};

use super::deployer::{Deployer, End};
use super::l2_fleet::L2NodeFleet;

/// Standard OP Stack deployment chain with monitoring.
///
/// This type alias chains through all deployment stages:
/// 1. Deploy Anvil (L1)
/// 2. Deploy contracts via op-deployer
/// 3. Deploy L2 nodes (op-reth + kona-node trios)
/// 4. Deploy op-batcher
/// 5. Deploy op-proposer
/// 6. Deploy op-challenger
/// 7. Deploy monitoring (Prometheus + Grafana)
pub type StandardDeployer = Deployer<
    AnvilConfig,
    Deployer<
        OpDeployerConfig,
        Deployer<
            L2NodeFleet,
            Deployer<
                OpBatcherBuilder,
                Deployer<
                    OpProposerBuilder,
                    Deployer<OpChallengerBuilder, Deployer<MonitoringConfig, End>>,
                >,
            >,
        >,
    >,
>;

/// OP Stack deployment chain without monitoring.
///
/// Same as StandardDeployer but without the monitoring stage.
pub type NoMonitoringDeployer = Deployer<
    AnvilConfig,
    Deployer<
        OpDeployerConfig,
        Deployer<
            L2NodeFleet,
            Deployer<
                OpBatcherBuilder,
                Deployer<OpProposerBuilder, Deployer<OpChallengerBuilder, End>>,
            >,
        >,
    >,
>;

/// Result type for standard deployment.
///
/// Provides named access to all deployed service handlers.
pub type StandardDeploymentResult = super::runner::DeploymentResult;

impl StandardDeployer {
    /// Create a standard deployer with default configurations.
    pub fn default_stack() -> Self {
        Deployer::new(AnvilConfig::default())
            .then(OpDeployerConfig::default())
            .then(L2NodeFleet::default())
            .then(OpBatcherBuilder::default())
            .then(OpProposerBuilder::default())
            .then(OpChallengerBuilder::default())
            .then(MonitoringConfig::default())
    }
}

impl NoMonitoringDeployer {
    /// Create a deployer without monitoring using default configurations.
    pub fn default_stack() -> Self {
        Deployer::new(AnvilConfig::default())
            .then(OpDeployerConfig::default())
            .then(L2NodeFleet::default())
            .then(OpBatcherBuilder::default())
            .then(OpProposerBuilder::default())
            .then(OpChallengerBuilder::default())
    }
}
