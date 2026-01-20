//! Deployment context passed between stages.

use std::path::PathBuf;

use crate::{AnvilHandler, KupDocker};

/// Context available at L1 stage (minimal).
pub struct L1Context<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub l1_chain_id: u64,
    pub l2_chain_id: u64,
}

/// Context after L1 is deployed.
pub struct ContractsContext<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub l1_chain_id: u64,
    pub l2_chain_id: u64,
    pub anvil: &'a AnvilHandler,
}

/// Context after contracts are deployed.
pub struct L2Context<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub l1_chain_id: u64,
    pub l2_chain_id: u64,
    pub anvil: &'a AnvilHandler,
}

/// Context after L2 stack is deployed.
pub struct MonitoringContext<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub l2_stack: &'a crate::L2StackHandler,
    pub dashboards_path: Option<PathBuf>,
}
