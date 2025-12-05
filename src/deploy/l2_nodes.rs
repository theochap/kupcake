//! L2 nodes orchestration for the OP Stack.
//!
//! This module coordinates the startup of all L2 node components.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::deploy::{
    docker::KupDocker,
    fs::FsHandler,
    services::{
        anvil::AnvilHandler,
        kona_node::{KonaNodeConfig, KonaNodeHandler},
        op_batcher::{OpBatcherConfig, OpBatcherHandler},
        op_challenger::{OpChallengerConfig, OpChallengerHandler},
        op_proposer::{OpProposerConfig, OpProposerHandler},
        op_reth::{OpRethConfig, OpRethHandler},
    },
};

/// Combined configuration for all L2 node components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2NodesConfig {
    /// Configuration for op-reth execution client.
    pub op_reth: OpRethConfig,
    /// Configuration for kona-node consensus client.
    pub kona_node: KonaNodeConfig,
    /// Configuration for op-batcher.
    pub op_batcher: OpBatcherConfig,
    /// Configuration for op-proposer.
    pub op_proposer: OpProposerConfig,
    /// Configuration for op-challenger.
    pub op_challenger: OpChallengerConfig,
}

impl Default for L2NodesConfig {
    fn default() -> Self {
        Self {
            op_reth: OpRethConfig::default(),
            kona_node: KonaNodeConfig::default(),
            op_batcher: OpBatcherConfig::default(),
            op_proposer: OpProposerConfig::default(),
            op_challenger: OpChallengerConfig::default(),
        }
    }
}

/// Handler for the complete L2 node setup.
pub struct L2NodesHandler {
    pub op_reth: OpRethHandler,
    pub kona_node: KonaNodeHandler,
    pub op_batcher: OpBatcherHandler,
    pub op_proposer: OpProposerHandler,
    pub op_challenger: OpChallengerHandler,
}

impl L2NodesConfig {
    /// Generate a JWT secret for authenticated communication between op-reth and kona-node.
    fn generate_jwt_secret() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let secret: [u8; 32] = rng.random();
        hex::encode(secret)
    }

    /// Write the JWT secret to a file.
    async fn write_jwt_secret(host_config_path: &PathBuf) -> Result<PathBuf, anyhow::Error> {
        let jwt_secret = Self::generate_jwt_secret();
        let jwt_path = host_config_path.join("jwt.hex");

        tokio::fs::write(&jwt_path, &jwt_secret)
            .await
            .context("Failed to write JWT secret file")?;

        tracing::debug!(path = ?jwt_path, "JWT secret written");
        Ok(jwt_path)
    }

    /// Start all L2 node components.
    ///
    /// This starts op-reth first (execution client), then kona-node (consensus client),
    /// followed by op-batcher (batch submitter), op-proposer, and op-challenger.
    /// The components communicate via the Engine API using JWT authentication.
    pub async fn start(
        self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
    ) -> Result<L2NodesHandler, anyhow::Error> {
        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Generate JWT secret for Engine API authentication
        Self::write_jwt_secret(&host_config_path).await?;

        tracing::info!("Starting op-reth execution client...");

        // Start op-reth first
        let op_reth_handler = self.op_reth.start(docker, &host_config_path).await?;

        // Give op-reth a moment to initialize before starting kona-node
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        tracing::info!("Starting kona-node consensus client...");

        // Start kona-node
        let kona_node_handler = self
            .kona_node
            .start(docker, &host_config_path, anvil_handler, &op_reth_handler)
            .await?;

        // Give kona-node a moment to initialize before starting op-batcher
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        tracing::info!("Starting op-batcher...");

        // Start op-batcher
        let op_batcher_handler = self
            .op_batcher
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &op_reth_handler,
                &kona_node_handler,
            )
            .await?;

        tracing::info!("Starting op-proposer...");

        // Start op-proposer
        let op_proposer_handler = self
            .op_proposer
            .start(docker, &host_config_path, anvil_handler, &kona_node_handler)
            .await?;

        tracing::info!("Starting op-challenger...");

        // Start op-challenger
        let op_challenger_handler = self
            .op_challenger
            .start(
                docker,
                &host_config_path,
                anvil_handler,
                &kona_node_handler,
                &self.op_reth,
            )
            .await?;

        tracing::info!(
            l2_http_rpc = %op_reth_handler.http_rpc_url,
            l2_ws_rpc = %op_reth_handler.ws_rpc_url,
            kona_node_rpc = %kona_node_handler.rpc_url,
            op_batcher_rpc = %op_batcher_handler.rpc_url,
            op_proposer_rpc = %op_proposer_handler.rpc_url,
            op_challenger_rpc = %op_challenger_handler.rpc_url,
            "L2 nodes started successfully"
        );

        Ok(L2NodesHandler {
            op_reth: op_reth_handler,
            kona_node: kona_node_handler,
            op_batcher: op_batcher_handler,
            op_proposer: op_proposer_handler,
            op_challenger: op_challenger_handler,
        })
    }
}
