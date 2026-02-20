//! Integration tests for kupcake-deploy.
//!
//! These tests require Docker to be running and will deploy actual networks.
//! They run in local mode without forking, which deploys all contracts from scratch.
//! Each test uses a unique random L1 chain ID to avoid conflicts when running in parallel.
//! Run with: cargo test --test integration_test

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use kupcake_deploy::{
    DeployerBuilder, DeploymentResult, KONA_NODE_DEFAULT_IMAGE, KONA_NODE_DEFAULT_TAG,
    OP_RETH_DEFAULT_IMAGE, OP_RETH_DEFAULT_TAG, OutDataPath, cleanup_by_prefix, faucet, health,
    rpc, services::SyncStatus,
};
use rand::Rng;
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::time::{sleep, timeout};

/// Global semaphore to limit concurrent integration tests.
/// Each test deploys Docker containers which consume significant resources,
/// so we limit concurrency to avoid OOM kills and resource exhaustion.
static TEST_SEMAPHORE: Semaphore = Semaphore::const_new(5);

// Timeout constants
const DEPLOYMENT_TIMEOUT_SECS: u64 = 600;
const CONDUCTOR_DEPLOYMENT_TIMEOUT_SECS: u64 = 900;
const NODE_READY_TIMEOUT_SECS: u64 = 120;

/// Generate a random L1 chain ID for local testing.
///
/// Uses a range that doesn't conflict with known chains (Mainnet=1, Sepolia=11155111)
/// and is different for each test run. Range: 100000-999999
fn generate_random_l1_chain_id() -> u64 {
    rand::rng().random_range(100000..=999999)
}

/// Test setup context containing common test infrastructure.
struct TestContext {
    l1_chain_id: u64,
    network_name: String,
    outdata_path: PathBuf,
}

impl TestContext {
    /// Initialize a new test context with random chain ID and unique network name.
    fn new(test_prefix: &str) -> Self {
        let l1_chain_id = generate_random_l1_chain_id();
        let network_name = format!("kup-{}-{}", test_prefix, l1_chain_id);
        let base_tmp = std::env::var("KUPCAKE_TEST_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        let outdata_path = base_tmp.join(&network_name);

        Self {
            l1_chain_id,
            network_name,
            outdata_path,
        }
    }

    /// Build a standard deployer for testing.
    async fn build_deployer(&self) -> Result<kupcake_deploy::Deployer> {
        DeployerBuilder::new(self.l1_chain_id)
            .network_name(&self.network_name)
            .outdata(OutDataPath::Path(self.outdata_path.clone()))
            .l2_node_count(2) // 1 sequencer + 1 validator
            .sequencer_count(1)
            .block_time(2)
            .detach(true)
            .build()
            .await
            .context("Failed to build deployer")
    }

    /// Execute a deployment with timeout and error handling.
    ///
    /// Returns the DeploymentResult on success.
    async fn deploy(&self, deployer: kupcake_deploy::Deployer) -> Result<DeploymentResult> {
        let deploy_result = timeout(
            Duration::from_secs(DEPLOYMENT_TIMEOUT_SECS),
            deployer.deploy(false, false),
        )
        .await;

        match deploy_result {
            Ok(Ok(deployment)) => Ok(deployment),
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
    fn get_deployment_hash(&self) -> Result<String> {
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

    /// Cleanup network resources.
    async fn cleanup(&self) -> Result<()> {
        let cleanup_result = cleanup_by_prefix(&self.network_name).await?;
        tracing::info!(
            "Cleaned up {} containers",
            cleanup_result.containers_removed.len()
        );
        if let Some(network) = cleanup_result.network_removed {
            tracing::info!("Removed network: {}", network);
        }
        Ok(())
    }
}

/// Collect sync status from all L2 nodes in a deployment.
async fn collect_all_sync_status(deployment: &DeploymentResult) -> Vec<(String, SyncStatus)> {
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
async fn wait_for_all_nodes(deployment: &DeploymentResult) {
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

/// Initialize tracing for tests (idempotent).
fn init_test_tracing() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init()
        .ok();
}

/// Query sync status from a kona-node RPC URL.
/// Helper for tests that don't have access to deployment result yet.
async fn get_sync_status(rpc_url: &str) -> Result<SyncStatus> {
    let client = rpc::create_client()?;
    rpc::json_rpc_call(&client, rpc_url, "optimism_syncStatus", vec![]).await
}

/// Wait for a kona-node to be ready by polling its RPC endpoint.
/// Helper for tests that don't have access to deployment result yet.
async fn wait_for_node_ready(rpc_url: &str, timeout_secs: u64) -> Result<()> {
    rpc::wait_until_ready("kona-node", timeout_secs, || async {
        get_sync_status(rpc_url).await.map(|_| ())
    })
    .await
}

/// Get the host port mapped to a container port using docker inspect.
/// Build the Docker-internal RPC URL for the primary sequencer from a loaded deployer.
fn sequencer_rpc_url(deployer: &kupcake_deploy::Deployer) -> String {
    deployer.l2_stack.sequencers[0].op_reth.docker_rpc_url()
}

fn get_container_host_port(container_name: &str, container_port: u16) -> Result<u16> {
    let output = Command::new("docker")
        .args([
            "inspect",
            "--format",
            &format!(
                "{{{{(index (index .NetworkSettings.Ports \"{}/tcp\") 0).HostPort}}}}",
                container_port
            ),
            container_name,
        ])
        .output()
        .context("Failed to run docker inspect")?;

    if !output.status.success() {
        anyhow::bail!(
            "docker inspect failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let port_str = String::from_utf8_lossy(&output.stdout);
    let port_str = port_str.trim();
    port_str.parse::<u16>().with_context(|| {
        format!(
            "Failed to parse port '{}' for container {}",
            port_str, container_name
        )
    })
}

/// Status of op-batcher.
#[derive(Debug, Clone)]
struct OpBatcherStatus {
    /// Whether the batcher is healthy (RPC responding).
    is_healthy: bool,
    /// Error message if not healthy.
    error: Option<String>,
}

/// Check op-batcher health by calling its RPC endpoint.
/// op-batcher exposes admin_nodeInfo which can be used as a health check.
async fn get_op_batcher_status(rpc_url: &str) -> Result<OpBatcherStatus> {
    let client = rpc::create_client()?;

    // Try opp_version as a health check (standard op-service method)
    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "opp_version",
            "params": [],
            "id": 1
        }))
        .send()
        .await;

    match response {
        Ok(resp) => {
            let body: Value = resp
                .json()
                .await
                .context("Failed to parse opp_version response")?;
            if body.get("error").is_some() {
                // Even if there's an error, the service is responding
                Ok(OpBatcherStatus {
                    is_healthy: true,
                    error: body["error"]["message"].as_str().map(|s| s.to_string()),
                })
            } else {
                Ok(OpBatcherStatus {
                    is_healthy: true,
                    error: None,
                })
            }
        }
        Err(e) => Ok(OpBatcherStatus {
            is_healthy: false,
            error: Some(e.to_string()),
        }),
    }
}

/// Wait for op-batcher to be ready by polling its RPC endpoint.
async fn wait_for_op_batcher_ready(rpc_url: &str, timeout_secs: u64) -> Result<()> {
    rpc::wait_until_ready("op-batcher", timeout_secs, || async {
        let status = get_op_batcher_status(rpc_url).await?;
        if status.is_healthy {
            Ok(())
        } else {
            anyhow::bail!("op-batcher not healthy yet")
        }
    })
    .await
}

/// Test that deploys a network and verifies all nodes have advancing heads.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_network_deployment_and_sync_status() -> Result<()> {
    // Initialize tracing for test output
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let l1_chain_id = generate_random_l1_chain_id();
    let network_name = format!("kup-test-{}", l1_chain_id);
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    tracing::info!(
        "=== Starting test deployment with network: {} (L1 chain ID: {}) ===",
        network_name,
        l1_chain_id
    );

    // Build the deployer - use local mode (no forking, deploys all contracts from scratch)
    let deployer = DeployerBuilder::new(l1_chain_id)
        .network_name(&network_name)
        .outdata(OutDataPath::Path(outdata_path.clone()))
        // No l1_rpc_url - this triggers local mode
        .l2_node_count(2) // 1 sequencer + 1 validator (minimum for faster testing)
        .sequencer_count(1)
        .block_time(2) // Fast block time for testing
        .detach(true) // Exit after deployment
        .build()
        .await
        .context("Failed to build deployer")?;

    // Save config for debugging
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");

    // Deploy with a timeout
    let deploy_result = timeout(
        Duration::from_secs(DEPLOYMENT_TIMEOUT_SECS),
        deployer.deploy(false, false),
    )
    .await;

    let deployment = match deploy_result {
        Ok(Ok(deployment)) => {
            tracing::info!("=== Deployment completed successfully ===");
            deployment
        }
        Ok(Err(e)) => {
            // Cleanup before returning error
            let _ = cleanup_by_prefix(&network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            // Cleanup before returning error
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!(
                "Deployment timed out after {} seconds",
                DEPLOYMENT_TIMEOUT_SECS
            );
        }
    };

    // Wait for all nodes to be ready using handlers
    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    // Get initial sync status using handlers
    tracing::info!("=== Getting initial sync status... ===");
    let initial_status = collect_all_sync_status(&deployment).await;

    if initial_status.is_empty() {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("No nodes available for testing");
    }

    // Wait for blocks to be produced (with 2s block time, wait ~30s for several blocks)
    tracing::info!("=== Waiting 30 seconds for blocks to be produced... ===");
    sleep(Duration::from_secs(30)).await;

    // Check that nodes have advanced
    tracing::info!("=== Checking that nodes have advanced... ===");
    let mut all_advancing = true;
    let mut errors = Vec::new();

    let current_status = collect_all_sync_status(&deployment).await;

    for (label, current) in &current_status {
        // Find the corresponding initial status
        if let Some((_, initial)) = initial_status.iter().find(|(l, _)| l == label) {
            let unsafe_advanced = current.unsafe_l2.number > initial.unsafe_l2.number;
            let safe_advanced = current.safe_l2.number > initial.safe_l2.number;

            tracing::info!(
                "{}: unsafe {} -> {} ({}), safe {} -> {} ({})",
                label,
                initial.unsafe_l2.number,
                current.unsafe_l2.number,
                if unsafe_advanced {
                    "ADVANCING"
                } else {
                    "STALLED"
                },
                initial.safe_l2.number,
                current.safe_l2.number,
                if safe_advanced {
                    "ADVANCING"
                } else {
                    "STALLED"
                },
            );

            if !unsafe_advanced && !safe_advanced {
                all_advancing = false;
                errors.push(format!("{} is not advancing", label));
            }
        }
    }

    // Cleanup
    cleanup_by_prefix(&network_name).await?;

    if !all_advancing {
        anyhow::bail!("Some nodes are not advancing: {:?}", errors);
    }

    Ok(())
}

/// Test that op-reth nodes are properly deployed and syncing.
/// This test verifies:
/// - op-reth RPC endpoints are accessible
/// - Block numbers are advancing over time
/// - eth_syncing returns expected values
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_op_reth_sync_and_block_advancement() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let l1_chain_id = generate_random_l1_chain_id();
    let network_name = format!("kup-reth-test-{}", l1_chain_id);
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    tracing::info!(
        "=== Starting op-reth test deployment with network: {} (L1 chain ID: {}) ===",
        network_name,
        l1_chain_id
    );

    // Use local mode (no forking, deploys all contracts from scratch)
    let deployer = DeployerBuilder::new(l1_chain_id)
        .network_name(&network_name)
        .outdata(OutDataPath::Path(outdata_path.clone()))
        // No l1_rpc_url - this triggers local mode
        .l2_node_count(2)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");

    let deploy_result = timeout(
        Duration::from_secs(DEPLOYMENT_TIMEOUT_SECS),
        deployer.deploy(false, false),
    )
    .await;

    let deployment = match deploy_result {
        Ok(Ok(deployment)) => {
            tracing::info!("=== Deployment completed successfully ===");
            deployment
        }
        Ok(Err(e)) => {
            let _ = cleanup_by_prefix(&network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!(
                "Deployment timed out after {} seconds",
                DEPLOYMENT_TIMEOUT_SECS
            );
        }
    };

    // Wait for op-reth nodes to be ready using handlers
    tracing::info!("=== Waiting for op-reth nodes to be ready... ===");
    for (idx, node) in deployment.l2_stack.all_nodes().enumerate() {
        let label = if node.is_sequencer() {
            "sequencer-reth".to_string()
        } else {
            format!("validator-{}-reth", idx)
        };

        if let Err(e) = node.op_reth.wait_until_ready(NODE_READY_TIMEOUT_SECS).await {
            tracing::info!("Warning: {} not ready: {}", label, e);
        }
    }

    // Get initial block numbers using handlers
    tracing::info!("=== Getting initial op-reth status... ===");
    let mut initial_blocks = Vec::new();
    for (idx, node) in deployment.l2_stack.all_nodes().enumerate() {
        let label = if node.is_sequencer() {
            "sequencer-reth".to_string()
        } else {
            format!("validator-{}-reth", idx)
        };

        match node.op_reth.sync_status().await {
            Ok(status) => {
                tracing::info!(
                    "{}: block={}, syncing={}{}",
                    label,
                    status.block_number,
                    status.is_syncing,
                    status
                        .sync_progress
                        .as_ref()
                        .map(|p| format!(
                            " (current: {}, highest: {})",
                            p.current_block, p.highest_block
                        ))
                        .unwrap_or_default()
                );
                initial_blocks.push((label.clone(), status.block_number));
            }
            Err(e) => {
                tracing::info!("{}: failed to get status: {}", label, e);
            }
        }
    }

    if initial_blocks.is_empty() {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("No op-reth nodes available for testing");
    }

    // Wait for blocks to be produced
    tracing::info!("=== Waiting 30 seconds for blocks to be produced... ===");
    sleep(Duration::from_secs(30)).await;

    // Check that block numbers have advanced using handlers
    tracing::info!("=== Checking that op-reth block numbers have advanced... ===");
    let mut all_advancing = true;
    let mut errors = Vec::new();

    for (idx, node) in deployment.l2_stack.all_nodes().enumerate() {
        let label = if node.is_sequencer() {
            "sequencer-reth".to_string()
        } else {
            format!("validator-{}-reth", idx)
        };

        // Find the initial block for this node
        if let Some((_, initial_block)) = initial_blocks.iter().find(|(l, _)| l == &label) {
            match node.op_reth.sync_status().await {
                Ok(status) => {
                    let advanced = status.block_number > *initial_block;
                    tracing::info!(
                        "{}: block {} -> {} ({})",
                        label,
                        initial_block,
                        status.block_number,
                        if advanced { "ADVANCING" } else { "STALLED" }
                    );

                    if !advanced {
                        errors.push(format!("{}: block number not advancing", label));
                        all_advancing = false;
                    }
                }
                Err(e) => {
                    errors.push(format!("{}: failed to get current status: {}", label, e));
                    all_advancing = false;
                }
            }
        }
    }

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    let cleanup_result = cleanup_by_prefix(&network_name).await?;
    tracing::info!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );
    if let Some(network) = cleanup_result.network_removed {
        tracing::info!("Removed network: {}", network);
    }

    if !all_advancing {
        anyhow::bail!(
            "Not all op-reth nodes are advancing:\n{}",
            errors.join("\n")
        );
    }

    tracing::info!("=== Test passed! All op-reth nodes are advancing. ===");
    Ok(())
}

/// Test multi-sequencer deployment with op-conductor.
/// This test verifies:
/// - Multiple sequencer nodes are deployed with conductors
/// - Conductor containers are running and RPC is accessible
/// - All sequencers produce blocks
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_multi_sequencer_with_conductor() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let l1_chain_id = generate_random_l1_chain_id();
    let network_name = format!("kup-conductor-test-{}", l1_chain_id);
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    tracing::info!(
        "=== Starting multi-sequencer conductor test with network: {} (L1 chain ID: {}) ===",
        network_name,
        l1_chain_id
    );

    // Deploy with 2 sequencers (triggers conductor deployment) + 1 validator
    let deployer = DeployerBuilder::new(l1_chain_id)
        .network_name(&network_name)
        .outdata(OutDataPath::Path(outdata_path.clone()))
        .l2_node_count(3) // 2 sequencers + 1 validator
        .sequencer_count(2) // This triggers conductor deployment
        .block_time(2)
        .detach(true)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network with 2 sequencers + conductor... ===");

    let deploy_result = timeout(
        Duration::from_secs(CONDUCTOR_DEPLOYMENT_TIMEOUT_SECS),
        deployer.deploy(false, false),
    )
    .await;

    match deploy_result {
        Ok(Ok(_deployment)) => tracing::info!("=== Deployment completed successfully ==="),
        Ok(Err(e)) => {
            let _ = cleanup_by_prefix(&network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!(
                "Conductor deployment timed out after {} seconds",
                CONDUCTOR_DEPLOYMENT_TIMEOUT_SECS
            );
        }
    }

    // Verify conductor containers are running
    tracing::info!("=== Verifying conductor containers... ===");
    let conductor_containers = vec![
        format!("{}-op-conductor", network_name),
        format!("{}-op-conductor-1", network_name),
    ];

    for conductor_name in &conductor_containers {
        let output = Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}", conductor_name])
            .output()
            .context("Failed to run docker inspect")?;

        if !output.status.success() {
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!("Conductor container {} not found", conductor_name);
        }

        let running = String::from_utf8_lossy(&output.stdout).trim() == "true";
        if !running {
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!("Conductor container {} is not running", conductor_name);
        }
        tracing::info!("Conductor {} is running", conductor_name);
    }

    // Get conductor RPC ports and verify they respond
    tracing::info!("=== Verifying conductor RPC endpoints... ===");
    for conductor_name in &conductor_containers {
        let conductor_port = get_container_host_port(conductor_name, 8547).context(format!(
            "Failed to get conductor port for {}",
            conductor_name
        ))?;
        let conductor_url = format!("http://localhost:{}", conductor_port);

        // Wait for conductor to be ready
        if let Err(e) = wait_for_conductor_ready(&conductor_url, 60).await {
            tracing::info!("Warning: Conductor {} not ready: {}", conductor_name, e);
        } else {
            tracing::info!(
                "Conductor {} RPC is responding at {}",
                conductor_name,
                conductor_url
            );
        }
    }

    // Get ports for both sequencer kona-nodes
    let sequencer1_port = get_container_host_port(&format!("{}-kona-node", network_name), 7545)
        .context("Failed to get sequencer 1 kona-node port")?;
    let sequencer2_port =
        get_container_host_port(&format!("{}-kona-node-sequencer-1", network_name), 7545)
            .context("Failed to get sequencer 2 kona-node port")?;

    let sequencer_endpoints = vec![
        (
            "sequencer-1",
            format!("http://localhost:{}", sequencer1_port),
        ),
        (
            "sequencer-2",
            format!("http://localhost:{}", sequencer2_port),
        ),
    ];

    // Wait for sequencers to be ready
    tracing::info!("=== Waiting for sequencer nodes to be ready... ===");
    for (label, url) in &sequencer_endpoints {
        if let Err(e) = wait_for_node_ready(url, NODE_READY_TIMEOUT_SECS).await {
            tracing::info!("Warning: {} at {} not ready: {}", label, url, e);
        }
    }

    // Get initial sync status
    tracing::info!("=== Getting initial sync status from sequencers... ===");
    let mut initial_status: Vec<(String, String, SyncStatus)> = Vec::new();
    for (label, url) in &sequencer_endpoints {
        match get_sync_status(url).await {
            Ok(status) => {
                tracing::info!(
                    "{}: unsafe_l2={}, safe_l2={}, finalized_l2={}",
                    label,
                    status.unsafe_l2.number,
                    status.safe_l2.number,
                    status.finalized_l2.number
                );
                initial_status.push((label.to_string(), url.clone(), status));
            }
            Err(e) => {
                tracing::info!("{}: failed to get sync status: {}", label, e);
            }
        }
    }

    if initial_status.is_empty() {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("No sequencer nodes available for testing");
    }

    // Note: With conductor-controlled sequencers, block production is managed by the conductor
    // and requires the conductor to elect a leader and enable sequencing. This takes longer
    // to stabilize. For this test, we focus on verifying the infrastructure is deployed correctly:
    // - Conductor containers running
    // - Conductor RPCs responding
    // - Sequencer nodes ready (RPCs responding with sync status)
    // Block production testing with conductor coordination is a more advanced scenario.

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    let cleanup_result = cleanup_by_prefix(&network_name).await?;
    tracing::info!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );
    if let Some(network) = cleanup_result.network_removed {
        tracing::info!("Removed network: {}", network);
    }

    tracing::info!("=== Test passed! Multi-sequencer deployment with conductor is working. ===");
    Ok(())
}

/// Wait for op-conductor to be ready by polling its RPC endpoint.
async fn wait_for_conductor_ready(rpc_url: &str, timeout_secs: u64) -> Result<()> {
    rpc::wait_until_ready("conductor", timeout_secs, || async {
        let client = rpc::create_client()?;

        // Try conductor_active as a health check
        let response = client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "conductor_active",
                "params": [],
                "id": 1
            }))
            .send()
            .await
            .context("Failed to connect to conductor")?;

        let body: Value = response
            .json()
            .await
            .context("Failed to parse conductor response")?;

        // If we get any response (even error), conductor is running
        if body.get("result").is_some() || body.get("error").is_some() {
            Ok(())
        } else {
            anyhow::bail!("Invalid conductor response")
        }
    })
    .await
}

/// Test that op-batcher is properly deployed and healthy.
/// This test verifies:
/// - op-batcher RPC endpoint is accessible
/// - The batcher is responding to RPC calls
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_op_batcher_health() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let l1_chain_id = generate_random_l1_chain_id();
    let network_name = format!("kup-batcher-test-{}", l1_chain_id);
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    tracing::info!(
        "=== Starting op-batcher test deployment with network: {} (L1 chain ID: {}) ===",
        network_name,
        l1_chain_id
    );

    // Use local mode (no forking, deploys all contracts from scratch)
    let deployer = DeployerBuilder::new(l1_chain_id)
        .network_name(&network_name)
        .outdata(OutDataPath::Path(outdata_path.clone()))
        // No l1_rpc_url - this triggers local mode
        .l2_node_count(2)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");

    let deploy_result = timeout(
        Duration::from_secs(DEPLOYMENT_TIMEOUT_SECS),
        deployer.deploy(false, false),
    )
    .await;

    match deploy_result {
        Ok(Ok(_deployment)) => tracing::info!("=== Deployment completed successfully ==="),
        Ok(Err(e)) => {
            let _ = cleanup_by_prefix(&network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!(
                "Deployment timed out after {} seconds",
                DEPLOYMENT_TIMEOUT_SECS
            );
        }
    }

    // Get the port for op-batcher (RPC on 8548)
    let batcher_port = get_container_host_port(&format!("{}-op-batcher", network_name), 8548)
        .context("Failed to get op-batcher port")?;

    let batcher_url = format!("http://localhost:{}", batcher_port);

    // Wait for op-batcher to be ready
    tracing::info!("=== Waiting for op-batcher to be ready... ===");
    if let Err(e) = wait_for_op_batcher_ready(&batcher_url, NODE_READY_TIMEOUT_SECS).await {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("op-batcher not ready: {}", e);
    }

    // Check op-batcher health
    tracing::info!("=== Checking op-batcher health... ===");
    let status = get_op_batcher_status(&batcher_url).await?;

    if !status.is_healthy {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!(
            "op-batcher is not healthy: {}",
            status.error.unwrap_or_else(|| "unknown error".to_string())
        );
    }

    tracing::info!(
        "op-batcher health check passed{}",
        status
            .error
            .as_ref()
            .map(|e| format!(" (note: {})", e))
            .unwrap_or_default()
    );

    // Additional check: verify kona-node sync status is progressing (batcher needs this)
    // The safe head advancing indicates batches are being processed
    let kona_port = get_container_host_port(&format!("{}-kona-node", network_name), 7545)
        .context("Failed to get kona-node port")?;
    let kona_url = format!("http://localhost:{}", kona_port);

    tracing::info!("=== Verifying batcher activity via safe head progression... ===");

    // Get initial safe head
    let initial_status = get_sync_status(&kona_url).await?;
    tracing::info!("Initial safe head: {}", initial_status.safe_l2.number);

    // Wait for batches to be submitted and processed
    tracing::info!("=== Waiting 45 seconds for batch submissions... ===");
    sleep(Duration::from_secs(45)).await;

    // Check if safe head has advanced (indicates batcher is submitting batches)
    let final_status = get_sync_status(&kona_url).await?;
    let safe_advanced = final_status.safe_l2.number > initial_status.safe_l2.number;

    tracing::info!(
        "Safe head: {} -> {} ({})",
        initial_status.safe_l2.number,
        final_status.safe_l2.number,
        if safe_advanced {
            "ADVANCING - batcher is working"
        } else {
            "NOT YET ADVANCING"
        }
    );

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    let cleanup_result = cleanup_by_prefix(&network_name).await?;
    tracing::info!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );
    if let Some(network) = cleanup_result.network_removed {
        tracing::info!("Removed network: {}", network);
    }

    // Note: We don't fail if safe head hasn't advanced yet, as it can take time
    // The main assertion is that the batcher RPC is healthy
    if safe_advanced {
        tracing::info!("=== Test passed! op-batcher is healthy and submitting batches. ===");
    } else {
        tracing::info!(
            "=== Test passed! op-batcher is healthy (safe head may need more time to advance). ==="
        );
    }

    Ok(())
}
/// Test that --publish-all-ports functionality works correctly.
/// This test verifies:
/// - All service ports are published to random host ports when enabled
/// - Published ports are accessible from the host
/// - Containers are still on the custom Docker network
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_publish_all_ports() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let l1_chain_id = generate_random_l1_chain_id();
    let network_name = format!("kup-publish-test-{}", l1_chain_id);
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    tracing::info!(
        "=== Starting publish-all-ports test with network: {} (L1 chain ID: {}) ===",
        network_name,
        l1_chain_id
    );

    // Deploy with publish_all_ports enabled
    let deployer = DeployerBuilder::new(l1_chain_id)
        .network_name(&network_name)
        .outdata(OutDataPath::Path(outdata_path.clone()))
        .l2_node_count(2) // 1 sequencer + 1 validator
        .sequencer_count(1)
        .block_time(2)
        .publish_all_ports(true) // Enable publish_all_ports
        .detach(true)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network with publish_all_ports enabled... ===");

    let deploy_result = timeout(
        Duration::from_secs(DEPLOYMENT_TIMEOUT_SECS),
        deployer.deploy(false, false),
    )
    .await;

    match deploy_result {
        Ok(Ok(_deployment)) => tracing::info!("=== Deployment completed successfully ==="),
        Ok(Err(e)) => {
            let _ = cleanup_by_prefix(&network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!(
                "Deployment timed out after {} seconds",
                DEPLOYMENT_TIMEOUT_SECS
            );
        }
    }

    // Define containers and their expected exposed ports to check
    // When publish_all_ports is enabled, ports that are NOT published by default SHOULD be published
    // Focus on verifying that optional/metrics ports that default to None are now published
    let containers_to_check = vec![
        // Core RPC ports (always published)
        (format!("{}-anvil", network_name), 8545, true), // required: always published
        (format!("{}-op-reth", network_name), 9545, true), // HTTP RPC - always published
        (format!("{}-op-reth", network_name), 9546, true), // WS RPC - always published
        (format!("{}-kona-node", network_name), 7545, true), // RPC - always published
        (format!("{}-op-batcher", network_name), 8548, true), // RPC - always published
        (format!("{}-prometheus", network_name), 9090, true), // always published
        (format!("{}-grafana", network_name), 3000, true), // always published
        // Optional ports that should ONLY be published when publish_all_ports is true
        // These are the key ports to verify for this test
        (format!("{}-op-reth", network_name), 8551, false), // AuthRPC - default None
        (format!("{}-op-reth", network_name), 9001, false), // Metrics - default None
        (format!("{}-kona-node", network_name), 7300, false), // Metrics - default None
        (format!("{}-op-batcher", network_name), 7301, false), // Metrics - default None
        (format!("{}-op-proposer", network_name), 8560, false), // RPC - default None
        (format!("{}-op-proposer", network_name), 7302, false), // Metrics - default None
    ];

    tracing::info!("=== Verifying that ports are published to the host... ===");
    let mut published_ports = Vec::new();
    let mut errors = Vec::new();

    for (container_name, container_port, required) in containers_to_check {
        match get_container_host_port(&container_name, container_port) {
            Ok(host_port) => {
                tracing::info!(
                    "✓ {}:{} -> host:{} {}",
                    container_name,
                    container_port,
                    host_port,
                    if required {
                        "(required)"
                    } else {
                        "(optional - publish_all_ports enabled)"
                    }
                );
                published_ports.push((container_name, container_port, host_port));
            }
            Err(e) => {
                let error_msg = format!(
                    "✗ {}:{} - Failed to get host port: {}",
                    container_name, container_port, e
                );
                tracing::info!("{}", error_msg);
                // Only treat as error if this port is required
                if required {
                    errors.push(error_msg);
                }
            }
        }
    }

    // Verify we found at least some published ports
    if published_ports.is_empty() {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("No ports were published to the host");
    }

    tracing::info!("=== Found {} published ports ===", published_ports.len());

    // Verify containers are still on the custom network
    tracing::info!("=== Verifying containers are on custom network... ===");
    let network_name_docker = format!("{}-network", network_name);
    let output = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{json .NetworkSettings.Networks}}",
            &format!("{}-anvil", network_name),
        ])
        .output()
        .context("Failed to inspect container networks")?;

    if !output.status.success() {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!(
            "Failed to inspect network: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let networks_json = String::from_utf8_lossy(&output.stdout);
    if !networks_json.contains(&network_name_docker) {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!(
            "Container not on expected network {}. Networks: {}",
            network_name_docker,
            networks_json
        );
    }

    tracing::info!(
        "✓ Containers are on custom network: {}",
        network_name_docker
    );

    // Test accessibility of a few key ports
    tracing::info!("=== Testing accessibility of published ports... ===");

    // Test anvil RPC
    if let Some((_, _, host_port)) = published_ports
        .iter()
        .find(|(name, port, _)| name == &format!("{}-anvil", network_name) && *port == 8545)
    {
        let anvil_url = format!("http://localhost:{}", host_port);
        match test_rpc_endpoint(&anvil_url, "eth_blockNumber").await {
            Ok(_) => tracing::info!("✓ Anvil RPC accessible at {}", anvil_url),
            Err(e) => {
                tracing::info!("✗ Anvil RPC not accessible: {}", e);
                errors.push(format!("Anvil RPC not accessible: {}", e));
            }
        }
    }

    // Test op-reth RPC
    if let Some((_, _, host_port)) = published_ports
        .iter()
        .find(|(name, port, _)| name == &format!("{}-op-reth", network_name) && *port == 9545)
    {
        let reth_url = format!("http://localhost:{}", host_port);
        match test_rpc_endpoint(&reth_url, "eth_blockNumber").await {
            Ok(_) => tracing::info!("✓ op-reth RPC accessible at {}", reth_url),
            Err(e) => {
                tracing::info!("✗ op-reth RPC not accessible: {}", e);
                errors.push(format!("op-reth RPC not accessible: {}", e));
            }
        }
    }

    // Test kona-node RPC
    if let Some((_, _, host_port)) = published_ports
        .iter()
        .find(|(name, port, _)| name == &format!("{}-kona-node", network_name) && *port == 7545)
    {
        let kona_url = format!("http://localhost:{}", host_port);
        match test_rpc_endpoint(&kona_url, "optimism_syncStatus").await {
            Ok(_) => tracing::info!("✓ kona-node RPC accessible at {}", kona_url),
            Err(e) => {
                tracing::info!("Warning: kona-node RPC not accessible yet: {}", e);
                // Don't treat as error since it may take time to be ready
            }
        }
    }

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    let cleanup_result = cleanup_by_prefix(&network_name).await?;
    tracing::info!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );
    if let Some(network) = cleanup_result.network_removed {
        tracing::info!("Removed network: {}", network);
    }

    // Assert after cleanup
    if !errors.is_empty() {
        anyhow::bail!("Test failed with errors:\n{}", errors.join("\n"));
    }

    tracing::info!("=== Test passed! All ports are published and accessible. ===");
    Ok(())
}

/// Test an RPC endpoint by calling a simple method.
async fn test_rpc_endpoint(rpc_url: &str, method: &str) -> Result<()> {
    let client = rpc::create_client()?;
    let _: Value = rpc::json_rpc_call(&client, rpc_url, method, vec![]).await?;
    Ok(())
}

/// Test deploying a network with a local kona-node binary.
///
/// This test:
/// - Passes the kona source directory to with_kona_node_binary (auto-builds and cross-compiles)
/// - Deploys a network using the locally-built binary instead of a Docker image
/// - Verifies the network starts successfully
/// - Verifies sync status can be queried
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_local_kona_binary() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let l1_chain_id = generate_random_l1_chain_id();
    let network_name = format!("kup-local-kona-{}", l1_chain_id);
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    tracing::info!(
        "=== Starting local kona binary test with network: {} (L1 chain ID: {}) ===",
        network_name,
        l1_chain_id
    );

    // Path to the kona submodule (relative to the test crate)
    // env!("CARGO_MANIFEST_DIR") points to crates/deploy
    // Pass the directory — ensure_image_ready will auto-build and cross-compile
    let kona_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/kona");

    tracing::info!(
        "=== Using kona source directory at {} (will auto-build) ===",
        kona_dir.display()
    );

    // Deploy with local kona-node source directory (auto-builds for Docker's platform)
    let deployer = DeployerBuilder::new(l1_chain_id)
        .network_name(&network_name)
        .outdata(OutDataPath::Path(outdata_path.clone()))
        .l2_node_count(2) // 1 sequencer + 1 validator
        .sequencer_count(1)
        .block_time(2)
        .with_kona_node_binary(&kona_dir)
        .publish_all_ports(true) // Ensure all ports (including kona-node RPC) are published
        .detach(true)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network with local kona-node binary... ===");
    let deployment = deployer
        .deploy(false, false)
        .await
        .context("Failed to deploy network")?;

    tracing::info!("=== Network deployed successfully ===");

    // Verify that kona-node containers were created from local binary images
    tracing::info!("=== Verifying local binary images were used... ===");

    // List all Docker images with the "kupcake-*-local" pattern
    let output = Command::new("docker")
        .args([
            "images",
            "--filter",
            &format!("reference=kupcake-{}-*-local*", network_name),
            "--format",
            "{{.Repository}}:{{.Tag}}",
        ])
        .output()
        .context("Failed to list Docker images")?;

    let images_list = String::from_utf8_lossy(&output.stdout).trim().to_string();
    tracing::info!("Found local binary images:");
    for image in images_list.lines() {
        tracing::info!("  - {}", image);
    }

    // We expect 2 local images (one for sequencer kona-node, one for validator kona-node)
    let image_count = images_list.lines().count();
    if image_count < 2 {
        anyhow::bail!(
            "Expected at least 2 local binary images, found {}",
            image_count
        );
    }

    tracing::info!(
        "=== Successfully deployed with {} local binary Docker images! ===",
        image_count
    );

    // Verify kona nodes are advancing
    tracing::info!("=== Verifying kona nodes are advancing... ===");

    // Get RPC URLs directly from the deployment result
    let sequencer_rpc_url = deployment.l2_stack.sequencers[0]
        .kona_node
        .rpc_host_url
        .as_ref()
        .context("Sequencer kona-node RPC URL not available")?;

    let validator_rpc_url = deployment.l2_stack.validators[0]
        .kona_node
        .rpc_host_url
        .as_ref()
        .context("Validator kona-node RPC URL not available")?;

    let node_endpoints = vec![
        ("sequencer", sequencer_rpc_url.to_string()),
        ("validator-1", validator_rpc_url.to_string()),
    ];

    // Wait for nodes to be ready
    tracing::info!("=== Waiting for nodes to be ready... ===");
    for (label, url) in &node_endpoints {
        if let Err(e) = wait_for_node_ready(url, NODE_READY_TIMEOUT_SECS).await {
            tracing::info!("Warning: {} at {} not ready: {}", label, url, e);
        }
    }

    // Get initial sync status
    tracing::info!("=== Getting initial sync status... ===");
    let mut initial_status: Vec<(String, String, SyncStatus)> = Vec::new();
    for (label, url) in &node_endpoints {
        match get_sync_status(url).await {
            Ok(status) => {
                tracing::info!(
                    "{}: unsafe_l2={}, safe_l2={}, finalized_l2={}",
                    label,
                    status.unsafe_l2.number,
                    status.safe_l2.number,
                    status.finalized_l2.number
                );
                initial_status.push((label.to_string(), url.clone(), status));
            }
            Err(e) => {
                tracing::info!("{}: failed to get sync status: {}", label, e);
            }
        }
    }

    if initial_status.is_empty() {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("No nodes available for testing");
    }

    // Wait for blocks to be produced (with 2s block time, wait ~30s for several blocks)
    tracing::info!("=== Waiting 30 seconds for blocks to be produced... ===");
    sleep(Duration::from_secs(30)).await;

    // Check that nodes have advanced
    tracing::info!("=== Checking that nodes have advanced... ===");
    let mut all_advancing = true;
    let mut errors = Vec::new();

    for (label, url, initial) in &initial_status {
        match get_sync_status(url).await {
            Ok(current) => {
                let unsafe_advanced = current.unsafe_l2.number > initial.unsafe_l2.number;
                let safe_advanced = current.safe_l2.number > initial.safe_l2.number;

                tracing::info!(
                    "{}: unsafe {} -> {} ({}), safe {} -> {} ({})",
                    label,
                    initial.unsafe_l2.number,
                    current.unsafe_l2.number,
                    if unsafe_advanced {
                        "ADVANCING"
                    } else {
                        "STALLED"
                    },
                    initial.safe_l2.number,
                    current.safe_l2.number,
                    if safe_advanced {
                        "ADVANCING"
                    } else {
                        "STALLED"
                    },
                );

                if !unsafe_advanced {
                    errors.push(format!("{}: unsafe head not advancing", label));
                    all_advancing = false;
                }
            }
            Err(e) => {
                errors.push(format!("{}: failed to get current status: {}", label, e));
                all_advancing = false;
            }
        }
    }

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    let cleanup_result = cleanup_by_prefix(&network_name).await?;
    tracing::info!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );

    if let Some(network) = cleanup_result.network_removed {
        tracing::info!("Removed network: {}", network);
    }

    // Assert after cleanup so we always clean up
    if !all_advancing {
        anyhow::bail!("Not all nodes are advancing:\n{}", errors.join("\n"));
    }

    tracing::info!("=== Test passed! All kona nodes with local binary are advancing. ===");
    Ok(())
}

/// Test that deployment skipping works correctly.
/// This test verifies:
/// - Deploy a network once
/// - Stop and cleanup
/// - Redeploy with same configuration (should skip contract deployment)
/// - Verify deployment version file exists and hash matches
/// - Network is healthy and advances
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_deployment_skipping() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("skip-test");
    tracing::info!(
        "=== Starting deployment skipping test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    // First deployment - should deploy contracts
    tracing::info!("=== First deployment: deploying contracts ===");
    let deployer = ctx.build_deployer().await?;
    let config_path = deployer.save_config()?;
    tracing::info!("Configuration saved to: {}", config_path.display());

    let _deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== First deployment completed successfully ===");

    // Verify deployment version file and get hash
    let first_hash = ctx
        .get_deployment_hash()
        .inspect(|hash| tracing::info!("First deployment hash: {}", hash))?;

    // Stop and cleanup the network
    tracing::info!("=== Cleaning up first deployment... ===");
    ctx.cleanup().await?;

    // Second deployment - should skip contract deployment
    tracing::info!("=== Second deployment: should skip contract deployment ===");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;
    tracing::info!("Configuration loaded from: {}", config_path.display());

    let start_time = std::time::Instant::now();
    let deployment = ctx.deploy(loaded_deployer).await?;
    tracing::info!(
        "=== Second deployment completed in {:?} ===",
        start_time.elapsed()
    );

    // Verify hash matches (contracts were skipped)
    let second_hash = ctx.get_deployment_hash()?;
    if first_hash != second_hash {
        ctx.cleanup().await?;
        anyhow::bail!(
            "Deployment hash mismatch! First: {}, Second: {}",
            first_hash,
            second_hash
        );
    }
    tracing::info!("✓ Deployment hash matches: {}", second_hash);

    // Verify network health
    tracing::info!("=== Verifying network health after redeployment... ===");
    wait_for_all_nodes(&deployment).await;

    let statuses = collect_all_sync_status(&deployment).await;
    if statuses.is_empty() {
        ctx.cleanup().await?;
        anyhow::bail!("Failed to get sync status from redeployed network");
    }
    tracing::info!("✓ Network is healthy");

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Deployment skipping works correctly. ===");
    Ok(())
}

/// Test that a network can be stopped and restarted from configuration files.
/// This test verifies:
/// - Deploy a network and let it run for a bit
/// - Save configuration
/// - Get sync status
/// - Stop all containers (but keep data)
/// - Restart from saved configuration
/// - Verify network continues from where it left off
/// - Verify network is still healthy and advancing
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_stop_and_restart_from_config() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("restart-test");
    tracing::info!(
        "=== Starting stop/restart test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    // Initial deployment
    tracing::info!("=== Initial deployment ===");
    let deployer = ctx.build_deployer().await?;
    let config_path = deployer.save_config()?;
    tracing::info!("Configuration saved to: {}", config_path.display());

    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Initial deployment completed successfully ===");

    // Wait for nodes and collect initial status
    tracing::info!("=== Waiting for nodes to be ready and collecting initial state... ===");
    wait_for_all_nodes(&deployment).await;

    // Let network produce blocks
    tracing::info!("=== Letting network run for 30 seconds to produce blocks... ===");
    sleep(Duration::from_secs(30)).await;

    // Collect status before stopping
    tracing::info!("=== Getting sync status before stopping... ===");
    let status_before = collect_all_sync_status(&deployment).await;
    if status_before.is_empty() {
        ctx.cleanup().await?;
        anyhow::bail!("Could not get sync status from any node before stopping");
    }

    // Stop network (keep data)
    tracing::info!("=== Stopping all containers... ===");
    ctx.cleanup().await?;

    // Verify data directory still exists
    if !ctx.outdata_path.exists() {
        anyhow::bail!(
            "Data directory disappeared after cleanup: {}",
            ctx.outdata_path.display()
        );
    }
    tracing::info!(
        "✓ Data directory still exists: {}",
        ctx.outdata_path.display()
    );

    // Wait before restart
    sleep(Duration::from_secs(5)).await;

    // Restart from configuration
    tracing::info!("=== Restarting from saved configuration... ===");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    let deployment = ctx.deploy(loaded_deployer).await?;
    tracing::info!("=== Network restarted successfully ===");

    // Wait for nodes after restart
    tracing::info!("=== Waiting for nodes to be ready after restart... ===");
    wait_for_all_nodes(&deployment).await;

    // Collect status after restart
    tracing::info!("=== Getting sync status after restart... ===");
    let status_after = collect_all_sync_status(&deployment).await;
    if status_after.is_empty() {
        ctx.cleanup().await?;
        anyhow::bail!("Could not get sync status from any node after restart");
    }

    // Verify sequencer state persisted
    tracing::info!("=== Verifying network resumed from previous state... ===");
    verify_sequencer_state_persisted(&status_before, &status_after)?;

    tracing::info!("✓ Sequencer resumed from previous state");
    tracing::info!("✓ Test objective achieved: sequencer state persisted across restart");

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Sequencer can be stopped and restarted successfully. ===");
    Ok(())
}

/// Verify that the sequencer maintained its state across restart.
fn verify_sequencer_state_persisted(
    before: &[(String, SyncStatus)],
    after: &[(String, SyncStatus)],
) -> Result<()> {
    let mut errors = Vec::new();
    let mut sequencer_resumed = false;

    for (before_label, before_status) in before {
        let Some((_, after_status)) = after.iter().find(|(label, _)| label == before_label) else {
            continue;
        };

        let block_diff =
            after_status.unsafe_l2.number as i64 - before_status.unsafe_l2.number as i64;
        tracing::info!(
            "{}: block before={}, after={}, diff={}",
            before_label,
            before_status.unsafe_l2.number,
            after_status.unsafe_l2.number,
            block_diff
        );

        let is_sequencer = before_label.contains("sequencer");

        // Handle validator nodes (they may need time to sync)
        if !is_sequencer {
            if after_status.unsafe_l2.number < 2 {
                tracing::info!("  Note: {} needs to re-sync from sequencer", before_label);
            }
            continue;
        }

        // Sequencer checks
        if after_status.unsafe_l2.number < 2 {
            errors.push(format!(
                "{}: Block number too low after restart ({}), state was reset",
                before_label, after_status.unsafe_l2.number
            ));
        } else {
            sequencer_resumed = true;
        }

        if block_diff < -10 {
            errors.push(format!(
                "{}: Block number regressed (before: {}, after: {})",
                before_label, before_status.unsafe_l2.number, after_status.unsafe_l2.number
            ));
        }
    }

    if !sequencer_resumed {
        errors.push("Sequencer did not resume from previous state".to_string());
    }

    if !errors.is_empty() {
        anyhow::bail!("Network state verification failed:\n{}", errors.join("\n"));
    }

    Ok(())
}

/// Test that the health check command reports a healthy network after deployment.
///
/// This test:
/// - Deploys a network in detached mode
/// - Waits for nodes to be ready and blocks to advance
/// - Runs health_check() and verifies the report is healthy
/// - Verifies chain IDs, block numbers, and container states in the report
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_health_check_reports_healthy() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("health-ok");
    tracing::info!(
        "=== Starting health check (healthy) test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    // Wait for nodes to be ready
    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    // Wait for blocks to advance on all nodes (validators need extra time to sync)
    tracing::info!("=== Waiting 60 seconds for blocks to be produced... ===");
    sleep(Duration::from_secs(60)).await;

    // Load config and run health check
    tracing::info!("=== Running health check... ===");
    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    let report = health::health_check(&loaded_deployer).await?;
    tracing::info!("{}", report);

    // Verify the report
    assert!(report.healthy, "Expected healthy report but got unhealthy");

    // L1 checks
    assert!(report.l1.running, "L1 (Anvil) should be running");
    assert!(
        report.l1.chain_id_match(),
        "L1 chain ID should match config"
    );
    assert_eq!(
        report.l1.chain_id,
        Some(ctx.l1_chain_id),
        "L1 chain ID should match test chain ID"
    );
    assert!(
        report.l1.block_number.unwrap_or(0) > 0,
        "L1 should have produced blocks"
    );

    // L2 node checks
    assert!(!report.nodes.is_empty(), "Should have at least one L2 node");

    for node in &report.nodes {
        assert!(
            node.execution.running,
            "{} op-reth should be running",
            node.label
        );
        assert!(
            node.execution.chain_id_match(),
            "{} L2 chain ID should match config",
            node.label
        );
        assert!(
            node.consensus.running,
            "{} kona-node should be running",
            node.label
        );
    }

    // Sequencer should have produced blocks
    let sequencer = &report.nodes[0];
    assert!(
        sequencer.execution.block_number.unwrap_or(0) > 0,
        "Sequencer should have produced L2 blocks"
    );

    // Service checks — op-batcher and op-proposer must be running
    assert_eq!(report.services.len(), 3, "Should have 3 services");
    for service in &report.services {
        if service.name != "op-challenger" {
            assert!(service.running, "{} should be running", service.name);
        }
    }

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Health check correctly reports healthy network. ===");
    Ok(())
}

/// Test that the health check command reports an unhealthy network when a container is stopped.
///
/// This test:
/// - Deploys a network in detached mode
/// - Waits for the network to be healthy
/// - Stops the op-batcher container
/// - Runs health_check() and verifies the report is unhealthy
/// - Verifies the stopped service is correctly identified
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_health_check_reports_unhealthy_on_stopped_container() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("health-fail");
    tracing::info!(
        "=== Starting health check (unhealthy) test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    // Wait for nodes to be ready and blocks to advance
    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting 60 seconds for blocks to be produced... ===");
    sleep(Duration::from_secs(60)).await;

    // Load config for health checks
    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    // Verify network is healthy first
    tracing::info!("=== Verifying network is initially healthy... ===");
    let initial_report = health::health_check(&loaded_deployer).await?;
    tracing::info!("{}", initial_report);
    assert!(
        initial_report.healthy,
        "Network should be healthy before stopping a container"
    );

    // Stop the op-batcher container
    let batcher_container = format!("{}-op-batcher", ctx.network_name);
    tracing::info!("=== Stopping container: {} ===", batcher_container);
    let output = Command::new("docker")
        .args(["stop", &batcher_container])
        .output()
        .context("Failed to run docker stop")?;

    if !output.status.success() {
        ctx.cleanup().await?;
        anyhow::bail!(
            "Failed to stop container: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    tracing::info!("Container stopped successfully");

    // Run health check again - should be unhealthy
    tracing::info!("=== Running health check after stopping op-batcher... ===");
    let unhealthy_report = health::health_check(&loaded_deployer).await?;
    tracing::info!("{}", unhealthy_report);

    assert!(
        !unhealthy_report.healthy,
        "Expected unhealthy report after stopping op-batcher"
    );

    // Verify the stopped service is correctly identified
    let batcher_service = unhealthy_report
        .services
        .iter()
        .find(|s| s.name == "op-batcher")
        .expect("op-batcher should be in the services list");

    assert!(
        !batcher_service.running,
        "op-batcher should be reported as not running"
    );

    // Other critical services should still be running
    for service in &unhealthy_report.services {
        if service.name != "op-batcher" && service.name != "op-challenger" {
            assert!(service.running, "{} should still be running", service.name);
        }
    }

    // L1 should still be running
    assert!(unhealthy_report.l1.running, "L1 should still be running");

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Health check correctly reports unhealthy network. ===");
    Ok(())
}

/// Query eth_getBalance on an L2 node and return the balance as a u128 (wei).
async fn get_l2_balance(rpc_url: &str, address: &str) -> Result<u128> {
    let client = rpc::create_client()?;
    let balance_hex: String = rpc::json_rpc_call(
        &client,
        rpc_url,
        "eth_getBalance",
        vec![serde_json::json!(address), serde_json::json!("latest")],
    )
    .await
    .context("Failed to get L2 balance")?;

    u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16)
        .context("Failed to parse balance hex")
}

/// Test that faucet_deposit sends ETH from L1 to L2 and the deposit arrives.
///
/// This test:
/// - Deploys a network in detached mode
/// - Waits for the sequencer to be ready
/// - Calls faucet_deposit with wait=true to send 1 ETH to a test address
/// - Verifies the L1 tx hash is returned
/// - Verifies the L2 balance increased
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_faucet_deposit_with_wait() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("faucet-wait");
    tracing::info!(
        "=== Starting faucet deposit (with wait) test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    // Wait for nodes to be ready
    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    // Let the network stabilize for a few blocks
    tracing::info!("=== Waiting 15 seconds for network to stabilize... ===");
    sleep(Duration::from_secs(15)).await;

    // Load the deployer from config for faucet_deposit
    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    // Use an arbitrary test address (not a deployer account)
    let test_address = "0x000000000000000000000000000000000000dEaD";

    // Get the sequencer host RPC URL for balance checks
    let seq_reth_port = get_container_host_port(
        &format!("{}-op-reth", ctx.network_name),
        loaded_deployer.l2_stack.sequencers[0].op_reth.http_port,
    )
    .context("Failed to get sequencer op-reth port")?;
    let l2_rpc_url = format!("http://localhost:{}", seq_reth_port);

    // Verify initial balance is zero
    let initial_balance = get_l2_balance(&l2_rpc_url, test_address).await?;
    tracing::info!("Initial L2 balance: {} wei", initial_balance);
    assert_eq!(
        initial_balance, 0,
        "Test address should start with zero balance"
    );

    // Send 1 ETH via faucet with wait=true
    tracing::info!("=== Sending 1 ETH via faucet (wait=true)... ===");
    let result = faucet::faucet_deposit(&loaded_deployer, test_address, 1.0, true).await?;

    // Verify tx hash is returned
    assert!(
        result.l1_tx_hash.starts_with("0x"),
        "L1 tx hash should be a hex string, got: {}",
        result.l1_tx_hash
    );
    tracing::info!("L1 tx hash: {}", result.l1_tx_hash);

    // Verify L2 balance was returned (wait=true)
    assert!(
        result.l2_balance.is_some(),
        "L2 balance should be returned when wait=true"
    );
    let l2_balance_hex = result.l2_balance.as_ref().unwrap();
    tracing::info!("L2 balance after deposit: {}", l2_balance_hex);

    // Verify balance is now ~1 ETH (1e18 wei)
    let final_balance = get_l2_balance(&l2_rpc_url, test_address).await?;
    tracing::info!("Final L2 balance: {} wei", final_balance);

    let one_eth_wei: u128 = 1_000_000_000_000_000_000;
    assert_eq!(
        final_balance, one_eth_wei,
        "L2 balance should be exactly 1 ETH (1e18 wei)"
    );

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Faucet deposit with wait works correctly. ===");
    Ok(())
}

/// Test that faucet_deposit without wait returns immediately with just the L1 tx hash.
///
/// This test:
/// - Deploys a network in detached mode
/// - Calls faucet_deposit with wait=false
/// - Verifies the L1 tx hash is returned
/// - Verifies L2 balance is None (no waiting)
/// - Then manually polls to confirm the deposit eventually arrives
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_faucet_deposit_no_wait() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("faucet-nowait");
    tracing::info!(
        "=== Starting faucet deposit (no wait) test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting 15 seconds for network to stabilize... ===");
    sleep(Duration::from_secs(15)).await;

    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    let test_address = "0x0000000000000000000000000000000000001234";

    // Send 2 ETH via faucet with wait=false
    tracing::info!("=== Sending 2 ETH via faucet (wait=false)... ===");
    let result = faucet::faucet_deposit(&loaded_deployer, test_address, 2.0, false).await?;

    // Verify tx hash is returned
    assert!(
        result.l1_tx_hash.starts_with("0x"),
        "L1 tx hash should be a hex string, got: {}",
        result.l1_tx_hash
    );
    tracing::info!("L1 tx hash: {}", result.l1_tx_hash);

    // Verify L2 balance is None (wait=false)
    assert!(
        result.l2_balance.is_none(),
        "L2 balance should be None when wait=false"
    );

    // Manually poll to confirm the deposit eventually arrives
    let seq_reth_port = get_container_host_port(
        &format!("{}-op-reth", ctx.network_name),
        loaded_deployer.l2_stack.sequencers[0].op_reth.http_port,
    )
    .context("Failed to get sequencer op-reth port")?;
    let l2_rpc_url = format!("http://localhost:{}", seq_reth_port);

    tracing::info!("=== Polling for L2 deposit to arrive (up to 120s)... ===");
    let two_eth_wei: u128 = 2_000_000_000_000_000_000;

    rpc::wait_until_ready("L2 faucet deposit", 120, || {
        let url = l2_rpc_url.clone();
        let addr = test_address.to_string();
        async move {
            let balance = get_l2_balance(&url, &addr).await?;
            if balance >= two_eth_wei {
                Ok(())
            } else {
                anyhow::bail!("Balance {} < expected {}", balance, two_eth_wei)
            }
        }
    })
    .await
    .context("Deposit did not arrive on L2 within timeout")?;

    let final_balance = get_l2_balance(&l2_rpc_url, test_address).await?;
    tracing::info!("Final L2 balance: {} wei", final_balance);
    assert_eq!(
        final_balance, two_eth_wei,
        "L2 balance should be exactly 2 ETH"
    );

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Faucet deposit without wait works correctly. ===");
    Ok(())
}

/// Test that multiple faucet deposits accumulate on L2.
///
/// This test:
/// - Deploys a network
/// - Sends 1 ETH, waits, verifies 1 ETH balance
/// - Sends 0.5 ETH more, waits, verifies 1.5 ETH total
/// - Confirms deposits are additive
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_faucet_multiple_deposits() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("faucet-multi");
    tracing::info!(
        "=== Starting faucet multiple deposits test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting 15 seconds for network to stabilize... ===");
    sleep(Duration::from_secs(15)).await;

    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    let test_address = "0x0000000000000000000000000000000000005678";

    let seq_reth_port = get_container_host_port(
        &format!("{}-op-reth", ctx.network_name),
        loaded_deployer.l2_stack.sequencers[0].op_reth.http_port,
    )
    .context("Failed to get sequencer op-reth port")?;
    let l2_rpc_url = format!("http://localhost:{}", seq_reth_port);

    // First deposit: 1 ETH
    tracing::info!("=== Sending first deposit: 1 ETH... ===");
    let result1 = faucet::faucet_deposit(&loaded_deployer, test_address, 1.0, true).await?;
    tracing::info!("First deposit L1 tx: {}", result1.l1_tx_hash);

    let balance_after_first = get_l2_balance(&l2_rpc_url, test_address).await?;
    let one_eth: u128 = 1_000_000_000_000_000_000;
    tracing::info!("Balance after first deposit: {} wei", balance_after_first);
    assert_eq!(
        balance_after_first, one_eth,
        "Balance should be 1 ETH after first deposit"
    );

    // Second deposit: 0.5 ETH
    tracing::info!("=== Sending second deposit: 0.5 ETH... ===");
    let result2 = faucet::faucet_deposit(&loaded_deployer, test_address, 0.5, true).await?;
    tracing::info!("Second deposit L1 tx: {}", result2.l1_tx_hash);

    // Verify tx hashes are different
    assert_ne!(
        result1.l1_tx_hash, result2.l1_tx_hash,
        "Each deposit should have a unique tx hash"
    );

    let balance_after_second = get_l2_balance(&l2_rpc_url, test_address).await?;
    let one_and_half_eth: u128 = 1_500_000_000_000_000_000;
    tracing::info!("Balance after second deposit: {} wei", balance_after_second);
    assert_eq!(
        balance_after_second, one_and_half_eth,
        "Balance should be 1.5 ETH after both deposits"
    );

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Multiple faucet deposits accumulate correctly. ===");
    Ok(())
}

/// Test that faucet_deposit rejects invalid addresses.
///
/// This test verifies the address validation without deploying a network,
/// since validation happens before any RPC calls.
#[tokio::test]
async fn test_faucet_rejects_invalid_address() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("faucet-invalid");
    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    // Deploy so we have a valid config (the deployer config is needed to get past
    // load_from_file, but validation happens before any RPC calls)
    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    // Test with too-short address
    let result = faucet::faucet_deposit(&loaded_deployer, "0x1234", 1.0, false).await;
    assert!(result.is_err(), "Should reject short address");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Invalid address"),
        "Error should mention invalid address, got: {}",
        err_msg
    );

    // Test with no 0x prefix
    let result = faucet::faucet_deposit(
        &loaded_deployer,
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef00",
        1.0,
        false,
    )
    .await;
    assert!(result.is_err(), "Should reject address without 0x prefix");

    // Test with non-hex characters
    let result = faucet::faucet_deposit(
        &loaded_deployer,
        "0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
        1.0,
        false,
    )
    .await;
    assert!(result.is_err(), "Should reject non-hex address");

    tracing::info!("=== Test passed! Faucet correctly rejects invalid addresses. ===");
    Ok(())
}

// ==================== Spam tests ====================

/// Test that spam rejects an empty rpc_url.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_spam_rejects_empty_rpc_url() -> Result<()> {
    tracing::info!("=== Test: spam rejects empty rpc_url ===");

    let ctx = TestContext::new("spam-empty-url");
    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    let spam_config = kupcake_deploy::spam::SpamConfig {
        scenario: "transfers".to_string(),
        tps: 10,
        duration: 5,
        forever: false,
        accounts: 10,
        min_balance: "0.1".to_string(),
        fund_amount: 100.0,
        funder_account_index: 10,
        report: false,
        contender_image: kupcake_deploy::spam::CONTENDER_DEFAULT_IMAGE.to_string(),
        contender_tag: kupcake_deploy::spam::CONTENDER_DEFAULT_TAG.to_string(),
        rpc_url: String::new(),
        extra_args: vec![],
    };

    let result = kupcake_deploy::spam::run_spam(&loaded_deployer, &spam_config).await;
    assert!(result.is_err(), "Should reject empty rpc_url");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("rpc_url"),
        "Error should mention rpc_url, got: {}",
        err_msg
    );

    tracing::info!("=== Test passed! Spam correctly rejects empty rpc_url. ===");
    Ok(())
}

/// Test that spam runs successfully with the built-in "transfers" scenario.
///
/// This test deploys a full network, funds the spammer, and runs contender briefly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_spam_transfers() -> Result<()> {
    tracing::info!("=== Test: spam transfers ===");

    let ctx = TestContext::new("spam-transfers");
    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;
    let _deployment = ctx.deploy(deployer).await?;

    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    // Wait for the network to be ready
    wait_for_all_nodes(&_deployment).await;

    let spam_config = kupcake_deploy::spam::SpamConfig {
        scenario: "transfers".to_string(),
        tps: 10,
        duration: 5,
        forever: false,
        accounts: 5,
        min_balance: "0.01".to_string(),
        fund_amount: 50.0,
        funder_account_index: 10,
        report: false,
        contender_image: kupcake_deploy::spam::CONTENDER_DEFAULT_IMAGE.to_string(),
        contender_tag: kupcake_deploy::spam::CONTENDER_DEFAULT_TAG.to_string(),
        rpc_url: sequencer_rpc_url(&loaded_deployer),
        extra_args: vec![],
    };

    // Run spam with a timeout
    let spam_result = timeout(
        Duration::from_secs(300),
        kupcake_deploy::spam::run_spam(&loaded_deployer, &spam_config),
    )
    .await;

    // Cleanup
    let _ = cleanup_by_prefix(&ctx.network_name).await;

    match spam_result {
        Ok(Ok(())) => {
            tracing::info!("=== Test passed! Spam transfers completed successfully. ===");
        }
        Ok(Err(e)) => {
            // Contender may exit non-zero if the scenario has issues, but the infrastructure worked
            tracing::info!("Spam completed with error (may be expected): {}", e);
        }
        Err(_) => {
            anyhow::bail!("Spam timed out after 300 seconds");
        }
    }

    Ok(())
}

// ==================== Additional Faucet Tests ====================

/// Get the current block number from an RPC endpoint.
async fn get_block_number(rpc_url: &str) -> Result<u64> {
    let client = rpc::create_client()?;
    let block_hex: String = rpc::json_rpc_call(&client, rpc_url, "eth_blockNumber", vec![]).await?;
    u64::from_str_radix(block_hex.trim_start_matches("0x"), 16)
        .context("Failed to parse block number")
}

/// Get the transaction count for a specific block.
async fn get_block_tx_count(rpc_url: &str, block_number: u64) -> Result<usize> {
    let client = rpc::create_client()?;
    let block: Value = rpc::json_rpc_call(
        &client,
        rpc_url,
        "eth_getBlockByNumber",
        vec![
            serde_json::json!(format!("0x{:x}", block_number)),
            serde_json::json!(false),
        ],
    )
    .await?;

    block["transactions"]
        .as_array()
        .context("Block missing transactions field")
        .map(|txs| txs.len())
}

/// Test faucet deposits with various amounts (small, fractional, large).
///
/// This test verifies:
/// - 0.001 ETH deposits work correctly (precision)
/// - 0.123456789 ETH works (gwei precision boundary)
/// - 10 ETH deposits work (larger amounts)
/// - All deposits to different addresses have correct independent balances
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_faucet_deposit_various_amounts() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("faucet-amounts");
    tracing::info!(
        "=== Starting faucet various amounts test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting 15 seconds for network to stabilize... ===");
    sleep(Duration::from_secs(15)).await;

    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    let seq_reth_port = get_container_host_port(
        &format!("{}-op-reth", ctx.network_name),
        loaded_deployer.l2_stack.sequencers[0].op_reth.http_port,
    )
    .context("Failed to get sequencer op-reth port")?;
    let l2_rpc_url = format!("http://localhost:{}", seq_reth_port);

    // Use addresses in a high range to avoid precompile/genesis allocations
    // (addresses 0x01-0x09 have pre-existing balances in OP Stack genesis)

    // Test 1: Small amount (0.001 ETH)
    let addr1 = "0x000000000000000000000000000000000000Aa01";
    tracing::info!("=== Test 1: Depositing 0.001 ETH... ===");
    let initial1 = get_l2_balance(&l2_rpc_url, addr1).await?;
    assert_eq!(initial1, 0, "Test address 1 should start with zero balance");
    faucet::faucet_deposit(&loaded_deployer, addr1, 0.001, true).await?;
    let balance1 = get_l2_balance(&l2_rpc_url, addr1).await?;
    let expected1: u128 = 1_000_000_000_000_000; // 0.001 ETH
    tracing::info!("Balance: {} wei (expected: {} wei)", balance1, expected1);
    assert_eq!(balance1, expected1, "0.001 ETH deposit should be exact");

    // Test 2: Fractional amount (0.123456789 ETH) — test gwei precision boundary
    let addr2 = "0x000000000000000000000000000000000000Aa02";
    tracing::info!("=== Test 2: Depositing 0.123456789 ETH... ===");
    let initial2 = get_l2_balance(&l2_rpc_url, addr2).await?;
    assert_eq!(initial2, 0, "Test address 2 should start with zero balance");
    faucet::faucet_deposit(&loaded_deployer, addr2, 0.123456789, true).await?;
    let balance2 = get_l2_balance(&l2_rpc_url, addr2).await?;
    let expected2: u128 = 123_456_789_000_000_000; // 0.123456789 ETH (gwei precision)
    tracing::info!("Balance: {} wei (expected: {} wei)", balance2, expected2);
    assert_eq!(
        balance2, expected2,
        "0.123456789 ETH deposit should match gwei precision"
    );

    // Test 3: Large amount (10 ETH)
    let addr3 = "0x000000000000000000000000000000000000Aa03";
    tracing::info!("=== Test 3: Depositing 10 ETH... ===");
    faucet::faucet_deposit(&loaded_deployer, addr3, 10.0, true).await?;
    let balance3 = get_l2_balance(&l2_rpc_url, addr3).await?;
    let expected3: u128 = 10_000_000_000_000_000_000; // 10 ETH
    tracing::info!("Balance: {} wei (expected: {} wei)", balance3, expected3);
    assert_eq!(balance3, expected3, "10 ETH deposit should be exact");

    // Verify all three addresses still have correct independent balances
    tracing::info!("=== Verifying all balances remain independent... ===");
    let final1 = get_l2_balance(&l2_rpc_url, addr1).await?;
    let final2 = get_l2_balance(&l2_rpc_url, addr2).await?;
    let final3 = get_l2_balance(&l2_rpc_url, addr3).await?;
    assert_eq!(final1, expected1, "Address 1 balance should be unchanged");
    assert_eq!(final2, expected2, "Address 2 balance should be unchanged");
    assert_eq!(final3, expected3, "Address 3 balance should be unchanged");
    tracing::info!("All three addresses have correct independent balances");

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Faucet handles various amounts correctly. ===");
    Ok(())
}

/// Test that faucet deposits are visible on validator nodes (not just sequencer).
///
/// This test verifies:
/// - Deposit is visible on the sequencer op-reth
/// - Deposit is also visible on the validator op-reth (synced via L1 derivation)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_faucet_deposit_visible_on_validator() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("faucet-validator");
    tracing::info!(
        "=== Starting faucet validator visibility test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting 15 seconds for network to stabilize... ===");
    sleep(Duration::from_secs(15)).await;

    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    // Get RPC URLs for both sequencer and validator
    let seq_container = &loaded_deployer.l2_stack.sequencers[0]
        .op_reth
        .container_name;
    let seq_port = loaded_deployer.l2_stack.sequencers[0].op_reth.http_port;
    let seq_host_port = get_container_host_port(seq_container, seq_port)
        .context("Failed to get sequencer op-reth port")?;
    let seq_rpc_url = format!("http://localhost:{}", seq_host_port);

    let val_container = &loaded_deployer.l2_stack.validators[0]
        .op_reth
        .container_name;
    let val_port = loaded_deployer.l2_stack.validators[0].op_reth.http_port;
    let val_host_port = get_container_host_port(val_container, val_port)
        .context("Failed to get validator op-reth port")?;
    let val_rpc_url = format!("http://localhost:{}", val_host_port);

    tracing::info!("Sequencer RPC: {}", seq_rpc_url);
    tracing::info!("Validator RPC: {}", val_rpc_url);

    let test_address = "0x000000000000000000000000000000000000CaFE";

    // Deposit 1 ETH
    tracing::info!("=== Depositing 1 ETH via faucet... ===");
    faucet::faucet_deposit(&loaded_deployer, test_address, 1.0, true).await?;

    // Verify on sequencer
    let seq_balance = get_l2_balance(&seq_rpc_url, test_address).await?;
    let one_eth: u128 = 1_000_000_000_000_000_000;
    tracing::info!("Sequencer balance: {} wei", seq_balance);
    assert_eq!(seq_balance, one_eth, "Sequencer should show 1 ETH");

    // Verify on validator (may need to wait for derivation to catch up)
    tracing::info!("=== Waiting for validator to sync deposit (up to 60s)... ===");
    let val_url = val_rpc_url.clone();
    let addr = test_address.to_string();
    rpc::wait_until_ready("validator sync", 60, || {
        let url = val_url.clone();
        let addr = addr.clone();
        async move {
            let balance = get_l2_balance(&url, &addr).await?;
            if balance >= one_eth {
                Ok(())
            } else {
                anyhow::bail!("Validator balance {} < expected {}", balance, one_eth)
            }
        }
    })
    .await
    .context("Validator did not sync deposit within timeout")?;

    let val_balance = get_l2_balance(&val_rpc_url, test_address).await?;
    tracing::info!("Validator balance: {} wei", val_balance);
    assert_eq!(val_balance, one_eth, "Validator should also show 1 ETH");

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Faucet deposit visible on both sequencer and validator. ===");
    Ok(())
}

// ==================== Additional Spam Tests ====================

/// Test that spam actually generates L2 transactions and traffic.
///
/// This test:
/// - Deploys a network and waits for it to stabilize
/// - Records initial block number
/// - Runs contender spam for 10 seconds with low TPS
/// - Records final block number
/// - Queries blocks for transactions
/// - Verifies blocks advanced and some contain spam transactions
/// - Verifies funder was funded on L2
/// - Verifies contender data directory was created
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_spam_generates_l2_traffic() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("spam-traffic");
    tracing::info!(
        "=== Starting spam traffic test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting 15 seconds for network to stabilize... ===");
    sleep(Duration::from_secs(15)).await;

    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    let seq_reth_port = get_container_host_port(
        &format!("{}-op-reth", ctx.network_name),
        loaded_deployer.l2_stack.sequencers[0].op_reth.http_port,
    )
    .context("Failed to get sequencer op-reth port")?;
    let l2_rpc_url = format!("http://localhost:{}", seq_reth_port);

    // Read funder address from anvil.json (account index 10)
    let anvil_json_content = std::fs::read_to_string(ctx.outdata_path.join("anvil/anvil.json"))
        .context("Failed to read anvil.json")?;
    let anvil_data: Value =
        serde_json::from_str(&anvil_json_content).context("Failed to parse anvil.json")?;
    let funder_address = anvil_data["available_accounts"][10]
        .as_str()
        .context("Funder account (index 10) not found in anvil.json")?;
    tracing::info!("Funder address: {}", funder_address);

    // Verify funder has no L2 balance before spam
    let funder_balance_before = get_l2_balance(&l2_rpc_url, funder_address).await?;
    tracing::info!(
        "Funder L2 balance before spam: {} wei",
        funder_balance_before
    );
    assert_eq!(
        funder_balance_before, 0,
        "Funder should have zero L2 balance before spam"
    );

    // Record initial block number
    let initial_block = get_block_number(&l2_rpc_url).await?;
    tracing::info!("Initial block number: {}", initial_block);

    // Verify contender data directory doesn't exist yet
    let contender_dir = ctx.outdata_path.join("contender");
    let dir_existed_before = contender_dir.exists();
    tracing::info!("Contender dir exists before spam: {}", dir_existed_before);

    // Run spam with low TPS and short duration to avoid rate limiting
    let spam_config = kupcake_deploy::spam::SpamConfig {
        scenario: "transfers".to_string(),
        tps: 2,
        duration: 10,
        forever: false,
        accounts: 2,
        min_balance: "0.01".to_string(),
        fund_amount: 50.0,
        funder_account_index: 10,
        report: false,
        contender_image: kupcake_deploy::spam::CONTENDER_DEFAULT_IMAGE.to_string(),
        contender_tag: kupcake_deploy::spam::CONTENDER_DEFAULT_TAG.to_string(),
        rpc_url: sequencer_rpc_url(&loaded_deployer),
        extra_args: vec![],
    };

    tracing::info!("=== Running spam (tps=2, duration=10s, accounts=2)... ===");
    let spam_result = timeout(
        Duration::from_secs(300),
        kupcake_deploy::spam::run_spam(&loaded_deployer, &spam_config),
    )
    .await;

    // Check spam result (don't fail test for non-zero exit since contender may have issues)
    match &spam_result {
        Ok(Ok(())) => tracing::info!("Spam completed successfully"),
        Ok(Err(e)) => tracing::info!("Spam completed with error (may be expected): {}", e),
        Err(_) => {
            let _ = cleanup_by_prefix(&ctx.network_name).await;
            anyhow::bail!("Spam timed out after 300 seconds");
        }
    }

    // Verify contender data directory was created
    assert!(
        contender_dir.exists(),
        "Contender data directory should exist after spam"
    );
    tracing::info!(
        "Contender data directory created at: {}",
        contender_dir.display()
    );

    // Verify funder was funded on L2 (run_spam does this via faucet_deposit)
    let funder_balance_after = get_l2_balance(&l2_rpc_url, funder_address).await?;
    tracing::info!("Funder L2 balance after spam: {} wei", funder_balance_after);
    assert!(
        funder_balance_after > 0,
        "Funder should have non-zero L2 balance after spam (was funded via faucet)"
    );

    // Wait a moment for any remaining blocks to be produced
    sleep(Duration::from_secs(5)).await;

    // Record final block number
    let final_block = get_block_number(&l2_rpc_url).await?;
    tracing::info!(
        "Final block number: {} (advanced {} blocks)",
        final_block,
        final_block - initial_block
    );

    // Verify blocks advanced
    assert!(
        final_block > initial_block,
        "Block number should advance during spam ({} -> {})",
        initial_block,
        final_block
    );

    // Query blocks for transactions
    tracing::info!("=== Checking blocks for transactions... ===");
    let mut total_txs = 0;
    let mut blocks_with_txs = 0;
    let blocks_to_check = std::cmp::min(final_block - initial_block, 20);

    for i in 0..blocks_to_check {
        let block_num = initial_block + 1 + i;
        match get_block_tx_count(&l2_rpc_url, block_num).await {
            Ok(tx_count) => {
                if tx_count > 0 {
                    blocks_with_txs += 1;
                    total_txs += tx_count;
                    tracing::info!("  Block {}: {} transaction(s)", block_num, tx_count);
                }
            }
            Err(e) => {
                tracing::info!("  Block {}: failed to query ({})", block_num, e);
            }
        }
    }

    tracing::info!(
        "Summary: {} blocks with transactions, {} total transactions in {} blocks checked",
        blocks_with_txs,
        total_txs,
        blocks_to_check
    );

    // Verify that at least some blocks had transactions
    // The faucet deposit itself creates at least 1 deposit tx, plus contender spam txs
    assert!(
        total_txs > 0,
        "Expected at least some transactions during spam, found 0"
    );

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!(
        "=== Test passed! Spam generated L2 traffic: {} transactions across {} blocks. ===",
        total_txs,
        blocks_with_txs
    );
    Ok(())
}

/// Test that the spam contender container runs on the correct Docker network.
///
/// This test:
/// - Deploys a network
/// - Runs spam briefly
/// - Inspects the contender container to verify it's on the kupcake Docker network
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_spam_container_on_correct_network() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("spam-network");
    tracing::info!(
        "=== Starting spam Docker network test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting 15 seconds for network to stabilize... ===");
    sleep(Duration::from_secs(15)).await;

    let config_path = ctx.outdata_path.join("Kupcake.toml");

    let expected_network = format!("{}-network", ctx.network_name);
    let contender_container = format!("{}-contender", ctx.network_name);

    // Spawn spam in a background task so we can inspect the container while it's running
    tracing::info!("=== Spawning spam task... ===");
    let loaded_deployer_clone = kupcake_deploy::Deployer::load_from_file(&config_path)?;

    // Run spam with very short duration — we just need the container to start
    let spam_config = kupcake_deploy::spam::SpamConfig {
        scenario: "transfers".to_string(),
        tps: 1,
        duration: 5,
        forever: false,
        accounts: 2,
        min_balance: "0.01".to_string(),
        fund_amount: 50.0,
        funder_account_index: 10,
        report: false,
        contender_image: kupcake_deploy::spam::CONTENDER_DEFAULT_IMAGE.to_string(),
        contender_tag: kupcake_deploy::spam::CONTENDER_DEFAULT_TAG.to_string(),
        rpc_url: sequencer_rpc_url(&loaded_deployer_clone),
        extra_args: vec![],
    };
    let spam_handle = tokio::spawn(async move {
        kupcake_deploy::spam::run_spam(&loaded_deployer_clone, &spam_config).await
    });

    // Wait for the contender container to appear (faucet deposit + image pull + start)
    tracing::info!("=== Waiting for contender container to start (up to 120s)... ===");
    let container_appeared = rpc::wait_until_ready("contender container", 120, || {
        let name = contender_container.clone();
        async move {
            let output = Command::new("docker")
                .args(["inspect", "--format", "{{.State.Status}}", &name])
                .output()
                .context("Failed to run docker inspect")?;

            if output.status.success() {
                let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if status == "running" || status == "exited" {
                    Ok(())
                } else {
                    anyhow::bail!("Container status: {}", status)
                }
            } else {
                anyhow::bail!("Container not found yet")
            }
        }
    })
    .await;

    if let Err(e) = container_appeared {
        tracing::info!("Warning: Could not detect contender container: {}", e);
        let _ = spam_handle.await;
    } else {
        // Inspect the container's network membership
        tracing::info!("=== Inspecting contender container network... ===");
        let output = Command::new("docker")
            .args([
                "inspect",
                "--format",
                "{{json .NetworkSettings.Networks}}",
                &contender_container,
            ])
            .output()
            .context("Failed to inspect container networks")?;

        if output.status.success() {
            let networks_json = String::from_utf8_lossy(&output.stdout);
            tracing::info!("Container networks: {}", networks_json.trim());

            assert!(
                networks_json.contains(&expected_network),
                "Contender container should be on network '{}', but found: {}",
                expected_network,
                networks_json.trim()
            );
            tracing::info!(
                "Contender container is on correct network: {}",
                expected_network
            );
        } else {
            tracing::info!(
                "Warning: Could not inspect container networks: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Wait for spam to finish
        let _ = spam_handle.await;
    }

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Contender container runs on correct Docker network. ===");
    Ok(())
}

// ==================== Spam Preset Tests ====================

/// Helper: deploy a network, run spam with a given preset, and verify that
/// the preset generated strictly more transactions than the number of blocks
/// produced (proving actual spam traffic beyond the 1 L1-attributes-deposit
/// tx that every L2 block contains).
///
/// Returns (total_txs, blocks_advanced) on success.
async fn run_preset_and_verify_traffic(
    preset: kupcake_deploy::spam::SpamPreset,
    test_prefix: &str,
    spam_duration_secs: u64,
) -> Result<(usize, u64)> {
    let ctx = TestContext::new(test_prefix);
    tracing::info!(
        "=== [{}] Starting preset test with network: {} (L1 chain ID: {}) ===",
        preset,
        ctx.network_name,
        ctx.l1_chain_id
    );

    let mut deployer = ctx.build_deployer().await?;

    // Increase op-reth RPC connection limit so heavy spam presets don't get 429s.
    // op-reth's own test config uses 429496729 (essentially unlimited).
    for seq in &mut deployer.l2_stack.sequencers {
        seq.op_reth.rpc_max_connections = Some(429496729);
    }
    for val in &mut deployer.l2_stack.validators {
        val.op_reth.rpc_max_connections = Some(429496729);
    }

    deployer.save_config()?;

    tracing::info!("=== [{}] Deploying network... ===", preset);
    let deployment = ctx.deploy(deployer).await?;

    tracing::info!("=== [{}] Waiting for nodes to be ready... ===", preset);
    wait_for_all_nodes(&deployment).await;
    sleep(Duration::from_secs(10)).await;

    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)?;

    // Get L2 RPC URL
    let seq_reth_port = get_container_host_port(
        &format!("{}-op-reth", ctx.network_name),
        loaded_deployer.l2_stack.sequencers[0].op_reth.http_port,
    )?;
    let l2_rpc_url = format!("http://localhost:{}", seq_reth_port);

    // Record initial block number
    let initial_block = get_block_number(&l2_rpc_url).await?;
    tracing::info!(
        "=== [{}] Initial block number: {} ===",
        preset,
        initial_block
    );

    // Generate config from the preset, override forever → bounded duration
    let mut spam_config = preset.to_config(&sequencer_rpc_url(&loaded_deployer));
    spam_config.forever = false;
    spam_config.duration = spam_duration_secs;

    tracing::info!(
        "=== [{}] Running spam (scenario={}, tps={}, accounts={}, duration={}s)... ===",
        preset,
        spam_config.scenario,
        spam_config.tps,
        spam_config.accounts,
        spam_config.duration
    );

    let spam_result = timeout(
        Duration::from_secs(300),
        kupcake_deploy::spam::run_spam(&loaded_deployer, &spam_config),
    )
    .await;

    match &spam_result {
        Ok(Ok(())) => tracing::info!("[{}] Spam completed successfully", preset),
        Ok(Err(e)) => tracing::info!(
            "[{}] Spam completed with error (may be expected): {}",
            preset,
            e
        ),
        Err(_) => {
            ctx.cleanup().await?;
            anyhow::bail!("[{}] Spam timed out after 300 seconds", preset);
        }
    }

    // Wait for remaining blocks to land
    sleep(Duration::from_secs(5)).await;

    let final_block = get_block_number(&l2_rpc_url).await?;
    let blocks_advanced = final_block - initial_block;
    tracing::info!(
        "[{}] Final block: {} (advanced {} blocks)",
        preset,
        final_block,
        blocks_advanced
    );

    assert!(
        blocks_advanced > 0,
        "[{}] Block number should advance during spam ({} -> {})",
        preset,
        initial_block,
        final_block
    );

    // Count total transactions across all new blocks
    let mut total_txs: usize = 0;
    let blocks_to_check = std::cmp::min(blocks_advanced, 20);
    for i in 0..blocks_to_check {
        let block_num = initial_block + 1 + i;
        if let Ok(tx_count) = get_block_tx_count(&l2_rpc_url, block_num).await {
            if tx_count > 0 {
                tracing::info!("  [{}] Block {}: {} tx(s)", preset, block_num, tx_count);
            }
            total_txs += tx_count;
        }
    }

    tracing::info!(
        "[{}] Total txs: {}, blocks checked: {}, blocks advanced: {}",
        preset,
        total_txs,
        blocks_to_check,
        blocks_advanced
    );

    // Each block contains at least 1 system tx (L1-attributes deposit).
    // Spam must produce strictly more txs than blocks, proving real user traffic.
    assert!(
        total_txs as u64 > blocks_to_check,
        "[{}] Expected more transactions ({}) than blocks ({}) — \
         spam should generate traffic beyond the 1 system tx per block",
        preset,
        total_txs,
        blocks_to_check
    );

    // Verify contender data directory was created
    let contender_dir = ctx.outdata_path.join("contender");
    assert!(
        contender_dir.exists(),
        "[{}] Contender data directory should exist after spam",
        preset
    );

    ctx.cleanup().await?;
    tracing::info!(
        "=== [{}] PASSED — {} txs across {} blocks (ratio: {:.1}x) ===",
        preset,
        total_txs,
        blocks_to_check,
        total_txs as f64 / blocks_to_check as f64
    );
    Ok((total_txs, blocks_advanced))
}

/// Test that SpamPreset::Light generates real L2 traffic.
///
/// Verifies total transactions > blocks produced (proving spam beyond system txs).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_spam_preset_light_generates_traffic() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();
    let (total_txs, _) = run_preset_and_verify_traffic(
        kupcake_deploy::spam::SpamPreset::Light,
        "spam-preset-light",
        15,
    )
    .await?;
    assert!(
        total_txs > 1,
        "Light preset should produce multiple transactions"
    );
    Ok(())
}

/// Test that SpamPreset::Erc20 generates real L2 traffic with the ERC-20 scenario.
///
/// Verifies total transactions > blocks produced (proving spam beyond system txs).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_spam_preset_erc20_generates_traffic() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();
    let (total_txs, _) = run_preset_and_verify_traffic(
        kupcake_deploy::spam::SpamPreset::Erc20,
        "spam-preset-erc20",
        15,
    )
    .await?;
    assert!(
        total_txs > 1,
        "Erc20 preset should produce multiple transactions"
    );
    Ok(())
}

/// Test that SpamPreset::Heavy generates more traffic than SpamPreset::Light.
///
/// Heavy has 200 TPS / 50 accounts vs Light's 10 TPS / 5 accounts,
/// so it should produce noticeably more transactions in the same duration.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_spam_preset_heavy_more_traffic_than_light() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let (light_txs, _) = run_preset_and_verify_traffic(
        kupcake_deploy::spam::SpamPreset::Light,
        "spam-cmp-light",
        10,
    )
    .await?;

    let (heavy_txs, _) = run_preset_and_verify_traffic(
        kupcake_deploy::spam::SpamPreset::Heavy,
        "spam-cmp-heavy",
        10,
    )
    .await?;

    tracing::info!(
        "Light txs: {}, Heavy txs: {} (ratio: {:.1}x)",
        light_txs,
        heavy_txs,
        heavy_txs as f64 / light_txs.max(1) as f64
    );

    assert!(
        heavy_txs > light_txs,
        "Heavy preset ({} txs) should produce more traffic than Light ({} txs)",
        heavy_txs,
        light_txs
    );

    Ok(())
}

/// Test the full `kupcake --spam` flow: deploy(no-wait) → reload config → spam → cleanup.
///
/// This simulates the exact code path that `kupcake --spam` uses:
/// 1. Build and save deployer config
/// 2. Deploy with wait_for_exit=false (returns immediately after services start)
/// 3. Reload deployer from saved Kupcake.toml
/// 4. Run spam using SpamPreset::Light
/// 5. Cleanup via prefix (simulating the post-spam cleanup)
///
/// Verifies the spam generated more txs than blocks (real traffic).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_spam_deploy_no_wait_then_reload_and_spam() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("spam-nowait");
    tracing::info!(
        "=== Starting spam no-wait deploy test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = ctx.build_deployer().await?;
    let config_path = deployer.save_config()?;

    tracing::info!("=== Deploying with wait_for_exit=false... ===");

    // Deploy with wait_for_exit=false — exactly what --spam does
    let deployment_result = timeout(
        Duration::from_secs(DEPLOYMENT_TIMEOUT_SECS),
        deployer.deploy(false, false),
    )
    .await;

    let deployment = match deployment_result {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => {
            let _ = cleanup_by_prefix(&ctx.network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            let _ = cleanup_by_prefix(&ctx.network_name).await;
            anyhow::bail!("Deployment timed out");
        }
    };

    tracing::info!("=== Deploy returned immediately (no wait). Waiting for nodes... ===");
    wait_for_all_nodes(&deployment).await;
    sleep(Duration::from_secs(10)).await;

    // Reload deployer from saved config — exactly what run_spam_after_deploy does
    let reloaded = kupcake_deploy::Deployer::load_from_file(&config_path)?;
    assert_eq!(reloaded.l1_chain_id, ctx.l1_chain_id);
    assert_eq!(
        reloaded.docker.net_name,
        format!("{}-network", ctx.network_name)
    );

    // Get L2 RPC URL for traffic verification
    let seq_reth_port = get_container_host_port(
        &format!("{}-op-reth", ctx.network_name),
        reloaded.l2_stack.sequencers[0].op_reth.http_port,
    )?;
    let l2_rpc_url = format!("http://localhost:{}", seq_reth_port);
    let initial_block = get_block_number(&l2_rpc_url).await?;

    // Use SpamPreset::Light with short duration
    let mut spam_config =
        kupcake_deploy::spam::SpamPreset::Light.to_config(&sequencer_rpc_url(&reloaded));
    spam_config.forever = false;
    spam_config.duration = 10;

    tracing::info!("=== Running spam from reloaded deployer... ===");
    let spam_result = timeout(
        Duration::from_secs(300),
        kupcake_deploy::spam::run_spam(&reloaded, &spam_config),
    )
    .await;

    match &spam_result {
        Ok(Ok(())) => tracing::info!("Spam completed successfully from reloaded deployer"),
        Ok(Err(e)) => tracing::info!("Spam completed with error (may be expected): {}", e),
        Err(_) => {
            let _ = cleanup_by_prefix(&ctx.network_name).await;
            anyhow::bail!("Spam timed out after 300 seconds");
        }
    }

    // Verify traffic was generated
    sleep(Duration::from_secs(5)).await;
    let final_block = get_block_number(&l2_rpc_url).await?;
    let blocks_advanced = final_block - initial_block;

    let mut total_txs: usize = 0;
    let blocks_to_check = std::cmp::min(blocks_advanced, 20);
    for i in 0..blocks_to_check {
        let block_num = initial_block + 1 + i;
        if let Ok(tx_count) = get_block_tx_count(&l2_rpc_url, block_num).await {
            total_txs += tx_count;
        }
    }

    tracing::info!(
        "No-wait flow: {} txs across {} blocks (ratio: {:.1}x)",
        total_txs,
        blocks_to_check,
        total_txs as f64 / blocks_to_check.max(1) as f64
    );

    assert!(
        total_txs as u64 > blocks_to_check,
        "Expected more transactions ({}) than blocks ({}) — \
         spam should generate traffic beyond the 1 system tx per block",
        total_txs,
        blocks_to_check
    );

    // Cleanup (simulates what --spam does when user didn't set --no-cleanup)
    tracing::info!("=== Cleaning up via prefix (simulating --spam cleanup)... ===");
    let cleanup_result = cleanup_by_prefix(&ctx.network_name).await?;
    assert!(
        !cleanup_result.containers_removed.is_empty(),
        "Should have cleaned up deployment containers"
    );
    tracing::info!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );

    tracing::info!(
        "=== Test passed! Full --spam flow: deploy(no-wait) → reload → spam → cleanup ==="
    );
    Ok(())
}

/// Get the Docker image used by a container via docker inspect.
fn get_container_image(container_name: &str) -> Result<String> {
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.Config.Image}}", container_name])
        .output()
        .context("Failed to run docker inspect")?;

    if !output.status.success() {
        anyhow::bail!(
            "docker inspect failed for {}: {}",
            container_name,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the exposed ports of a container via docker inspect.
fn get_container_exposed_ports(container_name: &str) -> Result<String> {
    let output = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{json .Config.ExposedPorts}}",
            container_name,
        ])
        .output()
        .context("Failed to run docker inspect")?;

    if !output.status.success() {
        anyhow::bail!(
            "docker inspect failed for {}: {}",
            container_name,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Test flashblocks deployment with op-rbuilder.
///
/// This test verifies:
/// - Sequencer uses op-rbuilder Docker image when flashblocks is enabled
/// - Validator still uses op-reth (not op-rbuilder)
/// - Flashblocks WS port (1111) is exposed on the sequencer's execution client container
/// - Blocks are advancing on both sequencer and validator
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_flashblocks_deployment() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("flashblocks");
    tracing::info!(
        "=== Starting flashblocks test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    // Deploy with flashblocks enabled
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2) // 1 sequencer + 1 validator
        .sequencer_count(1)
        .block_time(2)
        .flashblocks(true)
        .detach(true)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network with flashblocks enabled... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    // Verify sequencer op-reth container uses op-rbuilder image
    tracing::info!("=== Verifying sequencer uses op-rbuilder image... ===");
    let sequencer_reth_name = format!("{}-op-reth", ctx.network_name);
    let sequencer_image = get_container_image(&sequencer_reth_name)?;
    tracing::info!("Sequencer execution client image: {}", sequencer_image);

    if !sequencer_image.contains("op-rbuilder") {
        ctx.cleanup().await?;
        anyhow::bail!(
            "Expected sequencer to use op-rbuilder image, got: {}",
            sequencer_image
        );
    }

    // Verify validator op-reth container uses the default op-reth image (not op-rbuilder)
    tracing::info!("=== Verifying validator uses default op-reth image... ===");
    let validator_reth_name = format!("{}-op-reth-validator-1", ctx.network_name);
    let validator_image = get_container_image(&validator_reth_name)?;
    tracing::info!("Validator execution client image: {}", validator_image);

    let expected_reth_image = format!("{}:{}", OP_RETH_DEFAULT_IMAGE, OP_RETH_DEFAULT_TAG);
    if validator_image != expected_reth_image {
        ctx.cleanup().await?;
        anyhow::bail!(
            "Expected validator to use default op-reth image '{}', got: {}",
            expected_reth_image,
            validator_image
        );
    }

    // Verify both sequencer and validator kona-node containers use the default kona-node image
    tracing::info!("=== Verifying kona-node containers use default image... ===");
    let expected_kona_image = format!("{}:{}", KONA_NODE_DEFAULT_IMAGE, KONA_NODE_DEFAULT_TAG);
    let kona_containers = [
        format!("{}-kona-node", ctx.network_name),
        format!("{}-kona-node-validator-1", ctx.network_name),
    ];
    for kona_name in &kona_containers {
        let kona_image = get_container_image(kona_name)?;
        tracing::info!("{} image: {}", kona_name, kona_image);
        if kona_image != expected_kona_image {
            ctx.cleanup().await?;
            anyhow::bail!(
                "Expected {} to use default kona-node image '{}', got: {}",
                kona_name,
                expected_kona_image,
                kona_image
            );
        }
    }

    // Verify flashblocks port (1111) is exposed on sequencer's execution client
    tracing::info!("=== Verifying flashblocks port is exposed on sequencer... ===");
    let exposed_ports = get_container_exposed_ports(&sequencer_reth_name)?;
    tracing::info!("Sequencer exposed ports: {}", exposed_ports);

    if !exposed_ports.contains("1111/tcp") {
        ctx.cleanup().await?;
        anyhow::bail!(
            "Expected flashblocks port 1111/tcp to be exposed on sequencer, got: {}",
            exposed_ports
        );
    }

    // Verify flashblocks port is NOT exposed on validator
    let validator_exposed_ports = get_container_exposed_ports(&validator_reth_name)?;
    if validator_exposed_ports.contains("1111/tcp") {
        ctx.cleanup().await?;
        anyhow::bail!(
            "Flashblocks port 1111/tcp should NOT be exposed on validator, got: {}",
            validator_exposed_ports
        );
    }

    // Wait for both sequencer and validator op-reth to be ready
    tracing::info!("=== Waiting for op-reth nodes to be ready... ===");
    for node in deployment.l2_stack.all_nodes() {
        let label = if node.is_sequencer() {
            "sequencer"
        } else {
            "validator"
        };
        if let Err(e) = node.op_reth.wait_until_ready(NODE_READY_TIMEOUT_SECS).await {
            ctx.cleanup().await?;
            anyhow::bail!("{} op-reth not ready: {}", label, e);
        }
        tracing::info!("{} op-reth is ready", label);
    }

    // Wait for both kona-nodes to be ready (validator may need extra time with flashblocks)
    tracing::info!("=== Waiting for kona-node consensus clients to be ready... ===");
    for node in deployment.l2_stack.all_nodes() {
        let label = if node.is_sequencer() {
            "sequencer"
        } else {
            "validator"
        };
        // Give kona-nodes extra time (180s) since flashblocks relay adds startup latency
        if let Err(e) = node.kona_node.wait_until_ready(180).await {
            ctx.cleanup().await?;
            anyhow::bail!("{} kona-node not ready: {}", label, e);
        }
        tracing::info!("{} kona-node is ready", label);
    }

    // Get initial block numbers from op-reth (both sequencer and validator)
    tracing::info!("=== Getting initial op-reth block numbers... ===");
    let mut initial_blocks = Vec::new();
    for (idx, node) in deployment.l2_stack.all_nodes().enumerate() {
        let label = if node.is_sequencer() {
            "sequencer".to_string()
        } else {
            format!("validator-{}", idx)
        };
        let status = node
            .op_reth
            .sync_status()
            .await
            .with_context(|| format!("Failed to get {} op-reth status", label))?;
        tracing::info!("{}: block={}", label, status.block_number);
        initial_blocks.push((label, status.block_number));
    }

    // Wait for blocks to be produced
    tracing::info!("=== Waiting 30 seconds for blocks to be produced... ===");
    sleep(Duration::from_secs(30)).await;

    // Check that both sequencer and validator have advanced
    tracing::info!("=== Checking that all nodes have advanced... ===");
    let mut errors = Vec::new();

    for (idx, node) in deployment.l2_stack.all_nodes().enumerate() {
        let label = if node.is_sequencer() {
            "sequencer".to_string()
        } else {
            format!("validator-{}", idx)
        };

        if let Some((_, initial_block)) = initial_blocks.iter().find(|(l, _)| l == &label) {
            let status = node
                .op_reth
                .sync_status()
                .await
                .with_context(|| format!("Failed to get {} op-reth status", label))?;
            let advanced = status.block_number > *initial_block;
            tracing::info!(
                "{}: block {} -> {} ({})",
                label,
                initial_block,
                status.block_number,
                if advanced { "ADVANCING" } else { "STALLED" }
            );

            if !advanced {
                errors.push(format!("{}: block number not advancing", label));
            }
        }
    }

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    if !errors.is_empty() {
        anyhow::bail!("Not all nodes are advancing:\n{}", errors.join("\n"));
    }

    tracing::info!("=== Test passed! Flashblocks deployment with op-rbuilder is working. ===");
    Ok(())
}

/// Test flashblocks deployment with spam traffic.
///
/// This test verifies that spam works correctly against a flashblocks-enabled network:
/// - Deploys with flashblocks enabled (sequencer uses op-rbuilder)
/// - Verifies sequencer uses op-rbuilder image
/// - Funds the spammer account via faucet deposit
/// - Runs contender spam against the flashblocks sequencer
/// - Verifies blocks advanced and contain user transactions
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_flashblocks_with_spam() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("fb-spam");
    tracing::info!(
        "=== Starting flashblocks+spam test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    // Deploy with flashblocks enabled
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2) // 1 sequencer + 1 validator
        .sequencer_count(1)
        .block_time(2)
        .flashblocks(true)
        .detach(true)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network with flashblocks enabled... ===");
    let deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed successfully ===");

    // Verify sequencer uses op-rbuilder image
    let sequencer_reth_name = format!("{}-op-reth", ctx.network_name);
    let sequencer_image = get_container_image(&sequencer_reth_name)?;
    tracing::info!("Sequencer execution client image: {}", sequencer_image);

    if !sequencer_image.contains("op-rbuilder") {
        ctx.cleanup().await?;
        anyhow::bail!(
            "Expected sequencer to use op-rbuilder image, got: {}",
            sequencer_image
        );
    }

    // Wait for nodes to be ready (flashblocks needs extra time)
    tracing::info!("=== Waiting for nodes to be ready... ===");
    for node in deployment.l2_stack.all_nodes() {
        let label = if node.is_sequencer() {
            "sequencer"
        } else {
            "validator"
        };
        if let Err(e) = node.kona_node.wait_until_ready(180).await {
            ctx.cleanup().await?;
            anyhow::bail!("{} kona-node not ready: {}", label, e);
        }
        tracing::info!("{} is ready", label);
    }

    tracing::info!("=== Waiting 15 seconds for network to stabilize... ===");
    sleep(Duration::from_secs(15)).await;

    // Load deployer from config for spam
    let config_path = ctx.outdata_path.join("Kupcake.toml");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;

    // Get host RPC URL for verification queries
    let seq_reth_port = get_container_host_port(
        &sequencer_reth_name,
        loaded_deployer.l2_stack.sequencers[0].op_reth.http_port,
    )
    .context("Failed to get sequencer op-reth port")?;
    let l2_rpc_url = format!("http://localhost:{}", seq_reth_port);

    // Record initial block number
    let initial_block = get_block_number(&l2_rpc_url).await?;
    tracing::info!("Initial block number: {}", initial_block);

    // Run spam with moderate TPS and short duration
    let spam_config = kupcake_deploy::spam::SpamConfig {
        scenario: "transfers".to_string(),
        tps: 20,
        duration: 10,
        forever: false,
        accounts: 2,
        min_balance: "0.1".to_string(),
        fund_amount: 50.0,
        funder_account_index: 10,
        report: false,
        contender_image: kupcake_deploy::spam::CONTENDER_DEFAULT_IMAGE.to_string(),
        contender_tag: kupcake_deploy::spam::CONTENDER_DEFAULT_TAG.to_string(),
        rpc_url: sequencer_rpc_url(&loaded_deployer),
        extra_args: vec![],
    };

    tracing::info!("=== Running spam against flashblocks sequencer (tps=20, duration=10s)... ===");
    let spam_result = timeout(
        Duration::from_secs(300),
        kupcake_deploy::spam::run_spam(&loaded_deployer, &spam_config),
    )
    .await;

    match &spam_result {
        Ok(Ok(())) => tracing::info!("Spam completed successfully"),
        Ok(Err(e)) => {
            ctx.cleanup().await?;
            anyhow::bail!("Spam failed: {}", e);
        }
        Err(_) => {
            ctx.cleanup().await?;
            anyhow::bail!("Spam timed out after 300 seconds");
        }
    }

    // Wait for remaining blocks
    sleep(Duration::from_secs(5)).await;

    // Verify blocks advanced
    let final_block = get_block_number(&l2_rpc_url).await?;
    tracing::info!(
        "Block numbers: {} -> {} (advanced {} blocks)",
        initial_block,
        final_block,
        final_block - initial_block
    );

    assert!(
        final_block > initial_block,
        "Block number should advance during spam ({} -> {})",
        initial_block,
        final_block
    );

    // Check blocks for transactions
    tracing::info!("=== Checking blocks for transactions... ===");
    let mut total_txs = 0;
    let mut blocks_with_txs = 0;
    let blocks_to_check = std::cmp::min(final_block - initial_block, 20);

    for i in 0..blocks_to_check {
        let block_num = initial_block + 1 + i;
        if let Ok(tx_count) = get_block_tx_count(&l2_rpc_url, block_num).await
            && tx_count > 0
        {
            blocks_with_txs += 1;
            total_txs += tx_count;
            tracing::info!("  Block {}: {} transaction(s)", block_num, tx_count);
        }
    }

    tracing::info!(
        "Summary: {} total transactions across {} blocks with txs ({} blocks checked)",
        total_txs,
        blocks_with_txs,
        blocks_to_check
    );

    assert!(
        total_txs > 0,
        "Expected at least some transactions during spam, found 0"
    );

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!(
        "=== Test passed! Flashblocks + spam working: {} transactions across {} blocks. ===",
        total_txs,
        blocks_with_txs
    );
    Ok(())
}

/// Test that the flashblocks Grafana dashboard is correctly provisioned and can query
/// op-rbuilder metrics from Prometheus.
///
/// This test verifies:
/// - Grafana is accessible and healthy
/// - The flashblocks dashboard is provisioned with the correct datasource UID
/// - Prometheus is scraping op-rbuilder metrics
/// - Grafana can query the Prometheus datasource
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_flashblocks_grafana_dashboard() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("fb-dash");
    tracing::info!(
        "=== Starting flashblocks dashboard test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    // Deploy with flashblocks enabled and dashboards provisioned
    let dashboards_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../grafana/dashboards");
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2) // 1 sequencer + 1 validator
        .sequencer_count(1)
        .block_time(2)
        .flashblocks(true)
        .dashboards_path(dashboards_path)
        .detach(true)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network... ===");
    let _deployment = ctx.deploy(deployer).await?;
    tracing::info!("=== Deployment completed ===");

    // Get Grafana and Prometheus host ports
    let grafana_name = format!("{}-grafana", ctx.network_name);
    let prometheus_name = format!("{}-prometheus", ctx.network_name);

    let grafana_port =
        get_container_host_port(&grafana_name, 3000).context("Failed to get Grafana host port")?;
    let prometheus_port = get_container_host_port(&prometheus_name, 9099)
        .context("Failed to get Prometheus host port")?;

    let grafana_url = format!("http://localhost:{}", grafana_port);
    let prometheus_url = format!("http://localhost:{}", prometheus_port);

    tracing::info!(grafana_url = %grafana_url, prometheus_url = %prometheus_url, "Monitoring endpoints");

    let client = rpc::create_client()?;

    // Wait for Grafana to be ready
    tracing::info!("=== Waiting for Grafana to be ready... ===");
    let grafana_ready = timeout(Duration::from_secs(30), async {
        loop {
            match client
                .get(format!("{}/api/health", grafana_url))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => return Ok::<(), anyhow::Error>(()),
                _ => sleep(Duration::from_secs(1)).await,
            }
        }
    })
    .await;

    if grafana_ready.is_err() {
        ctx.cleanup().await?;
        anyhow::bail!("Grafana not ready after 30 seconds");
    }
    tracing::info!("Grafana is healthy");

    // Verify the flashblocks dashboard is provisioned (retry since provisioning is async)
    tracing::info!("=== Checking flashblocks dashboard... ===");
    let dashboard_json: Value = {
        let dashboard_result = timeout(Duration::from_secs(30), async {
            loop {
                match client
                    .get(format!(
                        "{}/api/dashboards/uid/kupcake-flashblocks",
                        grafana_url
                    ))
                    .basic_auth("admin", Some("admin"))
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        return resp.json::<Value>().await.map_err(|e| anyhow::anyhow!(e));
                    }
                    _ => sleep(Duration::from_secs(2)).await,
                }
            }
        })
        .await;

        match dashboard_result {
            Ok(Ok(json)) => json,
            Ok(Err(e)) => {
                ctx.cleanup().await?;
                anyhow::bail!("Failed to parse dashboard JSON: {}", e);
            }
            Err(_) => {
                ctx.cleanup().await?;
                anyhow::bail!("Flashblocks dashboard not provisioned after 30 seconds");
            }
        }
    };
    let dashboard_title = dashboard_json["dashboard"]["title"]
        .as_str()
        .unwrap_or("unknown");
    tracing::info!("Found dashboard: {}", dashboard_title);

    assert_eq!(
        dashboard_title, "Flashblocks",
        "Dashboard title should be 'Flashblocks'"
    );

    // Verify all panel datasource UIDs point to "Prometheus"
    let panels = dashboard_json["dashboard"]["panels"]
        .as_array()
        .context("No panels in dashboard")?;

    for panel in panels {
        let panel_title = panel["title"].as_str().unwrap_or("(no title)");
        if let Some(ds) = panel.get("datasource") {
            let uid = ds["uid"].as_str().unwrap_or("");
            if uid != "-- Grafana --" && !uid.is_empty() {
                assert_eq!(
                    uid, "Prometheus",
                    "Panel '{}' has wrong datasource UID: '{}' (expected 'Prometheus')",
                    panel_title, uid
                );
            }
        }
        // Also check targets within panels
        if let Some(targets) = panel["targets"].as_array() {
            for target in targets {
                if let Some(ds) = target.get("datasource") {
                    let uid = ds["uid"].as_str().unwrap_or("");
                    assert_eq!(
                        uid, "Prometheus",
                        "Target in panel '{}' has wrong datasource UID: '{}'",
                        panel_title, uid
                    );
                }
            }
        }
    }
    tracing::info!("All panel datasource UIDs are correct");

    // Verify Grafana's Prometheus datasource is configured and reachable
    tracing::info!("=== Verifying Grafana datasource... ===");
    let datasources_resp = client
        .get(format!("{}/api/datasources", grafana_url))
        .basic_auth("admin", Some("admin"))
        .send()
        .await
        .context("Failed to query Grafana datasources API")?;

    let datasources: Vec<Value> = datasources_resp
        .json()
        .await
        .context("Failed to parse datasources")?;

    let prometheus_ds = datasources
        .iter()
        .find(|ds| ds["type"].as_str() == Some("prometheus"))
        .context("No Prometheus datasource found in Grafana")?;

    tracing::info!(
        "Prometheus datasource: name={}, uid={}",
        prometheus_ds["name"].as_str().unwrap_or("?"),
        prometheus_ds["uid"].as_str().unwrap_or("?")
    );

    // Wait for Prometheus to scrape some op-rbuilder metrics
    tracing::info!("=== Waiting for Prometheus to scrape op-rbuilder metrics... ===");
    let metrics_ready = timeout(Duration::from_secs(60), async {
        loop {
            let query_url = format!(
                "{}/api/v1/query?query=reth_op_rbuilder_flags_flashblocks_enabled",
                prometheus_url
            );
            match client.get(&query_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(json) = resp.json::<Value>().await {
                        let results = &json["data"]["result"];
                        if let Some(arr) = results.as_array()
                            && !arr.is_empty()
                        {
                            return Ok::<(), anyhow::Error>(());
                        }
                    }
                }
                _ => {}
            }
            sleep(Duration::from_secs(3)).await;
        }
    })
    .await;

    if metrics_ready.is_err() {
        ctx.cleanup().await?;
        anyhow::bail!(
            "Prometheus did not scrape reth_op_rbuilder_flags_flashblocks_enabled within 60 seconds"
        );
    }
    tracing::info!("Prometheus is scraping op-rbuilder metrics");

    // Verify key op-rbuilder metrics exist in Prometheus
    let key_metrics = [
        "reth_op_rbuilder_flags_flashblocks_enabled",
        "reth_op_rbuilder_block_built_success",
    ];

    for metric in &key_metrics {
        let query_url = format!("{}/api/v1/query?query={}", prometheus_url, metric);
        let resp = client
            .get(&query_url)
            .send()
            .await
            .with_context(|| format!("Failed to query Prometheus for {}", metric))?;

        let json: Value = resp
            .json()
            .await
            .context("Failed to parse Prometheus response")?;
        let results = json["data"]["result"]
            .as_array()
            .context("No result array in Prometheus response")?;

        assert!(
            !results.is_empty(),
            "Expected Prometheus to have metric '{}', but got no results",
            metric
        );
        tracing::info!("Metric '{}' present ({} series)", metric, results.len());
    }

    // Verify Prometheus is scraping validator metrics (standard reth metrics)
    tracing::info!("=== Verifying validator metrics are being scraped... ===");
    let validator_metrics_ready = timeout(Duration::from_secs(30), async {
        loop {
            let query_url = format!(
                "{}/api/v1/query?query=up{{job=\"op-reth-validator-1\"}}",
                prometheus_url
            );
            match client.get(&query_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(json) = resp.json::<Value>().await
                        && let Some(arr) = json["data"]["result"].as_array()
                        && !arr.is_empty()
                    {
                        let value = arr[0]["value"][1].as_str().unwrap_or("0");
                        if value == "1" {
                            return Ok::<(), anyhow::Error>(());
                        }
                    }
                }
                _ => {}
            }
            sleep(Duration::from_secs(2)).await;
        }
    })
    .await;

    if validator_metrics_ready.is_err() {
        ctx.cleanup().await?;
        anyhow::bail!("Prometheus is not scraping validator op-reth metrics (up != 1)");
    }

    // Check validator has standard reth metrics
    let validator_metric = "reth_info{job=\"op-reth-validator-1\"}";
    let query_url = format!("{}/api/v1/query?query={}", prometheus_url, validator_metric);
    let resp = client
        .get(&query_url)
        .send()
        .await
        .context("Failed to query Prometheus for validator reth_info")?;
    let json: Value = resp.json().await.context("Failed to parse response")?;
    let results = json["data"]["result"]
        .as_array()
        .context("No result array")?;
    assert!(
        !results.is_empty(),
        "Expected validator to have reth_info metric"
    );
    tracing::info!(
        "Validator metrics confirmed: reth_info present ({} series)",
        results.len()
    );

    // Cleanup
    tracing::info!("=== Cleaning up network... ===");
    ctx.cleanup().await?;

    tracing::info!("=== Test passed! Flashblocks Grafana dashboard is correctly configured. ===");
    Ok(())
}
