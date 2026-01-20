//! Deployment execution engine.

use anyhow::Result;
use std::path::PathBuf;

use super::context::{ContractsContext, L1Context, L2Context, MonitoringContext};
use super::deployer::{Deployer, End};
use super::service::KupcakeService;
use crate::{AnvilHandler, KupDocker, L2StackHandler};

/// Result type for deployment chains.
///
/// This struct provides named access to all deployed service handlers.
pub struct DeploymentResult {
    pub anvil: AnvilHandler,
    pub l2_stack: L2StackHandler,
    pub monitoring: Option<crate::services::MonitoringHandler>,
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

// Implementation for AnvilConfig -> OpDeployerConfig -> L2StackBuilder -> MonitoringConfig
impl DeployChain
    for Deployer<
        crate::AnvilConfig,
        Deployer<
            crate::OpDeployerConfig,
            Deployer<crate::L2StackBuilder, Deployer<crate::MonitoringConfig, End>>,
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
        // Stage 1: Deploy L1 (Anvil)
        tracing::info!("Starting Anvil...");
        let l1_ctx = L1Context {
            docker,
            outdata: outdata.clone(),
            l1_chain_id,
            l2_chain_id,
        };
        let anvil = self.service.deploy(l1_ctx).await?;

        // Stage 2: Deploy contracts
        tracing::info!("Deploying L1 contracts...");
        let contracts_ctx = ContractsContext {
            docker,
            outdata: outdata.clone(),
            l1_chain_id,
            l2_chain_id,
            anvil: &anvil,
        };
        let _ = self.next.service.deploy(contracts_ctx).await?;

        // Stage 3: Deploy L2 stack
        tracing::info!("Starting L2 stack (op-reth + kona-node nodes + op-batcher + op-proposer + op-challenger)...");

        let l2_ctx = L2Context {
            docker,
            outdata: outdata.clone(),
            l1_chain_id,
            l2_chain_id,
            anvil: &anvil,
        };
        let l2_stack = self.next.next.service.deploy(l2_ctx).await?;

        // Stage 4: Deploy monitoring (optional)
        let mon_ctx = MonitoringContext {
            docker,
            outdata,
            l2_stack: &l2_stack,
            dashboards_path,
        };
        let monitoring = self.next.next.next.service.deploy(mon_ctx).await?;

        Ok(DeploymentResult {
            anvil,
            l2_stack,
            monitoring,
        })
    }
}

// Implementation for AnvilConfig -> OpDeployerConfig -> L2StackBuilder (no monitoring)
impl DeployChain
    for Deployer<
        crate::AnvilConfig,
        Deployer<crate::OpDeployerConfig, Deployer<crate::L2StackBuilder, End>>,
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
        // Stage 1: Deploy L1 (Anvil)
        tracing::info!("Starting Anvil...");
        let l1_ctx = L1Context {
            docker,
            outdata: outdata.clone(),
            l1_chain_id,
            l2_chain_id,
        };
        let anvil = self.service.deploy(l1_ctx).await?;

        // Stage 2: Deploy contracts
        tracing::info!("Deploying L1 contracts...");
        let contracts_ctx = ContractsContext {
            docker,
            outdata: outdata.clone(),
            l1_chain_id,
            l2_chain_id,
            anvil: &anvil,
        };
        let _ = self.next.service.deploy(contracts_ctx).await?;

        // Stage 3: Deploy L2 stack
        tracing::info!("Starting L2 stack (op-reth + kona-node nodes + op-batcher + op-proposer + op-challenger)...");

        let l2_ctx = L2Context {
            docker,
            outdata,
            l1_chain_id,
            l2_chain_id,
            anvil: &anvil,
        };
        let l2_stack = self.next.next.service.deploy(l2_ctx).await?;

        Ok(DeploymentResult {
            anvil,
            l2_stack,
            monitoring: None,
        })
    }
}
