//! Deployment context passed between stages.

use std::path::PathBuf;

use crate::{
    AnvilHandler, KupDocker,
    services::{
        kona_node::KonaNodeHandler,
        l2_node::L2NodeHandler,
        op_reth::OpRethHandler,
        OpBatcherHandler,
        OpChallengerHandler,
        OpProposerHandler,
    },
};

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

/// Context for the L2 node deployment stage.
///
/// This is the first L2 stage - it receives the same context as ContractsContext.
pub struct L2NodeContext<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub l1_chain_id: u64,
    pub l2_chain_id: u64,
    pub anvil: &'a AnvilHandler,
}

/// Result from deploying all L2 nodes (sequencers + validators).
///
/// This is returned by services in the L2NodeStage and used by subsequent stages.
pub struct L2NodesResult {
    /// Handlers for all deployed sequencer nodes.
    pub sequencers: Vec<L2NodeHandler>,
    /// Handlers for all deployed validator nodes.
    pub validators: Vec<L2NodeHandler>,
    /// Accumulated kona-node enodes for P2P discovery.
    pub kona_node_enodes: Vec<String>,
    /// Accumulated op-reth enodes for P2P discovery.
    pub op_reth_enodes: Vec<String>,
}

/// Context for the L2 batching stage (op-batcher deployment).
///
/// Requires primary sequencer handlers from the L2NodeStage.
pub struct L2BatchingContext<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub anvil: &'a AnvilHandler,
    pub primary_op_reth: &'a OpRethHandler,
    pub primary_kona_node: &'a KonaNodeHandler,
}

/// Context for the L2 proposal stage (op-proposer deployment).
///
/// Requires primary sequencer's kona-node handler.
pub struct L2ProposalContext<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub anvil: &'a AnvilHandler,
    pub primary_kona_node: &'a KonaNodeHandler,
}

/// Context for the L2 fault proof stage (op-challenger deployment).
///
/// Requires primary sequencer handlers for fault proof verification.
pub struct L2FaultProofContext<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub anvil: &'a AnvilHandler,
    pub primary_op_reth: &'a OpRethHandler,
    pub primary_kona_node: &'a KonaNodeHandler,
}

/// Context after L2 stack is fully deployed.
pub struct MonitoringContext<'a> {
    pub docker: &'a mut KupDocker,
    pub outdata: PathBuf,
    pub l2_nodes: &'a L2NodesResult,
    pub op_batcher: &'a OpBatcherHandler,
    pub op_proposer: &'a OpProposerHandler,
    pub op_challenger: &'a OpChallengerHandler,
    pub dashboards_path: Option<PathBuf>,
}

