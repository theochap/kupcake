//! L2 Stack configuration and deployment.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    AnvilHandler, KupDocker, OpBatcherBuilder, OpChallengerBuilder, OpConductorBuilder,
    OpProposerBuilder,
    deployer::L2StackHandler,
    fs,
    services::l2_node::{L2NodeBuilder, L2NodeHandler},
};

/// Combined configuration for all L2 components for the op-stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2StackBuilder {
    /// Configuration for sequencer nodes (op-reth + kona-node pairs).
    /// When there are multiple sequencers, op-conductor is deployed for coordination.
    pub sequencers: Vec<L2NodeBuilder>,
    /// Configuration for validator nodes (op-reth + kona-node pairs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validators: Vec<L2NodeBuilder>,
    /// Configuration for op-batcher.
    pub op_batcher: OpBatcherBuilder,
    /// Configuration for op-proposer.
    pub op_proposer: OpProposerBuilder,
    /// Configuration for op-challenger.
    pub op_challenger: OpChallengerBuilder,
    /// Configuration for op-conductor (only used when there are multiple sequencers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_conductor: Option<OpConductorBuilder>,
}

impl Default for L2StackBuilder {
    fn default() -> Self {
        Self {
            sequencers: vec![L2NodeBuilder::sequencer()],
            validators: Vec::new(),
            op_batcher: OpBatcherBuilder::default(),
            op_proposer: OpProposerBuilder::default(),
            op_challenger: OpChallengerBuilder::default(),
            op_conductor: None,
        }
    }
}

impl L2StackBuilder {
    /// Create a new L2 stack builder with the specified number of sequencers and validators.
    ///
    /// # Arguments
    /// * `sequencer_count` - Number of sequencer nodes to deploy
    /// * `validator_count` - Number of validator nodes to deploy
    ///
    /// When `sequencer_count > 1`, op-conductor will be deployed automatically.
    pub fn with_counts(sequencer_count: usize, validator_count: usize) -> Self {
        assert!(
            sequencer_count >= 1,
            "At least one sequencer node is required"
        );

        let mut sequencers = Vec::with_capacity(sequencer_count);
        let mut validators = Vec::with_capacity(validator_count);

        // Add sequencer nodes
        for i in 0..sequencer_count {
            let mut node = L2NodeBuilder::sequencer();
            if i > 0 {
                node = node.with_name_suffix(&format!("sequencer-{}", i));
            }
            sequencers.push(node);
        }

        // Add validator nodes
        for i in 0..validator_count {
            validators
                .push(L2NodeBuilder::validator().with_name_suffix(&format!("validator-{}", i + 1)));
        }

        // Create op-conductor config if multiple sequencers
        let op_conductor = if sequencer_count > 1 {
            Some(OpConductorBuilder::default())
        } else {
            None
        };

        Self {
            sequencers,
            validators,
            op_batcher: OpBatcherBuilder::default(),
            op_proposer: OpProposerBuilder::default(),
            op_challenger: OpChallengerBuilder::default(),
            op_conductor,
        }
    }

    /// Create a new L2 stack builder with the specified number of nodes.
    ///
    /// The first node is always a sequencer, and additional nodes are validators.
    /// This is a convenience method equivalent to `with_counts(1, count - 1)`.
    pub fn with_node_count(count: usize) -> Self {
        assert!(count >= 1, "At least one node (the sequencer) is required");
        Self::with_counts(1, count.saturating_sub(1))
    }

    /// Add a validator node to the stack.
    pub fn add_validator(mut self) -> Self {
        let validator_index = self.validators.len() + 1;
        self.validators.push(
            L2NodeBuilder::validator().with_name_suffix(&format!("validator-{}", validator_index)),
        );
        self
    }

    /// Add a sequencer node to the stack.
    ///
    /// If this creates multiple sequencers, op-conductor config is automatically added.
    pub fn add_sequencer(mut self) -> Self {
        let sequencer_index = self.sequencers.len();
        self.sequencers.push(
            L2NodeBuilder::sequencer().with_name_suffix(&format!("sequencer-{}", sequencer_index)),
        );

        // Ensure op-conductor is configured if we now have multiple sequencers
        if self.sequencers.len() > 1 && self.op_conductor.is_none() {
            self.op_conductor = Some(OpConductorBuilder::default());
        }

        self
    }

    /// Get the primary sequencer node builder (the first sequencer).
    pub fn primary_sequencer(&self) -> &L2NodeBuilder {
        &self.sequencers[0]
    }

    /// Get the total number of L2 nodes (sequencers + validators).
    pub fn node_count(&self) -> usize {
        self.sequencers.len() + self.validators.len()
    }

    /// Returns true if op-conductor should be deployed (multiple sequencers).
    pub fn needs_conductor(&self) -> bool {
        self.sequencers.len() > 1
    }

    /// Start all L2 node components.
    ///
    /// This starts sequencer nodes first, then validator nodes,
    /// optionally followed by op-conductor (if multiple sequencers),
    /// then op-batcher (batch submitter), op-proposer, and op-challenger.
    /// Each L2 node pair (op-reth + kona-node) generates its own JWT for authentication.
    /// P2P peer discovery is enabled by passing enodes between nodes.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
    ) -> Result<L2StackHandler, anyhow::Error> {
        if !host_config_path.exists() {
            fs::FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Mutable lists of peer enodes for P2P discovery
        // Each node adds its enode after starting, so subsequent nodes can use it as a bootnode
        let mut kona_node_enodes: Vec<String> = Vec::new();
        let mut op_reth_enodes: Vec<String> = Vec::new();

        // Start all sequencer nodes
        let mut sequencer_handlers: Vec<L2NodeHandler> = Vec::with_capacity(self.sequencers.len());
        for (i, sequencer) in self.sequencers.iter().enumerate() {
            if i == 0 {
                tracing::info!("Starting primary sequencer node (op-reth + kona-node)...");
            } else {
                tracing::info!("Starting sequencer node {} (op-reth + kona-node)...", i + 1);
            }

            let sequencer_handler = sequencer
                .start(
                    docker,
                    &host_config_path,
                    anvil_handler,
                    None, // Sequencers don't follow another sequencer
                    &mut kona_node_enodes,
                    &mut op_reth_enodes,
                )
                .await
                .context(format!("Failed to start sequencer node {}", i + 1))?;

            sequencer_handlers.push(sequencer_handler);
        }

        // Get the primary sequencer's RPC URL for validators to follow
        let sequencer_rpc = sequencer_handlers[0].op_reth.http_rpc_url.clone();

        // Start validator nodes
        let mut validator_handlers: Vec<L2NodeHandler> = Vec::with_capacity(self.validators.len());
        for (i, validator) in self.validators.iter().enumerate() {
            tracing::info!("Starting validator node {} (op-reth + kona-node)...", i + 1);

            let validator_handler = validator
                .start(
                    docker,
                    &host_config_path,
                    anvil_handler,
                    Some(&sequencer_rpc),
                    &mut kona_node_enodes,
                    &mut op_reth_enodes,
                )
                .await
                .context(format!("Failed to start validator node {}", i + 1))?;

            validator_handlers.push(validator_handler);
        }

        tracing::info!(
            kona_node_peer_count = kona_node_enodes.len(),
            op_reth_peer_count = op_reth_enodes.len(),
            sequencer_count = self.sequencers.len(),
            validator_count = self.validators.len(),
            "All L2 nodes started with P2P peer discovery"
        );

        // Start op-conductor if we have multiple sequencers
        let op_conductor_handlers = if let Some(ref conductor_config) = self.op_conductor {
            tracing::info!(
                sequencer_count = self.sequencers.len(),
                "Starting op-conductor for sequencer consensus..."
            );

            let mut conductor_handlers = Vec::with_capacity(self.sequencers.len());

            for (i, sequencer) in sequencer_handlers.iter().enumerate() {
                let server_id = format!("sequencer-{}", i);
                let is_leader = i == 0;

                // Create a conductor config with unique container name for each sequencer
                let mut conductor = conductor_config.clone();
                if i > 0 {
                    conductor.container_name = format!("{}-{}", conductor.container_name, i);
                }

                let conductor_handler = if is_leader {
                    conductor
                        .start_leader(
                            docker,
                            &host_config_path,
                            &server_id,
                            &sequencer.op_reth,
                            &sequencer.kona_node,
                        )
                        .await
                        .context(format!(
                            "Failed to start op-conductor leader for {}",
                            server_id
                        ))?
                } else {
                    conductor
                        .start_follower(
                            docker,
                            &host_config_path,
                            &server_id,
                            &sequencer.op_reth,
                            &sequencer.kona_node,
                        )
                        .await
                        .context(format!(
                            "Failed to start op-conductor follower for {}",
                            server_id
                        ))?
                };

                tracing::info!(
                    server_id = %server_id,
                    is_leader = is_leader,
                    rpc_url = %conductor_handler.rpc_url,
                    "op-conductor instance started"
                );

                conductor_handlers.push(conductor_handler);
            }

            Some(conductor_handlers)
        } else {
            None
        };

        // Get references to the primary sequencer for the remaining components
        let primary_sequencer = &sequencer_handlers[0];

        tracing::info!("Starting op-batcher...");

        // Start op-batcher (connects to primary sequencer)
        let op_batcher_handler = self
            .op_batcher
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &primary_sequencer.op_reth,
                &primary_sequencer.kona_node,
            )
            .await?;

        tracing::info!("Starting op-proposer...");

        // Start op-proposer (connects to primary sequencer)
        let op_proposer_handler = self
            .op_proposer
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &primary_sequencer.kona_node,
            )
            .await?;

        tracing::info!("Starting op-challenger...");

        // Start op-challenger (connects to primary sequencer)
        let op_challenger_handler = self
            .op_challenger
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &primary_sequencer.kona_node,
                &self.sequencers[0].op_reth,
            )
            .await?;

        // Log all sequencer endpoints
        for (i, sequencer) in sequencer_handlers.iter().enumerate() {
            tracing::info!(
                role = "sequencer",
                index = i,
                l2_http_rpc = %sequencer.op_reth.http_rpc_url,
                l2_ws_rpc = %sequencer.op_reth.ws_rpc_url,
                kona_node_rpc = %sequencer.kona_node.rpc_url,
                "L2 sequencer node started"
            );
        }

        // Log all validator endpoints
        for (i, validator) in validator_handlers.iter().enumerate() {
            tracing::info!(
                role = "validator",
                index = i,
                l2_http_rpc = %validator.op_reth.http_rpc_url,
                l2_ws_rpc = %validator.op_reth.ws_rpc_url,
                kona_node_rpc = %validator.kona_node.rpc_url,
                "L2 validator node started"
            );
        }

        tracing::info!(
            op_batcher_rpc = %op_batcher_handler.rpc_url,
            op_proposer_rpc = %op_proposer_handler.rpc_url,
            op_challenger_rpc = %op_challenger_handler.rpc_url,
            "L2 stack started successfully"
        );

        Ok(L2StackHandler {
            sequencers: sequencer_handlers,
            validators: validator_handlers,
            op_batcher: op_batcher_handler,
            op_proposer: op_proposer_handler,
            op_challenger: op_challenger_handler,
        })
    }
}
