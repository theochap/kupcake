//! L2 Node Fleet service for deploying multiple L2 nodes sequentially.
//!
//! This module provides the `L2NodeFleet` service which wraps multiple
//! L2NodeBuilder instances (sequencers + validators) and deploys them
//! in the correct order with enode accumulation for P2P networking.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::services::l2_node::{ConductorContext, L2NodeBuilder};

use super::{
    context::{L2NodeContext, L2NodesResult},
    service::KupcakeService,
    stages::L2NodeStage,
};

/// Fleet of L2 nodes (sequencers + validators) to be deployed together.
///
/// This service implements sequential deployment of all L2 nodes with
/// enode accumulation, ensuring each node can discover previously started nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2NodeFleet {
    /// Sequencer node builders (may include conductor configs for multi-sequencer setups).
    pub sequencers: Vec<L2NodeBuilder>,
    /// Validator node builders.
    pub validators: Vec<L2NodeBuilder>,
}

impl KupcakeService for L2NodeFleet {
    type Stage = L2NodeStage;
    type Handler = L2NodesResult;
    type Context<'a> = L2NodeContext<'a>;

    const SERVICE_NAME: &'static str = "l2-node-fleet";

    async fn deploy<'a>(self, ctx: Self::Context<'a>) -> Result<Self::Handler>
    where
        Self: 'a,
    {
        let host_config_path = ctx.outdata.join("l2-stack");

        // Create config directory if it doesn't exist
        if !host_config_path.exists() {
            crate::fs::FsHandler::create_host_config_directory(&host_config_path)?;
        }

        let mut kona_node_enodes = Vec::new();
        let mut op_reth_enodes = Vec::new();
        let mut sequencer_handlers = Vec::new();
        let mut validator_handlers = Vec::new();

        // Deploy sequencers with conductor context
        for (i, node) in self.sequencers.into_iter().enumerate() {
            let conductor_ctx = if node.op_conductor.is_some() {
                if i == 0 {
                    ConductorContext::Leader { index: i }
                } else {
                    ConductorContext::Follower { index: i }
                }
            } else {
                ConductorContext::None
            };

            tracing::info!(
                index = i,
                role = "sequencer",
                has_conductor = node.op_conductor.is_some(),
                "Deploying L2 sequencer node"
            );

            let handler = node
                .start(
                    ctx.docker,
                    &host_config_path,
                    ctx.anvil,
                    None, // Sequencers don't need sequencer_rpc
                    &mut kona_node_enodes,
                    &mut op_reth_enodes,
                    ctx.l1_chain_id,
                    conductor_ctx,
                )
                .await?;

            sequencer_handlers.push(handler);
        }

        // Get primary sequencer RPC for validators
        let sequencer_rpc = sequencer_handlers.first().map(|h| &h.kona_node.rpc_url);

        // Deploy validators
        for (i, node) in self.validators.into_iter().enumerate() {
            tracing::info!(
                index = i,
                role = "validator",
                "Deploying L2 validator node"
            );

            let handler = node
                .start(
                    ctx.docker,
                    &host_config_path,
                    ctx.anvil,
                    sequencer_rpc,
                    &mut kona_node_enodes,
                    &mut op_reth_enodes,
                    ctx.l1_chain_id,
                    ConductorContext::None,
                )
                .await?;

            validator_handlers.push(handler);
        }

        tracing::info!(
            sequencer_count = sequencer_handlers.len(),
            validator_count = validator_handlers.len(),
            kona_node_enodes = kona_node_enodes.len(),
            op_reth_enodes = op_reth_enodes.len(),
            "L2 node fleet deployed successfully"
        );

        Ok(L2NodesResult {
            sequencers: sequencer_handlers,
            validators: validator_handlers,
            kona_node_enodes,
            op_reth_enodes,
        })
    }
}

impl Default for L2NodeFleet {
    fn default() -> Self {
        Self {
            sequencers: vec![L2NodeBuilder::sequencer()],
            validators: vec![],
        }
    }
}
