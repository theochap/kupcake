//! This module deploys the network once the CLI inputs have been parsed and validated.

use alloy_core::primitives::Bytes;
use anyhow::{Context, Result};
use std::path::PathBuf;

mod anvil;
mod docker;
mod fs;
mod op_deployer;

pub use anvil::AnvilConfig;
pub use docker::{KupDocker, KupDockerConfig};
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
    pub docker_config: KupDockerConfig,
    pub op_deployer_config: OpDeployerConfig,
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

        self.op_deployer_config
            .deploy_contracts(
                &mut docker,
                self.outdata.join("deployer"),
                &anvil,
                self.l1_chain_id,
                self.l2_chain_id,
            )
            .await?;

        tracing::info!("Starting L2 nodes...");

        tracing::info!("Next steps:");
        tracing::info!("  - Start L2 nodes");
        tracing::info!("  - Configure network");

        // Keep the container running (you'll likely want to handle this differently)
        tracing::info!("Container is running. Press Ctrl+C to stop.");
        tokio::signal::ctrl_c().await?;

        Ok(())
    }
}
