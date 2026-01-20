//! Deployment stage markers for type-state pattern.
//!
//! The deployment order is fixed:
//! L1 -> Contracts -> L2Node -> L2Batching -> L2Proposal -> L2FaultProof -> Monitoring
//!
//! Each stage provides context required by subsequent stages.

use serde::{Deserialize, Serialize};

/// Marker for the L1 (Anvil) deployment stage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct L1Stage;

/// Marker for the contracts deployment stage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ContractsStage;

/// Marker for the L2 node deployment stage.
///
/// Deploys all L2 node trios (op-reth + kona-node + optional op-conductor).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct L2NodeStage;

/// Marker for the L2 batching stage (op-batcher).
///
/// Deploys the batcher that submits L2 transaction batches to L1.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct L2BatchingStage;

/// Marker for the L2 proposal stage (op-proposer).
///
/// Deploys the proposer that submits L2 output roots to L1.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct L2ProposalStage;

/// Marker for the L2 fault proof stage (op-challenger).
///
/// Deploys the challenger that verifies and challenges invalid output roots.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct L2FaultProofStage;

/// Marker for the monitoring deployment stage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct MonitoringStage;

/// Sealed trait for deployment stages.
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::L1Stage {}
    impl Sealed for super::ContractsStage {}
    impl Sealed for super::L2NodeStage {}
    impl Sealed for super::L2BatchingStage {}
    impl Sealed for super::L2ProposalStage {}
    impl Sealed for super::L2FaultProofStage {}
    impl Sealed for super::MonitoringStage {}
}

/// Marker trait for valid deployment stages.
pub trait DeploymentStage: sealed::Sealed + Default + Clone + Send + Sync + 'static {}

impl DeploymentStage for L1Stage {}
impl DeploymentStage for ContractsStage {}
impl DeploymentStage for L2NodeStage {}
impl DeploymentStage for L2BatchingStage {}
impl DeploymentStage for L2ProposalStage {}
impl DeploymentStage for L2FaultProofStage {}
impl DeploymentStage for MonitoringStage {}

/// Trait encoding valid stage transitions.
///
/// This is implemented only for valid transitions:
/// - L1Stage -> ContractsStage
/// - ContractsStage -> L2NodeStage
/// - L2NodeStage -> L2BatchingStage
/// - L2BatchingStage -> L2ProposalStage
/// - L2ProposalStage -> L2FaultProofStage
/// - L2FaultProofStage -> MonitoringStage
pub trait NextStage: DeploymentStage {
    type Next: DeploymentStage;
}

impl NextStage for L1Stage {
    type Next = ContractsStage;
}

impl NextStage for ContractsStage {
    type Next = L2NodeStage;
}

impl NextStage for L2NodeStage {
    type Next = L2BatchingStage;
}

impl NextStage for L2BatchingStage {
    type Next = L2ProposalStage;
}

impl NextStage for L2ProposalStage {
    type Next = L2FaultProofStage;
}

impl NextStage for L2FaultProofStage {
    type Next = MonitoringStage;
}

// MonitoringStage has no NextStage impl - it's terminal
