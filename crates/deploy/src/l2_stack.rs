//! L2 Stack configuration and deployment.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    AnvilHandler, KupDocker, OpBatcherBuilder, OpChallengerBuilder, OpConductorBuilder,
    OpProposerBuilder,
    deployer::L2StackHandler,
    fs,
    services::l2_node::{ConductorContext, L2NodeBuilder, L2NodeHandler},
};

/// Combined configuration for all L2 components for the op-stack.
///
/// Each sequencer node can optionally have an op-conductor attached for
/// multi-sequencer Raft consensus coordination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2StackBuilder {
    /// Configuration for sequencer nodes (op-reth + kona-node pairs).
    /// When there are multiple sequencers, each has an op-conductor for coordination.
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
}

impl Default for L2StackBuilder {
    fn default() -> Self {
        Self {
            sequencers: vec![L2NodeBuilder::sequencer()],
            validators: Vec::new(),
            op_batcher: OpBatcherBuilder::default(),
            op_proposer: OpProposerBuilder::default(),
            op_challenger: OpChallengerBuilder::default(),
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
    /// When `sequencer_count > 1`, each sequencer gets an op-conductor for Raft coordination.
    pub fn with_counts(sequencer_count: usize, validator_count: usize) -> Self {
        assert!(
            sequencer_count >= 1,
            "At least one sequencer node is required"
        );

        let needs_conductor = sequencer_count > 1;
        let mut sequencers = Vec::with_capacity(sequencer_count);
        let mut validators = Vec::with_capacity(validator_count);

        // Add sequencer nodes, each with optional conductor config
        for i in 0..sequencer_count {
            let mut node = L2NodeBuilder::sequencer();
            if i > 0 {
                node = node.with_name_suffix(&format!("sequencer-{}", i));
            }

            // Attach conductor config if multi-sequencer setup
            if needs_conductor {
                let mut conductor = OpConductorBuilder::default();
                if i > 0 {
                    conductor.container_name = format!("{}-{}", conductor.container_name, i);
                }
                node.op_conductor = Some(conductor);
            }

            sequencers.push(node);
        }

        // Add validator nodes (no conductors)
        for i in 0..validator_count {
            validators
                .push(L2NodeBuilder::validator().with_name_suffix(&format!("validator-{}", i + 1)));
        }

        Self {
            sequencers,
            validators,
            op_batcher: OpBatcherBuilder::default(),
            op_proposer: OpProposerBuilder::default(),
            op_challenger: OpChallengerBuilder::default(),
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
    /// If this creates multiple sequencers, op-conductor configs are automatically added
    /// to all sequencers for Raft coordination.
    pub fn add_sequencer(mut self) -> Self {
        let sequencer_index = self.sequencers.len();
        let mut new_sequencer =
            L2NodeBuilder::sequencer().with_name_suffix(&format!("sequencer-{}", sequencer_index));

        // If we're adding a second sequencer, we need conductors for all
        let needs_conductor = self.sequencers.len() >= 1; // Will have 2+ after adding

        if needs_conductor {
            // Add conductor to existing first sequencer if it doesn't have one
            if self.sequencers[0].op_conductor.is_none() {
                self.sequencers[0].op_conductor = Some(OpConductorBuilder::default());
            }

            // Add conductors to any other existing sequencers
            for (i, seq) in self.sequencers.iter_mut().enumerate().skip(1) {
                if seq.op_conductor.is_none() {
                    let mut conductor = OpConductorBuilder::default();
                    conductor.container_name = format!("{}-{}", conductor.container_name, i);
                    seq.op_conductor = Some(conductor);
                }
            }

            // Add conductor to the new sequencer
            let mut conductor = OpConductorBuilder::default();
            conductor.container_name = format!("{}-{}", conductor.container_name, sequencer_index);
            new_sequencer.op_conductor = Some(conductor);
        }

        self.sequencers.push(new_sequencer);
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

    /// Returns true if op-conductor should be deployed (any sequencer has conductor config).
    pub fn needs_conductor(&self) -> bool {
        self.sequencers.iter().any(|s| s.op_conductor.is_some())
    }

    /// Set the binary path or source directory for op-reth for all nodes (sequencers and validators).
    pub fn set_op_reth_binary(mut self, path: impl Into<PathBuf>) -> Self {
        let docker_image = crate::docker::DockerImage::from_binary_with_name(path, "op-reth");
        for sequencer in &mut self.sequencers {
            sequencer.op_reth.docker_image = docker_image.clone();
        }
        for validator in &mut self.validators {
            validator.op_reth.docker_image = docker_image.clone();
        }
        self
    }

    /// Set the binary path or source directory for kona-node for all nodes (sequencers and validators).
    pub fn set_kona_node_binary(mut self, path: impl Into<PathBuf>) -> Self {
        let docker_image = crate::docker::DockerImage::from_binary_with_name(path, "kona-node");
        for sequencer in &mut self.sequencers {
            sequencer.kona_node.docker_image = docker_image.clone();
        }
        for validator in &mut self.validators {
            validator.kona_node.docker_image = docker_image.clone();
        }
        self
    }

    /// Set the binary path or source directory for op-batcher.
    pub fn set_op_batcher_binary(mut self, path: impl Into<PathBuf>) -> Self {
        self.op_batcher.docker_image =
            crate::docker::DockerImage::from_binary_with_name(path, "op-batcher");
        self
    }

    /// Set the binary path or source directory for op-proposer.
    pub fn set_op_proposer_binary(mut self, path: impl Into<PathBuf>) -> Self {
        self.op_proposer.docker_image =
            crate::docker::DockerImage::from_binary_with_name(path, "op-proposer");
        self
    }

    /// Set the binary path or source directory for op-challenger.
    pub fn set_op_challenger_binary(mut self, path: impl Into<PathBuf>) -> Self {
        self.op_challenger.docker_image =
            crate::docker::DockerImage::from_binary_with_name(path, "op-challenger");
        self
    }

    /// Set the binary path or source directory for op-conductor for all sequencers that have conductor config.
    pub fn set_op_conductor_binary(mut self, path: impl Into<PathBuf>) -> Self {
        let docker_image =
            crate::docker::DockerImage::from_binary_with_name(path, "op-conductor");
        for sequencer in &mut self.sequencers {
            if let Some(conductor) = &mut sequencer.op_conductor {
                conductor.docker_image = docker_image.clone();
            }
        }
        self
    }

    /// Start all L2 node components.
    ///
    /// This starts sequencer nodes first (with their op-conductors if configured),
    /// then validator nodes, then op-batcher, op-proposer, and op-challenger.
    /// Each L2 node pair (op-reth + kona-node) generates its own JWT for authentication.
    /// P2P peer discovery is enabled by passing enodes between nodes.
    ///
    /// # Arguments
    /// * `docker` - Docker client
    /// * `host_config_path` - Path on host for config files
    /// * `anvil_handler` - Handler for the L1 Anvil instance
    /// * `l1_chain_id` - L1 chain ID (used to determine if we need a custom L1 config for kona-node)
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
        l1_chain_id: u64,
    ) -> Result<L2StackHandler, anyhow::Error> {
        if !host_config_path.exists() {
            fs::FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Mutable lists of peer enodes for P2P discovery
        // Each node adds its enode after starting, so subsequent nodes can use it as a bootnode
        let mut kona_node_enodes: Vec<String> = Vec::new();
        let mut op_reth_enodes: Vec<String> = Vec::new();

        // Determine if we need conductor coordination
        let needs_conductor = self.needs_conductor();

        // Start all sequencer nodes (with conductors if configured)
        let mut sequencer_handlers: Vec<L2NodeHandler> = Vec::with_capacity(self.sequencers.len());
        for (i, sequencer) in self.sequencers.iter().enumerate() {
            // Determine conductor context for this sequencer
            let conductor_context = if needs_conductor && sequencer.op_conductor.is_some() {
                if i == 0 {
                    ConductorContext::Leader { index: i }
                } else {
                    ConductorContext::Follower { index: i }
                }
            } else {
                ConductorContext::None
            };

            if i == 0 {
                tracing::info!(
                    has_conductor = sequencer.op_conductor.is_some(),
                    "Starting primary sequencer node (op-reth + kona-node)..."
                );
            } else {
                tracing::info!(
                    index = i + 1,
                    has_conductor = sequencer.op_conductor.is_some(),
                    "Starting sequencer node (op-reth + kona-node)..."
                );
            }

            let sequencer_handler = sequencer
                .start(
                    docker,
                    &host_config_path,
                    anvil_handler,
                    None, // Sequencers don't follow another sequencer
                    &mut kona_node_enodes,
                    &mut op_reth_enodes,
                    l1_chain_id,
                    conductor_context,
                    None, // Sequencers get flashblocks URL from their own op-rbuilder
                )
                .await
                .context(format!("Failed to start sequencer node {}", i + 1))?;

            sequencer_handlers.push(sequencer_handler);
        }

        // Get the primary sequencer's RPC URL for validators to follow
        let sequencer_rpc = sequencer_handlers[0].op_reth.http_rpc_url.clone();

        // Get the primary sequencer's flashblocks relay URL for validators (if flashblocks enabled)
        let sequencer_flashblocks_relay_url = sequencer_handlers[0]
            .kona_node
            .flashblocks_relay_url
            .as_ref();

        // Start validator nodes (no conductors)
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
                    l1_chain_id,
                    ConductorContext::None,
                    sequencer_flashblocks_relay_url,
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
            conductors_started = sequencer_handlers.iter().filter(|s| s.op_conductor.is_some()).count(),
            "All L2 nodes started with P2P peer discovery"
        );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_sequencer_no_conductor() {
        let stack = L2StackBuilder::with_counts(1, 0);

        assert_eq!(stack.sequencers.len(), 1);
        assert_eq!(stack.validators.len(), 0);
        assert!(!stack.needs_conductor());
        assert!(stack.sequencers[0].op_conductor.is_none());
    }

    #[test]
    fn test_multi_sequencer_has_conductors() {
        let stack = L2StackBuilder::with_counts(2, 0);

        assert_eq!(stack.sequencers.len(), 2);
        assert!(stack.needs_conductor());

        // Both sequencers should have conductor configs
        assert!(stack.sequencers[0].op_conductor.is_some());
        assert!(stack.sequencers[1].op_conductor.is_some());

        // Verify unique conductor container names
        let conductor_0 = stack.sequencers[0].op_conductor.as_ref().unwrap();
        let conductor_1 = stack.sequencers[1].op_conductor.as_ref().unwrap();
        assert_ne!(conductor_0.container_name, conductor_1.container_name);
    }

    #[test]
    fn test_three_sequencers_all_have_conductors() {
        let stack = L2StackBuilder::with_counts(3, 0);

        assert_eq!(stack.sequencers.len(), 3);
        assert!(stack.needs_conductor());

        // All sequencers should have conductor configs
        for sequencer in &stack.sequencers {
            assert!(sequencer.op_conductor.is_some());
        }

        // Verify all conductor container names are unique
        let conductor_names: Vec<_> = stack
            .sequencers
            .iter()
            .map(|s| s.op_conductor.as_ref().unwrap().container_name.clone())
            .collect();
        let unique_names: std::collections::HashSet<_> = conductor_names.iter().collect();
        assert_eq!(conductor_names.len(), unique_names.len());
    }

    #[test]
    fn test_validators_never_have_conductors() {
        let stack = L2StackBuilder::with_counts(2, 3);

        assert_eq!(stack.sequencers.len(), 2);
        assert_eq!(stack.validators.len(), 3);

        // Sequencers should have conductors
        for sequencer in &stack.sequencers {
            assert!(sequencer.op_conductor.is_some());
        }

        // Validators should never have conductors
        for validator in &stack.validators {
            assert!(validator.op_conductor.is_none());
        }
    }

    #[test]
    fn test_add_sequencer_adds_conductors_retroactively() {
        // Start with single sequencer (no conductor)
        let stack = L2StackBuilder::default();
        assert_eq!(stack.sequencers.len(), 1);
        assert!(!stack.needs_conductor());
        assert!(stack.sequencers[0].op_conductor.is_none());

        // Add a second sequencer - should retroactively add conductors to all
        let stack = stack.add_sequencer();
        assert_eq!(stack.sequencers.len(), 2);
        assert!(stack.needs_conductor());

        // Both sequencers should now have conductors
        assert!(stack.sequencers[0].op_conductor.is_some());
        assert!(stack.sequencers[1].op_conductor.is_some());
    }

    #[test]
    fn test_add_validator_no_conductor() {
        let stack = L2StackBuilder::default().add_validator();

        assert_eq!(stack.sequencers.len(), 1);
        assert_eq!(stack.validators.len(), 1);
        assert!(!stack.needs_conductor());

        // Validator should not have conductor
        assert!(stack.validators[0].op_conductor.is_none());
    }

    #[test]
    fn test_with_node_count_single_node() {
        let stack = L2StackBuilder::with_node_count(1);

        assert_eq!(stack.sequencers.len(), 1);
        assert_eq!(stack.validators.len(), 0);
        assert!(!stack.needs_conductor());
    }

    #[test]
    fn test_with_node_count_multiple_nodes() {
        let stack = L2StackBuilder::with_node_count(4);

        // 1 sequencer + 3 validators
        assert_eq!(stack.sequencers.len(), 1);
        assert_eq!(stack.validators.len(), 3);
        assert_eq!(stack.node_count(), 4);
        assert!(!stack.needs_conductor());
    }

    #[test]
    fn test_primary_sequencer() {
        let stack = L2StackBuilder::with_counts(3, 2);

        let primary = stack.primary_sequencer();
        assert_eq!(primary.op_reth.container_name, "kupcake-op-reth");
    }

    #[test]
    fn test_sequencer_container_name_suffixes() {
        let stack = L2StackBuilder::with_counts(3, 0);

        // First sequencer has no suffix
        assert_eq!(stack.sequencers[0].op_reth.container_name, "kupcake-op-reth");

        // Subsequent sequencers have suffixes
        assert!(stack.sequencers[1]
            .op_reth
            .container_name
            .contains("sequencer-1"));
        assert!(stack.sequencers[2]
            .op_reth
            .container_name
            .contains("sequencer-2"));
    }

    #[test]
    fn test_validator_container_name_suffixes() {
        let stack = L2StackBuilder::with_counts(1, 3);

        // All validators have numbered suffixes
        assert!(stack.validators[0]
            .op_reth
            .container_name
            .contains("validator-1"));
        assert!(stack.validators[1]
            .op_reth
            .container_name
            .contains("validator-2"));
        assert!(stack.validators[2]
            .op_reth
            .container_name
            .contains("validator-3"));
    }
}
