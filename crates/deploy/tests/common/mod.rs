//! Shared test helpers for integration tests.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use kupcake_deploy::{
    DeployerBuilder, DeploymentResult, DeploymentTarget, KupDocker, OutDataPath, cleanup_by_prefix,
    health, rpc, services::SyncStatus,
};
use rand::Rng;
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::time::{sleep, timeout};

/// Global semaphore to limit concurrent integration tests.
/// Each test deploys Docker containers which consume significant resources,
/// so we limit concurrency to avoid OOM kills and resource exhaustion.
pub static TEST_SEMAPHORE: Semaphore = Semaphore::const_new(5);

// Timeout constants
pub const DEPLOYMENT_TIMEOUT_SECS: u64 = 600;
pub const NODE_READY_TIMEOUT_SECS: u64 = 300;

/// Generate a random L1 chain ID for local testing.
///
/// Uses a range that doesn't conflict with known chains (Mainnet=1, Sepolia=11155111)
/// and is different for each test run. Range: 100000-999999
pub fn generate_random_l1_chain_id() -> u64 {
    rand::rng().random_range(100000..=999999)
}

/// Test setup context containing common test infrastructure.
pub struct TestContext {
    pub l1_chain_id: u64,
    pub network_name: String,
    pub outdata_path: PathBuf,
}

impl TestContext {
    /// Initialize a new test context with random chain ID and unique network name.
    pub fn new(test_prefix: &str) -> Self {
        let l1_chain_id = generate_random_l1_chain_id();
        let network_name = format!("kup-{}-{}", test_prefix, l1_chain_id);
        let base_tmp = std::env::var("KUPCAKE_TEST_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let outdata_path = base_tmp.join(&network_name);

        Self {
            l1_chain_id,
            network_name,
            outdata_path,
        }
    }

    /// Build a standard deployer for testing (uses genesis deployment mode for speed).
    pub async fn build_deployer(&self) -> Result<kupcake_deploy::Deployer> {
        DeployerBuilder::new(self.l1_chain_id)
            .network_name(&self.network_name)
            .outdata(OutDataPath::Path(self.outdata_path.clone()))
            .l2_node_count(2) // 1 sequencer + 1 validator
            .sequencer_count(1)
            .block_time(2)
            .detach(true)
            .dump_state(false)
            .deployment_target(DeploymentTarget::Genesis)
            .build()
            .await
            .context("Failed to build deployer")
    }

    /// Execute a deployment with timeout and error handling.
    ///
    /// Returns the KupDocker and DeploymentResult on success.
    pub async fn deploy(
        &self,
        deployer: kupcake_deploy::Deployer,
    ) -> Result<(KupDocker, DeploymentResult)> {
        let mut docker = KupDocker::new(deployer.docker.clone()).await?;
        let deploy_result = timeout(
            Duration::from_secs(DEPLOYMENT_TIMEOUT_SECS),
            deployer.deploy(&mut docker, false, false),
        )
        .await;

        match deploy_result {
            Ok(Ok(deployment)) => Ok((docker, deployment)),
            Ok(Err(e)) => {
                let _ = cleanup_by_prefix(&self.network_name).await;
                Err(e).context("Deployment failed")
            }
            Err(_) => {
                let _ = cleanup_by_prefix(&self.network_name).await;
                anyhow::bail!(
                    "Deployment timed out after {} seconds",
                    DEPLOYMENT_TIMEOUT_SECS
                )
            }
        }
    }

    /// Get the deployment version hash from the version file.
    pub fn get_deployment_hash(&self) -> Result<String> {
        let version_file_path = self.outdata_path.join("l2-stack/.deployment-version.json");

        if !version_file_path.exists() {
            anyhow::bail!(
                "Deployment version file not found: {}",
                version_file_path.display()
            );
        }

        let version_content = std::fs::read_to_string(&version_file_path)
            .context("Failed to read deployment version file")?;

        let version: Value = serde_json::from_str(&version_content)
            .context("Failed to parse deployment version file")?;

        version["config_hash"]
            .as_str()
            .context("No config_hash in deployment version file")
            .map(String::from)
    }
}

/// Collect sync status from all L2 nodes in a deployment.
pub async fn collect_all_sync_status(deployment: &DeploymentResult) -> Vec<(String, SyncStatus)> {
    let mut statuses = Vec::new();

    for (idx, node) in deployment.l2_stack.all_nodes().enumerate() {
        let label = if node.is_sequencer() {
            if idx == 0 {
                "sequencer".to_string()
            } else {
                format!("sequencer-{}", idx)
            }
        } else {
            format!(
                "validator-{}",
                idx - deployment.l2_stack.sequencers.len() + 1
            )
        };

        match node.kona_node.sync_status().await {
            Ok(status) => {
                tracing::info!(
                    "{}: unsafe_l2={}, safe_l2={}, finalized_l2={}",
                    label,
                    status.unsafe_l2.number,
                    status.safe_l2.number,
                    status.finalized_l2.number
                );
                statuses.push((label, status));
            }
            Err(e) => {
                tracing::info!("Warning: Failed to get status for {}: {}", label, e);
            }
        }
    }

    statuses
}

/// Wait for all L2 nodes in a deployment to be ready.
pub async fn wait_for_all_nodes(deployment: &DeploymentResult) {
    for (idx, node) in deployment.l2_stack.all_nodes().enumerate() {
        let label = if node.is_sequencer() {
            format!("sequencer-{}", idx)
        } else {
            format!("validator-{}", idx - deployment.l2_stack.sequencers.len())
        };

        if let Err(e) = node
            .kona_node
            .wait_until_ready(NODE_READY_TIMEOUT_SECS)
            .await
        {
            tracing::info!("Warning: {} not ready: {}", label, e);
        }
    }
}

/// Wait for all L2 nodes to have block_number > 0 by polling the health check.
/// Validators may take a while to sync from the sequencer, so this is more
/// reliable than a fixed sleep.
pub async fn wait_for_all_nodes_advancing(
    deployer: &kupcake_deploy::Deployer,
    timeout_secs: u64,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let report = health::health_check(deployer).await?;
        let all_advancing = report
            .nodes
            .iter()
            .all(|node| node.execution.block_number.unwrap_or(0) > 0);
        if all_advancing {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "Timed out waiting for all nodes to advance blocks after {}s.\n{}",
                timeout_secs,
                report
            );
        }
        sleep(Duration::from_secs(5)).await;
    }
}

/// Initialize tracing for tests (idempotent).
pub fn init_test_tracing() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init()
        .ok();
}

/// Query sync status from a kona-node RPC URL.
/// Helper for tests that don't have access to deployment result yet.
pub async fn get_sync_status(rpc_url: &str) -> Result<SyncStatus> {
    let client = rpc::create_client()?;
    rpc::json_rpc_call(&client, rpc_url, "optimism_syncStatus", vec![]).await
}

/// Wait for a kona-node to be ready by polling its RPC endpoint.
/// Helper for tests that don't have access to deployment result yet.
pub async fn wait_for_node_ready(rpc_url: &str, timeout_secs: u64) -> Result<()> {
    rpc::wait_until_ready("kona-node", timeout_secs, || async {
        get_sync_status(rpc_url).await.map(|_| ())
    })
    .await
}
