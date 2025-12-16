//! L2 Stack configuration and deployment.

use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    AnvilHandler, KupDocker, OpBatcherBuilder, OpChallengerBuilder, OpProposerBuilder,
    deployer::L2StackHandler,
    fs,
    services::l2_node::{L2NodeBuilder, L2NodeHandler},
};

/// Combined configuration for all L2 components for the op-stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2StackBuilder {
    /// Configuration for L2 nodes (op-reth + kona-node pairs).
    /// The first node is always the sequencer, subsequent nodes are validators.
    pub nodes: Vec<L2NodeBuilder>,
    /// Configuration for op-batcher.
    pub op_batcher: OpBatcherBuilder,
    /// Configuration for op-proposer.
    pub op_proposer: OpProposerBuilder,
    /// Configuration for op-challenger.
    pub op_challenger: OpChallengerBuilder,
}

impl Default for L2StackBuilder {
    fn default() -> Self {
        Self {
            nodes: vec![L2NodeBuilder::sequencer()],
            op_batcher: OpBatcherBuilder::default(),
            op_proposer: OpProposerBuilder::default(),
            op_challenger: OpChallengerBuilder::default(),
        }
    }
}

impl L2StackBuilder {
    /// Create a new L2 stack builder with the specified number of nodes.
    ///
    /// The first node is always a sequencer, and additional nodes are validators.
    pub fn with_node_count(count: usize) -> Self {
        assert!(count >= 1, "At least one node (the sequencer) is required");

        let mut nodes = Vec::with_capacity(count);

        // First node is the sequencer
        nodes.push(L2NodeBuilder::sequencer());

        let mut nodes = Self {
            nodes,
            ..Default::default()
        };

        // Additional nodes are validators
        for _ in 1..count {
            nodes = nodes.add_validator();
        }

        nodes
    }

    /// Add a validator node to the stack.
    pub fn add_validator(mut self) -> Self {
        let validator_index = self.nodes.len();
        self.nodes.push(
            L2NodeBuilder::validator().with_name_suffix(&format!("validator-{}", validator_index)),
        );
        self
    }

    /// Get the sequencer node builder (the first node).
    pub fn sequencer(&self) -> &L2NodeBuilder {
        &self.nodes[0]
    }

    /// Get validator node builders (all nodes except the first).
    pub fn validators(&self) -> Vec<&L2NodeBuilder> {
        self.nodes.iter().skip(1).collect()
    }

    /// Start all L2 node components.
    ///
    /// This starts the sequencer node first, then validator nodes,
    /// followed by op-batcher (batch submitter), op-proposer, and op-challenger.
    /// Each L2 node pair (op-reth + kona-node) generates its own JWT for authentication.
    /// P2P peer discovery is enabled by passing ENRs between nodes.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
    ) -> Result<L2StackHandler, anyhow::Error> {
        if !host_config_path.exists() {
            fs::FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Start all L2 nodes (each generates its own JWT)
        let mut node_handlers: Vec<L2NodeHandler> = Vec::with_capacity(self.nodes.len());

        // Mutable lists of peer ENRs for P2P discovery
        // Each node adds its ENR after starting, so subsequent nodes can use it as a bootnode
        let mut kona_node_enrs: Vec<String> = Vec::new();
        let mut op_reth_enrs: Vec<String> = Vec::new();

        // Start the sequencer first (it must be first)
        tracing::info!("Starting sequencer node (op-reth + kona-node)...");
        let sequencer_handler = self.nodes[0]
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                None,
                &mut kona_node_enrs,
                &mut op_reth_enrs,
            )
            .await
            .context("Failed to start sequencer node")?;

        let sequencer_rpc = sequencer_handler.op_reth.http_rpc_url.clone();
        node_handlers.push(sequencer_handler);

        // Start validator nodes (if any)
        for (i, validator) in self.nodes.iter().skip(1).enumerate() {
            tracing::info!("Starting validator node {} (op-reth + kona-node)...", i + 1);

            let validator_handler = validator
                .start(
                    docker,
                    &host_config_path,
                    anvil_handler,
                    Some(&sequencer_rpc),
                    &mut kona_node_enrs,
                    &mut op_reth_enrs,
                )
                .await
                .context(format!("Failed to start validator node {}", i + 1))?;

            node_handlers.push(validator_handler);
        }

        tracing::info!(
            kona_node_peer_count = kona_node_enrs.len(),
            op_reth_peer_count = op_reth_enrs.len(),
            "All L2 nodes started with P2P peer discovery"
        );

        // Get references to the sequencer handlers for the remaining components
        let sequencer = &node_handlers[0];

        tracing::info!("Starting op-batcher...");

        // Start op-batcher (connects to sequencer)
        let op_batcher_handler = self
            .op_batcher
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &sequencer.op_reth,
                &sequencer.kona_node,
            )
            .await?;

        tracing::info!("Starting op-proposer...");

        // Start op-proposer (connects to sequencer)
        let op_proposer_handler = self
            .op_proposer
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &sequencer.kona_node,
            )
            .await?;

        tracing::info!("Starting op-challenger...");

        // Start op-challenger (connects to sequencer)
        let op_challenger_handler = self
            .op_challenger
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &sequencer.kona_node,
                &self.nodes[0].op_reth,
            )
            .await?;

        // Log all node endpoints
        for (i, node) in node_handlers.iter().enumerate() {
            let role = if i == 0 { "sequencer" } else { "validator" };
            tracing::info!(
                role = role,
                index = i,
                l2_http_rpc = %node.op_reth.http_rpc_url,
                l2_ws_rpc = %node.op_reth.ws_rpc_url,
                kona_node_rpc = %node.kona_node.rpc_url,
                "L2 node started"
            );
        }

        tracing::info!(
            op_batcher_rpc = %op_batcher_handler.rpc_url,
            op_proposer_rpc = %op_proposer_handler.rpc_url,
            op_challenger_rpc = %op_challenger_handler.rpc_url,
            "L2 stack started successfully"
        );

        Ok(L2StackHandler {
            nodes: node_handlers,
            op_batcher: op_batcher_handler,
            op_proposer: op_proposer_handler,
            op_challenger: op_challenger_handler,
        })
    }
}
