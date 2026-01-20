//! Deployment stage markers for type-state pattern.
//!
//! The deployment order is fixed: L1 -> Contracts -> L2 -> Monitoring
//! Each stage provides context required by subsequent stages.

use serde::{Deserialize, Serialize};

/// Marker for the L1 (Anvil) deployment stage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct L1Stage;

/// Marker for the contracts deployment stage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ContractsStage;

/// Marker for the L2 stack deployment stage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct L2Stage;

/// Marker for the monitoring deployment stage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct MonitoringStage;

/// Sealed trait for deployment stages.
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::L1Stage {}
    impl Sealed for super::ContractsStage {}
    impl Sealed for super::L2Stage {}
    impl Sealed for super::MonitoringStage {}
}

/// Marker trait for valid deployment stages.
pub trait DeploymentStage: sealed::Sealed + Default + Clone + Send + Sync + 'static {}

impl DeploymentStage for L1Stage {}
impl DeploymentStage for ContractsStage {}
impl DeploymentStage for L2Stage {}
impl DeploymentStage for MonitoringStage {}

/// Trait encoding valid stage transitions.
///
/// This is implemented only for valid transitions:
/// - L1Stage -> ContractsStage
/// - ContractsStage -> L2Stage
/// - L2Stage -> MonitoringStage
pub trait NextStage: DeploymentStage {
    type Next: DeploymentStage;
}

impl NextStage for L1Stage {
    type Next = ContractsStage;
}

impl NextStage for ContractsStage {
    type Next = L2Stage;
}

impl NextStage for L2Stage {
    type Next = MonitoringStage;
}

// MonitoringStage has no NextStage impl - it's terminal
