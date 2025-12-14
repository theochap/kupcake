//! L2 Node combining op-reth execution client and kona-node consensus client.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::docker::KupDocker;

use super::{
    anvil::AnvilHandler,
    kona_node::{KonaNodeBuilder, KonaNodeHandler},
    op_reth::{OpRethBuilder, OpRethHandler},
};

/// Role of an L2 node in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum L2NodeRole {
    /// Sequencer node that produces blocks.
    Sequencer,
    /// Validator node that follows the sequencer.
    #[default]
    Validator,
}

impl L2NodeRole {
    /// Returns the kona-node mode string for this role.
    pub fn as_kona_mode(&self) -> &'static str {
        match self {
            L2NodeRole::Sequencer => "Sequencer",
            L2NodeRole::Validator => "Validator",
        }
    }
}

/// Configuration for an L2 node (op-reth + kona-node pair).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2NodeBuilder {
    /// Role of this node (sequencer or validator).
    pub role: L2NodeRole,
    /// Configuration for op-reth execution client.
    pub op_reth: OpRethBuilder,
    /// Configuration for kona-node consensus client.
    pub kona_node: KonaNodeBuilder,
}

impl Default for L2NodeBuilder {
    fn default() -> Self {
        Self {
            role: L2NodeRole::Sequencer,
            op_reth: OpRethBuilder::default(),
            kona_node: KonaNodeBuilder::default(),
        }
    }
}

impl L2NodeBuilder {
    /// Create a new L2 node builder with the given role.
    pub fn new(role: L2NodeRole) -> Self {
        Self {
            role,
            ..Default::default()
        }
    }

    /// Create a sequencer node builder.
    pub fn sequencer() -> Self {
        Self::new(L2NodeRole::Sequencer)
    }

    /// Create a validator node builder.
    pub fn validator() -> Self {
        Self::new(L2NodeRole::Validator)
    }

    /// Set a unique suffix for container names to avoid conflicts.
    pub fn with_name_suffix(mut self, suffix: &str) -> Self {
        self.op_reth.container_name = format!("{}-{}", self.op_reth.container_name, suffix);
        self.kona_node.container_name = format!("{}-{}", self.kona_node.container_name, suffix);
        self
    }

    /// Generate a JWT secret for authenticated communication between op-reth and kona-node.
    fn generate_jwt_secret() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let secret: [u8; 32] = rng.random();
        hex::encode(secret)
    }

    /// Write a JWT secret to a file for this node pair.
    async fn write_jwt_secret(
        host_config_path: &PathBuf,
        node_id: &str,
    ) -> Result<String, anyhow::Error> {
        let jwt_secret = Self::generate_jwt_secret();
        let jwt_filename = format!("jwt-{}.hex", node_id);
        let jwt_path = host_config_path.join(&jwt_filename);

        tokio::fs::write(&jwt_path, &jwt_secret)
            .await
            .context(format!(
                "Failed to write JWT secret file: {}",
                jwt_path.display()
            ))?;

        tracing::debug!(path = ?jwt_path, node_id, "JWT secret written for L2 node");
        Ok(jwt_filename)
    }

    /// Start the L2 node (op-reth + kona-node).
    ///
    /// For sequencer nodes, this starts with block production enabled.
    /// For validator nodes, this connects to the sequencer for block syncing.
    ///
    /// After starting, this fetches the node's ENRs and adds them to the peer lists
    /// so that subsequent nodes can use them as bootnodes.
    ///
    /// # Arguments
    /// * `docker` - Docker client
    /// * `host_config_path` - Path on host for config files
    /// * `anvil_handler` - Handler for the L1 Anvil instance
    /// * `sequencer_rpc` - Optional URL of the sequencer's kona-node RPC (required for validators)
    /// * `kona_node_enrs` - Mutable list of kona-node ENRs for P2P discovery. The node's ENR will be added after startup.
    /// * `op_reth_enrs` - Mutable list of op-reth ENRs for P2P discovery. The node's ENR will be added after startup.
    pub async fn start(
        &self,
        docker: &mut KupDocker,
        host_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
        sequencer_rpc: Option<&Url>,
        kona_node_enrs: &mut Vec<String>,
        op_reth_enrs: &mut Vec<String>,
    ) -> Result<L2NodeHandler, anyhow::Error> {
        // Generate a unique JWT secret for this node pair
        // Use the op-reth container name as the node ID for uniqueness
        let jwt_filename =
            Self::write_jwt_secret(host_config_path, &self.op_reth.container_name).await?;

        // Start op-reth first, passing existing op-reth ENRs as bootnodes
        let op_reth_handler = self
            .op_reth
            .start(
                docker,
                host_config_path,
                sequencer_rpc,
                &jwt_filename,
                op_reth_enrs,
            )
            .await?;

        // Fetch op-reth's enode via the admin_nodeInfo RPC endpoint and add it to the peer list
        // for subsequent op-reth nodes to use as a bootnode
        let op_reth_enr = op_reth_handler
            .fetch_enode(docker)
            .await
            .context("Failed to fetch enode from op-reth")?;

        tracing::info!(
            container_name = %op_reth_handler.container_name,
            enr = %op_reth_enr,
            "Added op-reth enode to peer list"
        );
        op_reth_enrs.push(op_reth_enr);

        // Give op-reth a moment to initialize before starting kona-node
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Start kona-node with the appropriate mode, using the same JWT
        // Pass the current kona-node ENRs as bootnodes
        let kona_node_handler = self
            .kona_node
            .start(
                docker,
                host_config_path,
                anvil_handler,
                &op_reth_handler,
                self.role,
                &jwt_filename,
                kona_node_enrs,
            )
            .await?;

        // Fetch kona-node's ENR via the opp2p_self RPC endpoint and add it to the peer list
        // for subsequent kona-nodes to use as a bootnode
        let kona_node_enr = kona_node_handler
            .fetch_enr(docker)
            .await
            .context("Failed to fetch ENR from kona-node")?;

        tracing::info!(
            container_name = %kona_node_handler.container_name,
            enr = %kona_node_enr,
            "Added kona-node ENR to peer list"
        );
        kona_node_enrs.push(kona_node_enr);

        Ok(L2NodeHandler {
            role: self.role,
            op_reth: op_reth_handler,
            kona_node: kona_node_handler,
        })
    }
}

/// Handler for a running L2 node (op-reth + kona-node pair).
pub struct L2NodeHandler {
    /// Role of this node.
    pub role: L2NodeRole,
    /// Handler for the op-reth execution client.
    pub op_reth: OpRethHandler,
    /// Handler for the kona-node consensus client.
    pub kona_node: KonaNodeHandler,
}

impl L2NodeHandler {
    /// Returns true if this is a sequencer node.
    pub fn is_sequencer(&self) -> bool {
        self.role == L2NodeRole::Sequencer
    }

    /// Returns true if this is a validator node.
    pub fn is_validator(&self) -> bool {
        self.role == L2NodeRole::Validator
    }
}
