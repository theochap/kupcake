//! Standard deployer type aliases replicating current Kupcake behavior.

use crate::{AnvilConfig, L2StackBuilder, MonitoringConfig, OpDeployerConfig};

use super::deployer::{Deployer, End};

/// Standard OP Stack deployment chain with monitoring.
///
/// This type alias replicates the current Kupcake deployment behavior:
/// 1. Deploy Anvil (L1)
/// 2. Deploy contracts via op-deployer
/// 3. Deploy L2 stack (op-reth + kona-node nodes, batcher, proposer, challenger)
/// 4. Deploy monitoring (Prometheus + Grafana)
pub type StandardDeployer = Deployer<
    AnvilConfig,
    Deployer<OpDeployerConfig, Deployer<L2StackBuilder, Deployer<MonitoringConfig, End>>>,
>;

/// OP Stack deployment chain without monitoring.
///
/// Same as StandardDeployer but without the monitoring stage.
pub type NoMonitoringDeployer =
    Deployer<AnvilConfig, Deployer<OpDeployerConfig, Deployer<L2StackBuilder, End>>>;

/// Result type for standard deployment.
///
/// Provides named access to all deployed service handlers.
pub type StandardDeploymentResult = super::runner::DeploymentResult;

impl StandardDeployer {
    /// Create a standard deployer with default configurations.
    pub fn default_stack() -> Self {
        Deployer::new(AnvilConfig::default())
            .then(OpDeployerConfig::default())
            .then(L2StackBuilder::default())
            .then(MonitoringConfig::default())
    }
}

impl NoMonitoringDeployer {
    /// Create a deployer without monitoring using default configurations.
    pub fn default_stack() -> Self {
        Deployer::new(AnvilConfig::default())
            .then(OpDeployerConfig::default())
            .then(L2StackBuilder::default())
    }
}
