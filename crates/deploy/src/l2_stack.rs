//! L2 Stack configuration and deployment.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    AnvilHandler, KupDocker, OpBatcherBuilder, OpChallengerBuilder, OpConductorBuilder,
    OpProposerBuilder,
    deployer::L2StackHandler,
    fs,
    services::l2_node::L2NodeBuilder,
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

    /// Set the binary path for op-reth for all nodes (sequencers and validators).
    pub fn set_op_reth_binary(mut self, binary_path: impl Into<PathBuf>) -> Self {
        let docker_image = crate::docker::DockerImage::from_binary(binary_path);
        for sequencer in &mut self.sequencers {
            sequencer.op_reth.docker_image = docker_image.clone();
        }
        for validator in &mut self.validators {
            validator.op_reth.docker_image = docker_image.clone();
        }
        self
    }

    /// Set the binary path for kona-node for all nodes (sequencers and validators).
    pub fn set_kona_node_binary(mut self, binary_path: impl Into<PathBuf>) -> Self {
        let docker_image = crate::docker::DockerImage::from_binary(binary_path);
        for sequencer in &mut self.sequencers {
            sequencer.kona_node.docker_image = docker_image.clone();
        }
        for validator in &mut self.validators {
            validator.kona_node.docker_image = docker_image.clone();
        }
        self
    }

    /// Set the binary path for op-batcher.
    pub fn set_op_batcher_binary(mut self, binary_path: impl Into<PathBuf>) -> Self {
        self.op_batcher.docker_image = crate::docker::DockerImage::from_binary(binary_path);
        self
    }

    /// Set the binary path for op-proposer.
    pub fn set_op_proposer_binary(mut self, binary_path: impl Into<PathBuf>) -> Self {
        self.op_proposer.docker_image = crate::docker::DockerImage::from_binary(binary_path);
        self
    }

    /// Set the binary path for op-challenger.
    pub fn set_op_challenger_binary(mut self, binary_path: impl Into<PathBuf>) -> Self {
        self.op_challenger.docker_image = crate::docker::DockerImage::from_binary(binary_path);
        self
    }

    /// Set the binary path for op-conductor for all sequencers that have conductor config.
    pub fn set_op_conductor_binary(mut self, binary_path: impl Into<PathBuf>) -> Self {
        let docker_image = crate::docker::DockerImage::from_binary(binary_path);
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

        // Use trait-based deployment for type-safe, staged deployment

        // Stage 1: Deploy all L2 nodes (sequencers + validators) via L2NodeFleet
        use crate::traits::{l2_fleet::L2NodeFleet, KupcakeService};
        use crate::traits::{L2NodeContext, L2BatchingContext, L2ProposalContext, L2FaultProofContext};

        tracing::info!(
            sequencer_count = self.sequencers.len(),
            validator_count = self.validators.len(),
            "Deploying L2 node fleet..."
        );

        let node_fleet = L2NodeFleet {
            sequencers: self.sequencers.clone(),
            validators: self.validators.clone(),
        };

        let node_ctx = L2NodeContext {
            docker,
            outdata: host_config_path.clone(),
            anvil: anvil_handler,
            l1_chain_id,
            l2_chain_id: 0, // L2 chain ID is not used by nodes, will be removed in future refactor
        };

        let nodes_result = node_fleet.deploy(node_ctx).await?;

        tracing::info!(
            kona_node_peer_count = nodes_result.kona_node_enodes.len(),
            op_reth_peer_count = nodes_result.op_reth_enodes.len(),
            sequencer_count = nodes_result.sequencers.len(),
            validator_count = nodes_result.validators.len(),
            conductors_started = nodes_result.sequencers.iter().filter(|s| s.op_conductor.is_some()).count(),
            "All L2 nodes started with P2P peer discovery"
        );

        // Get references to the primary sequencer for the remaining components
        let primary_sequencer = &nodes_result.sequencers[0];

        // Stage 2: Deploy op-batcher
        tracing::info!("Starting op-batcher...");

        let batcher_ctx = L2BatchingContext {
            docker,
            outdata: host_config_path.clone(),
            anvil: anvil_handler,
            primary_op_reth: &primary_sequencer.op_reth,
            primary_kona_node: &primary_sequencer.kona_node,
        };

        let op_batcher_handler = self.op_batcher.clone().deploy(batcher_ctx).await?;

        // Stage 3: Deploy op-proposer
        tracing::info!("Starting op-proposer...");

        let proposer_ctx = L2ProposalContext {
            docker,
            outdata: host_config_path.clone(),
            anvil: anvil_handler,
            primary_kona_node: &primary_sequencer.kona_node,
        };

        let op_proposer_handler = self.op_proposer.clone().deploy(proposer_ctx).await?;

        // Stage 4: Deploy op-challenger
        tracing::info!("Starting op-challenger...");

        let challenger_ctx = L2FaultProofContext {
            docker,
            outdata: host_config_path,
            anvil: anvil_handler,
            primary_kona_node: &primary_sequencer.kona_node,
            primary_op_reth: &primary_sequencer.op_reth,
        };

        let op_challenger_handler = self.op_challenger.clone().deploy(challenger_ctx).await?;

        // Log all sequencer endpoints
        for (i, sequencer) in nodes_result.sequencers.iter().enumerate() {
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
        for (i, validator) in nodes_result.validators.iter().enumerate() {
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
            sequencers: nodes_result.sequencers,
            validators: nodes_result.validators,
            op_batcher: op_batcher_handler,
            op_proposer: op_proposer_handler,
            op_challenger: op_challenger_handler,
        })
    }
}

// KupcakeService trait implementation
// NOTE: L2StackBuilder is a composite service that internally uses the new L2 stages.
// It's kept for backward compatibility and convenience, but users can also chain
// individual L2 services directly in the Deployer.
impl crate::traits::KupcakeService for L2StackBuilder {
    type Stage = crate::traits::L2NodeStage;
    type Handler = L2StackHandler;
    type Context<'a> = crate::traits::L2NodeContext<'a>;

    const SERVICE_NAME: &'static str = "l2-stack";

    async fn deploy<'a>(self, ctx: Self::Context<'a>) -> anyhow::Result<Self::Handler>
    where
        Self: 'a,
    {
        let host_config_path = ctx.outdata.join("l2-stack");
        self.start(ctx.docker, host_config_path, ctx.anvil, ctx.l1_chain_id)
            .await
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
