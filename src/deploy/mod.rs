//! This module deploys the network once the CLI inputs have been parsed and validated.

use alloy_core::primitives::Bytes;
use anyhow::{Context, Result};
use std::path::PathBuf;

mod anvil;
mod docker;
mod fs;
mod l2_nodes;
mod op_deployer;

pub use anvil::AnvilConfig;
pub use docker::{KupDocker, KupDockerConfig};
pub use l2_nodes::{KonaNodeConfig, L2NodesConfig, OpBatcherConfig, OpRethConfig};
pub use op_deployer::OpDeployerConfig;

pub struct AccountInfo {
    pub address: Bytes,
    pub private_key: Bytes,
}

pub struct Deployer {
    pub l1_chain_id: u64,
    pub l2_chain_id: u64,
    pub outdata: PathBuf,

    pub anvil_config: AnvilConfig,
    pub op_deployer_config: OpDeployerConfig,
    pub docker_config: KupDockerConfig,
    pub l2_nodes_config: L2NodesConfig,
}

impl Deployer {
    pub async fn deploy(self) -> Result<()> {
        tracing::info!("Starting deployment process...");

        // Initialize Docker client
        let mut docker = KupDocker::new(self.docker_config)
            .await
            .context("Failed to initialize Docker client")?;

        tracing::info!(
            anvil_config = ?self.anvil_config,
            "Starting Anvil..."
        );

        let anvil = self
            .anvil_config
            .start(&mut docker, self.outdata.join("anvil"), self.l1_chain_id)
            .await?;

        tracing::info!("Deploying L1 contracts...");

        // Deploy L1 contracts - the deployer output goes to the same directory
        // that will be used for L2 nodes config (genesis.json, rollup.json)
        let l2_nodes_data_path = self.outdata.join("deployer");

        self.op_deployer_config
            .deploy_contracts(
                &mut docker,
                l2_nodes_data_path.clone(),
                &anvil,
                self.l1_chain_id,
                self.l2_chain_id,
            )
            .await?;

        tracing::info!("Starting L2 nodes (op-reth + kona-node + op-batcher)...");

        let l2_nodes = self
            .l2_nodes_config
            .start(&mut docker, l2_nodes_data_path, &anvil)
            .await
            .context("Failed to start L2 nodes")?;

        tracing::info!("✓ Deployment complete!");
        tracing::info!("");
        tracing::info!("L1 (Anvil) RPC:       {}", anvil.l1_rpc_url);
        tracing::info!("L2 (op-reth) HTTP:    {}", l2_nodes.op_reth.http_rpc_url);
        tracing::info!("L2 (op-reth) WS:      {}", l2_nodes.op_reth.ws_rpc_url);
        tracing::info!("Kona Node RPC:        {}", l2_nodes.kona_node.rpc_url);
        tracing::info!("Op Batcher RPC:       {}", l2_nodes.op_batcher.rpc_url);
        tracing::info!("");

        tracing::info!("Press Ctrl+C to stop all nodes and cleanup.");

        tokio::signal::ctrl_c().await?;

        Ok(())
    }
}
