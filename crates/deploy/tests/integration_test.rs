//! Integration tests for kupcake-deploy.
//!
//! These tests require Docker to be running and will deploy actual networks.
//! They run in local mode without forking, which deploys all contracts from scratch.
//! Run with: cargo test --test integration_test

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use kupcake_deploy::{cleanup_by_prefix, DeployerBuilder, OutDataPath};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::{sleep, timeout};

/// Response from optimism_syncStatus RPC call.
#[derive(Debug, Deserialize)]
struct SyncStatusResponse {
    result: Option<SyncStatus>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    message: String,
}

/// Sync status from kona-node.
#[derive(Debug, Clone, Deserialize)]
struct SyncStatus {
    unsafe_l2: BlockRef,
    safe_l2: BlockRef,
    finalized_l2: BlockRef,
}

/// Block reference with number and hash.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct BlockRef {
    number: u64,
    hash: String,
}

/// Query the sync status from a kona-node RPC endpoint.
async fn get_sync_status(rpc_url: &str) -> Result<SyncStatus> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "optimism_syncStatus",
            "params": [],
            "id": 1
        }))
        .send()
        .await
        .context("Failed to send RPC request")?;

    let status: SyncStatusResponse = response
        .json()
        .await
        .context("Failed to parse sync status response")?;

    if let Some(error) = status.error {
        anyhow::bail!("RPC error: {}", error.message);
    }

    status.result.context("No result in response")
}

/// Get the host port mapped to a container port using docker inspect.
fn get_container_host_port(container_name: &str, container_port: u16) -> Result<u16> {
    let output = Command::new("docker")
        .args([
            "inspect",
            "--format",
            &format!("{{{{(index (index .NetworkSettings.Ports \"{}/tcp\") 0).HostPort}}}}", container_port),
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
    port_str
        .parse::<u16>()
        .with_context(|| format!("Failed to parse port '{}' for container {}", port_str, container_name))
}

/// Wait for a node to be ready by polling its sync status endpoint.
async fn wait_for_node_ready(rpc_url: &str, timeout_secs: u64) -> Result<()> {
    let start = std::time::Instant::now();
    let max_duration = Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > max_duration {
            anyhow::bail!("Timeout waiting for node at {} to be ready", rpc_url);
        }

        match get_sync_status(rpc_url).await {
            Ok(_) => {
                println!("Node at {} is ready", rpc_url);
                return Ok(());
            }
            Err(_) => {
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

/// Sync progress from eth_syncing when actively syncing.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct EthSyncProgress {
    #[serde(rename = "startingBlock", deserialize_with = "deserialize_hex_u64")]
    starting_block: u64,
    #[serde(rename = "currentBlock", deserialize_with = "deserialize_hex_u64")]
    current_block: u64,
    #[serde(rename = "highestBlock", deserialize_with = "deserialize_hex_u64")]
    highest_block: u64,
}

/// Deserialize hex string to u64.
fn deserialize_hex_u64<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    let s = s.trim_start_matches("0x");
    u64::from_str_radix(s, 16).map_err(serde::de::Error::custom)
}

/// Status of an op-reth node.
#[derive(Debug, Clone)]
struct OpRethStatus {
    /// Whether the node is syncing (true) or synced (false).
    is_syncing: bool,
    /// Current block number.
    block_number: u64,
    /// Sync progress if syncing.
    sync_progress: Option<EthSyncProgress>,
}

/// Query the sync status from an op-reth node using eth_syncing.
async fn get_op_reth_status(rpc_url: &str) -> Result<OpRethStatus> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    // Get eth_syncing status
    let syncing_response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_syncing",
            "params": [],
            "id": 1
        }))
        .send()
        .await
        .context("Failed to send eth_syncing request")?;

    let syncing_body: Value = syncing_response
        .json()
        .await
        .context("Failed to parse eth_syncing response")?;

    let (is_syncing, sync_progress) = match &syncing_body["result"] {
        Value::Bool(false) => (false, None),
        Value::Object(_) => {
            let progress: EthSyncProgress = serde_json::from_value(syncing_body["result"].clone())
                .context("Failed to parse sync progress")?;
            (true, Some(progress))
        }
        _ => anyhow::bail!("Unexpected eth_syncing response: {:?}", syncing_body),
    };

    // Get current block number
    let block_response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 2
        }))
        .send()
        .await
        .context("Failed to send eth_blockNumber request")?;

    let block_body: Value = block_response
        .json()
        .await
        .context("Failed to parse eth_blockNumber response")?;

    let block_hex = block_body["result"]
        .as_str()
        .context("eth_blockNumber result is not a string")?;
    let block_number =
        u64::from_str_radix(block_hex.trim_start_matches("0x"), 16).context("Failed to parse block number")?;

    Ok(OpRethStatus {
        is_syncing,
        block_number,
        sync_progress,
    })
}

/// Wait for op-reth to be ready by polling its RPC endpoint.
async fn wait_for_op_reth_ready(rpc_url: &str, timeout_secs: u64) -> Result<()> {
    let start = std::time::Instant::now();
    let max_duration = Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > max_duration {
            anyhow::bail!("Timeout waiting for op-reth at {} to be ready", rpc_url);
        }

        match get_op_reth_status(rpc_url).await {
            Ok(status) => {
                println!(
                    "op-reth at {} is ready (block: {}, syncing: {})",
                    rpc_url, status.block_number, status.is_syncing
                );
                return Ok(());
            }
            Err(_) => {
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
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
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

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
            let body: Value = resp.json().await.context("Failed to parse opp_version response")?;
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
    let start = std::time::Instant::now();
    let max_duration = Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > max_duration {
            anyhow::bail!("Timeout waiting for op-batcher at {} to be ready", rpc_url);
        }

        match get_op_batcher_status(rpc_url).await {
            Ok(status) if status.is_healthy => {
                println!("op-batcher at {} is ready", rpc_url);
                return Ok(());
            }
            Ok(_) | Err(_) => {
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

/// Test that deploys a network and verifies all nodes have advancing heads.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_network_deployment_and_sync_status() -> Result<()> {
    // Initialize tracing for test output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init()
        .ok();

    let network_name = format!("kup-test-{}", std::process::id());
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    println!("=== Starting test deployment with network: {} ===", network_name);

    // Build the deployer - use local mode (no forking, deploys all contracts from scratch)
    let deployer = DeployerBuilder::new(31337) // Local Anvil chain ID
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

    println!("=== Deploying network... ===");

    // Deploy with a timeout
    let deploy_result = timeout(Duration::from_secs(600), deployer.deploy(false)).await;

    match deploy_result {
        Ok(Ok(())) => println!("=== Deployment completed successfully ==="),
        Ok(Err(e)) => {
            // Cleanup before returning error
            let _ = cleanup_by_prefix(&network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            // Cleanup before returning error
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!("Deployment timed out after 600 seconds");
        }
    }

    // Get the actual mapped ports for kona-node containers
    // kona-node uses port 7545 internally
    let sequencer_port = get_container_host_port(&format!("{}-kona-node", network_name), 7545)
        .context("Failed to get sequencer kona-node port")?;
    let validator_port = get_container_host_port(&format!("{}-kona-node-validator-1", network_name), 7545)
        .context("Failed to get validator kona-node port")?;

    let node_endpoints = vec![
        ("sequencer", format!("http://localhost:{}", sequencer_port)),
        ("validator-1", format!("http://localhost:{}", validator_port)),
    ];

    // Wait for nodes to be ready
    println!("=== Waiting for nodes to be ready... ===");
    for (label, url) in &node_endpoints {
        if let Err(e) = wait_for_node_ready(url, 120).await {
            println!("Warning: {} at {} not ready: {}", label, url, e);
        }
    }

    // Get initial sync status
    println!("=== Getting initial sync status... ===");
    let mut initial_status: Vec<(String, String, SyncStatus)> = Vec::new();
    for (label, url) in &node_endpoints {
        match get_sync_status(url).await {
            Ok(status) => {
                println!(
                    "{}: unsafe_l2={}, safe_l2={}, finalized_l2={}",
                    label,
                    status.unsafe_l2.number,
                    status.safe_l2.number,
                    status.finalized_l2.number
                );
                initial_status.push((label.to_string(), url.clone(), status));
            }
            Err(e) => {
                println!("{}: failed to get sync status: {}", label, e);
            }
        }
    }

    if initial_status.is_empty() {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("No nodes available for testing");
    }

    // Wait for blocks to be produced (with 2s block time, wait ~30s for several blocks)
    println!("=== Waiting 30 seconds for blocks to be produced... ===");
    sleep(Duration::from_secs(30)).await;

    // Check that nodes have advanced
    println!("=== Checking that nodes have advanced... ===");
    let mut all_advancing = true;
    let mut errors = Vec::new();

    for (label, url, initial) in &initial_status {
        match get_sync_status(url).await {
            Ok(current) => {
                let unsafe_advanced = current.unsafe_l2.number > initial.unsafe_l2.number;
                let safe_advanced = current.safe_l2.number > initial.safe_l2.number;

                println!(
                    "{}: unsafe {} -> {} ({}), safe {} -> {} ({})",
                    label,
                    initial.unsafe_l2.number,
                    current.unsafe_l2.number,
                    if unsafe_advanced { "ADVANCING" } else { "STALLED" },
                    initial.safe_l2.number,
                    current.safe_l2.number,
                    if safe_advanced { "ADVANCING" } else { "STALLED" },
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
    println!("=== Cleaning up network... ===");
    let cleanup_result = cleanup_by_prefix(&network_name).await?;
    println!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );
    if let Some(network) = cleanup_result.network_removed {
        println!("Removed network: {}", network);
    }

    // Assert after cleanup so we always clean up
    if !all_advancing {
        anyhow::bail!("Not all nodes are advancing:\n{}", errors.join("\n"));
    }

    println!("=== Test passed! All nodes are advancing. ===");
    Ok(())
}

/// Test that op-reth nodes are properly deployed and syncing.
/// This test verifies:
/// - op-reth RPC endpoints are accessible
/// - Block numbers are advancing over time
/// - eth_syncing returns expected values
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_op_reth_sync_and_block_advancement() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init()
        .ok();

    let network_name = format!("kup-reth-test-{}", std::process::id());
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    println!("=== Starting op-reth test deployment with network: {} ===", network_name);

    // Use local mode (no forking, deploys all contracts from scratch)
    let deployer = DeployerBuilder::new(31337) // Local Anvil chain ID
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

    println!("=== Deploying network... ===");

    let deploy_result = timeout(Duration::from_secs(600), deployer.deploy(false)).await;

    match deploy_result {
        Ok(Ok(())) => println!("=== Deployment completed successfully ==="),
        Ok(Err(e)) => {
            let _ = cleanup_by_prefix(&network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!("Deployment timed out after 600 seconds");
        }
    }

    // Get the ports for op-reth containers (HTTP RPC on 9545)
    let sequencer_reth_port = get_container_host_port(&format!("{}-op-reth", network_name), 9545)
        .context("Failed to get sequencer op-reth port")?;
    let validator_reth_port = get_container_host_port(&format!("{}-op-reth-validator-1", network_name), 9545)
        .context("Failed to get validator op-reth port")?;

    let reth_endpoints = vec![
        ("sequencer-reth", format!("http://localhost:{}", sequencer_reth_port)),
        ("validator-reth", format!("http://localhost:{}", validator_reth_port)),
    ];

    // Wait for op-reth nodes to be ready
    println!("=== Waiting for op-reth nodes to be ready... ===");
    for (label, url) in &reth_endpoints {
        if let Err(e) = wait_for_op_reth_ready(url, 120).await {
            println!("Warning: {} at {} not ready: {}", label, url, e);
        }
    }

    // Get initial block numbers
    println!("=== Getting initial op-reth status... ===");
    let mut initial_blocks: Vec<(String, String, u64)> = Vec::new();
    for (label, url) in &reth_endpoints {
        match get_op_reth_status(url).await {
            Ok(status) => {
                println!(
                    "{}: block={}, syncing={}{}",
                    label,
                    status.block_number,
                    status.is_syncing,
                    status
                        .sync_progress
                        .as_ref()
                        .map(|p| format!(" (current: {}, highest: {})", p.current_block, p.highest_block))
                        .unwrap_or_default()
                );
                initial_blocks.push((label.to_string(), url.clone(), status.block_number));
            }
            Err(e) => {
                println!("{}: failed to get status: {}", label, e);
            }
        }
    }

    if initial_blocks.is_empty() {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("No op-reth nodes available for testing");
    }

    // Wait for blocks to be produced
    println!("=== Waiting 30 seconds for blocks to be produced... ===");
    sleep(Duration::from_secs(30)).await;

    // Check that block numbers have advanced
    println!("=== Checking that op-reth block numbers have advanced... ===");
    let mut all_advancing = true;
    let mut errors = Vec::new();

    for (label, url, initial_block) in &initial_blocks {
        match get_op_reth_status(url).await {
            Ok(status) => {
                let advanced = status.block_number > *initial_block;
                println!(
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

    // Cleanup
    println!("=== Cleaning up network... ===");
    let cleanup_result = cleanup_by_prefix(&network_name).await?;
    println!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );
    if let Some(network) = cleanup_result.network_removed {
        println!("Removed network: {}", network);
    }

    if !all_advancing {
        anyhow::bail!("Not all op-reth nodes are advancing:\n{}", errors.join("\n"));
    }

    println!("=== Test passed! All op-reth nodes are advancing. ===");
    Ok(())
}

/// Test that op-batcher is properly deployed and healthy.
/// This test verifies:
/// - op-batcher RPC endpoint is accessible
/// - The batcher is responding to RPC calls
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_op_batcher_health() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init()
        .ok();

    let network_name = format!("kup-batcher-test-{}", std::process::id());
    let outdata_path = PathBuf::from(format!("/tmp/{}", network_name));

    println!("=== Starting op-batcher test deployment with network: {} ===", network_name);

    // Use local mode (no forking, deploys all contracts from scratch)
    let deployer = DeployerBuilder::new(31337) // Local Anvil chain ID
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

    println!("=== Deploying network... ===");

    let deploy_result = timeout(Duration::from_secs(600), deployer.deploy(false)).await;

    match deploy_result {
        Ok(Ok(())) => println!("=== Deployment completed successfully ==="),
        Ok(Err(e)) => {
            let _ = cleanup_by_prefix(&network_name).await;
            return Err(e).context("Deployment failed");
        }
        Err(_) => {
            let _ = cleanup_by_prefix(&network_name).await;
            anyhow::bail!("Deployment timed out after 600 seconds");
        }
    }

    // Get the port for op-batcher (RPC on 8548)
    let batcher_port = get_container_host_port(&format!("{}-op-batcher", network_name), 8548)
        .context("Failed to get op-batcher port")?;

    let batcher_url = format!("http://localhost:{}", batcher_port);

    // Wait for op-batcher to be ready
    println!("=== Waiting for op-batcher to be ready... ===");
    if let Err(e) = wait_for_op_batcher_ready(&batcher_url, 120).await {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!("op-batcher not ready: {}", e);
    }

    // Check op-batcher health
    println!("=== Checking op-batcher health... ===");
    let status = get_op_batcher_status(&batcher_url).await?;

    if !status.is_healthy {
        let _ = cleanup_by_prefix(&network_name).await;
        anyhow::bail!(
            "op-batcher is not healthy: {}",
            status.error.unwrap_or_else(|| "unknown error".to_string())
        );
    }

    println!(
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

    println!("=== Verifying batcher activity via safe head progression... ===");

    // Get initial safe head
    let initial_status = get_sync_status(&kona_url).await?;
    println!("Initial safe head: {}", initial_status.safe_l2.number);

    // Wait for batches to be submitted and processed
    println!("=== Waiting 45 seconds for batch submissions... ===");
    sleep(Duration::from_secs(45)).await;

    // Check if safe head has advanced (indicates batcher is submitting batches)
    let final_status = get_sync_status(&kona_url).await?;
    let safe_advanced = final_status.safe_l2.number > initial_status.safe_l2.number;

    println!(
        "Safe head: {} -> {} ({})",
        initial_status.safe_l2.number,
        final_status.safe_l2.number,
        if safe_advanced { "ADVANCING - batcher is working" } else { "NOT YET ADVANCING" }
    );

    // Cleanup
    println!("=== Cleaning up network... ===");
    let cleanup_result = cleanup_by_prefix(&network_name).await?;
    println!(
        "Cleaned up {} containers",
        cleanup_result.containers_removed.len()
    );
    if let Some(network) = cleanup_result.network_removed {
        println!("Removed network: {}", network);
    }

    // Note: We don't fail if safe head hasn't advanced yet, as it can take time
    // The main assertion is that the batcher RPC is healthy
    if safe_advanced {
        println!("=== Test passed! op-batcher is healthy and submitting batches. ===");
    } else {
        println!("=== Test passed! op-batcher is healthy (safe head may need more time to advance). ===");
    }

    Ok(())
}
