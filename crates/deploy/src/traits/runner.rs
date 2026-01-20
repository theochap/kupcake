//! Deployment execution engine.

use anyhow::Result;
use std::path::PathBuf;

use super::context::{
    ContractsContext, L1Context, L2BatchingContext, L2FaultProofContext, L2NodeContext,
    L2NodesResult, L2ProposalContext, MonitoringContext,
};
use super::deployer::{Deployer, End};
use super::service::KupcakeService;
use crate::{
    services::{OpBatcherHandler, OpChallengerHandler, OpProposerHandler},
    AnvilHandler, KupDocker,
};

/// Result type for deployment chains.
///
/// This struct provides named access to all deployed service handlers.
pub struct DeploymentResult {
    pub anvil: AnvilHandler,
    pub l2_nodes: L2NodesResult,
    pub op_batcher: OpBatcherHandler,
    pub op_proposer: OpProposerHandler,
    pub op_challenger: OpChallengerHandler,
    pub monitoring: Option<crate::services::MonitoringHandler>,
}

/// Partial deployment result containing core services (L1 + Contracts + L2 services).
///
/// Used internally to avoid code duplication between standard and no-monitoring deployers.
struct CoreDeploymentResult {
    anvil: AnvilHandler,
    l2_nodes: L2NodesResult,
    op_batcher: OpBatcherHandler,
    op_proposer: OpProposerHandler,
    op_challenger: OpChallengerHandler,
}

/// Deploy the core stack (L1 + Contracts + L2 services) without monitoring.
///
/// This helper function contains the common deployment logic shared by both
/// StandardDeployer and NoMonitoringDeployer implementations.
async fn deploy_core_services(
    anvil_service: crate::AnvilConfig,
    contracts_service: crate::OpDeployerConfig,
    l2_nodes_service: crate::traits::l2_fleet::L2NodeFleet,
    batcher_service: crate::OpBatcherBuilder,
    proposer_service: crate::OpProposerBuilder,
    challenger_service: crate::OpChallengerBuilder,
    docker: &mut KupDocker,
    outdata: PathBuf,
    l1_chain_id: u64,
    l2_chain_id: u64,
) -> Result<CoreDeploymentResult> {
    // Stage 1: Deploy L1 (Anvil)
    tracing::info!("Starting Anvil...");
    let l1_ctx = L1Context {
        docker,
        outdata: outdata.clone(),
        l1_chain_id,
        l2_chain_id,
    };
    let anvil = anvil_service.deploy(l1_ctx).await?;

    // Stage 2: Deploy contracts
    tracing::info!("Deploying L1 contracts...");
    let contracts_ctx = ContractsContext {
        docker,
        outdata: outdata.clone(),
        l1_chain_id,
        l2_chain_id,
        anvil: &anvil,
    };
    let _ = contracts_service.deploy(contracts_ctx).await?;

    // Stage 3: Deploy L2 nodes
    tracing::info!("Deploying L2 nodes (op-reth + kona-node trios)...");
    let l2_node_ctx = L2NodeContext {
        docker,
        outdata: outdata.clone(),
        l1_chain_id,
        l2_chain_id,
        anvil: &anvil,
    };
    let l2_nodes = l2_nodes_service.deploy(l2_node_ctx).await?;

    // Stage 4: Deploy op-batcher
    tracing::info!("Starting op-batcher...");
    let batcher_ctx = L2BatchingContext {
        docker,
        outdata: outdata.clone(),
        anvil: &anvil,
        primary_op_reth: &l2_nodes.sequencers[0].op_reth,
        primary_kona_node: &l2_nodes.sequencers[0].kona_node,
    };
    let op_batcher = batcher_service.deploy(batcher_ctx).await?;

    // Stage 5: Deploy op-proposer
    tracing::info!("Starting op-proposer...");
    let proposer_ctx = L2ProposalContext {
        docker,
        outdata: outdata.clone(),
        anvil: &anvil,
        primary_kona_node: &l2_nodes.sequencers[0].kona_node,
    };
    let op_proposer = proposer_service.deploy(proposer_ctx).await?;

    // Stage 6: Deploy op-challenger
    tracing::info!("Starting op-challenger...");
    let challenger_ctx = L2FaultProofContext {
        docker,
        outdata,
        anvil: &anvil,
        primary_op_reth: &l2_nodes.sequencers[0].op_reth,
        primary_kona_node: &l2_nodes.sequencers[0].kona_node,
    };
    let op_challenger = challenger_service.deploy(challenger_ctx).await?;

    Ok(CoreDeploymentResult {
        anvil,
        l2_nodes,
        op_batcher,
        op_proposer,
        op_challenger,
    })
}

/// Trait for deploying a chain and collecting handlers.
pub trait DeployChain {
    /// Deploy all services in the chain.
    fn deploy_chain(
        self,
        docker: &mut KupDocker,
        outdata: PathBuf,
        l1_chain_id: u64,
        l2_chain_id: u64,
        dashboards_path: Option<PathBuf>,
    ) -> impl std::future::Future<Output = Result<DeploymentResult>> + Send;
}

// Implementation for StandardDeployer with monitoring
// AnvilConfig -> OpDeployerConfig -> L2NodeFleet -> OpBatcherBuilder -> OpProposerBuilder -> OpChallengerBuilder -> MonitoringConfig
impl DeployChain
    for Deployer<
        crate::AnvilConfig,
        Deployer<
            crate::OpDeployerConfig,
            Deployer<
                crate::traits::l2_fleet::L2NodeFleet,
                Deployer<
                    crate::OpBatcherBuilder,
                    Deployer<
                        crate::OpProposerBuilder,
                        Deployer<crate::OpChallengerBuilder, Deployer<crate::MonitoringConfig, End>>,
                    >,
                >,
            >,
        >,
    >
{
    async fn deploy_chain(
        self,
        docker: &mut KupDocker,
        outdata: PathBuf,
        l1_chain_id: u64,
        l2_chain_id: u64,
        dashboards_path: Option<PathBuf>,
    ) -> Result<DeploymentResult> {
        // Deploy core services (L1 + Contracts + L2 services)
        let core = deploy_core_services(
            self.service,
            self.next.service,
            self.next.next.service,
            self.next.next.next.service,
            self.next.next.next.next.service,
            self.next.next.next.next.next.service,
            docker,
            outdata.clone(),
            l1_chain_id,
            l2_chain_id,
        )
        .await?;

        // Stage 7: Deploy monitoring
        let mon_ctx = MonitoringContext {
            docker,
            outdata,
            l2_nodes: &core.l2_nodes,
            op_batcher: &core.op_batcher,
            op_proposer: &core.op_proposer,
            op_challenger: &core.op_challenger,
            dashboards_path,
        };
        let monitoring = self
            .next
            .next
            .next
            .next
            .next
            .next
            .service
            .deploy(mon_ctx)
            .await?;

        Ok(DeploymentResult {
            anvil: core.anvil,
            l2_nodes: core.l2_nodes,
            op_batcher: core.op_batcher,
            op_proposer: core.op_proposer,
            op_challenger: core.op_challenger,
            monitoring,
        })
    }
}

// Implementation for NoMonitoringDeployer without monitoring
// AnvilConfig -> OpDeployerConfig -> L2NodeFleet -> OpBatcherBuilder -> OpProposerBuilder -> OpChallengerBuilder
impl DeployChain
    for Deployer<
        crate::AnvilConfig,
        Deployer<
            crate::OpDeployerConfig,
            Deployer<
                crate::traits::l2_fleet::L2NodeFleet,
                Deployer<
                    crate::OpBatcherBuilder,
                    Deployer<crate::OpProposerBuilder, Deployer<crate::OpChallengerBuilder, End>>,
                >,
            >,
        >,
    >
{
    async fn deploy_chain(
        self,
        docker: &mut KupDocker,
        outdata: PathBuf,
        l1_chain_id: u64,
        l2_chain_id: u64,
        _dashboards_path: Option<PathBuf>,
    ) -> Result<DeploymentResult> {
        // Deploy core services (L1 + Contracts + L2 services)
        let core = deploy_core_services(
            self.service,
            self.next.service,
            self.next.next.service,
            self.next.next.next.service,
            self.next.next.next.next.service,
            self.next.next.next.next.next.service,
            docker,
            outdata,
            l1_chain_id,
            l2_chain_id,
        )
        .await?;

        Ok(DeploymentResult {
            anvil: core.anvil,
            l2_nodes: core.l2_nodes,
            op_batcher: core.op_batcher,
            op_proposer: core.op_proposer,
            op_challenger: core.op_challenger,
            monitoring: None,
        })
    }
}
