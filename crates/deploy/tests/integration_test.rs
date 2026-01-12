//! Integration tests for kupcake-deploy.
//!
//! These tests require Docker to be running and will deploy actual networks.
//! Run with: cargo test --test integration_test -- --ignored
//!
//! Note: These tests are marked as #[ignore] by default since they require
//! Docker and take significant time to run.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::time::sleep;

/// Response from optimism_syncStatus RPC call.
#[derive(Debug, Deserialize)]
struct SyncStatusResponse {
    result: SyncStatus,
}

/// Sync status from kona-node.
#[derive(Debug, Deserialize)]
struct SyncStatus {
    unsafe_l2: BlockRef,
    safe_l2: BlockRef,
    finalized_l2: BlockRef,
}

/// Block reference with number and hash.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BlockRef {
    number: u64,
    hash: String,
}

/// Query the sync status from a kona-node RPC endpoint.
async fn get_sync_status(rpc_url: &str) -> Result<SyncStatus> {
    let client = reqwest::Client::new();
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

    Ok(status.result)
}

#[tokio::test]
#[ignore = "requires a running network"]
async fn test_sync_status_on_running_network() -> Result<()> {
    // Initialize tracing for test output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init()
        .ok();

    // This test assumes a network is already running.
    // It queries the default kona-node ports to check sync status.
    let base_port = 7545; // Default kona-node RPC port for sequencer

    // Try to query the first node
    let url = format!("http://localhost:{}", base_port);
    tracing::info!("Querying sync status at {}", url);

    let status = get_sync_status(&url)
        .await
        .context("Failed to get sync status - is a network running?")?;

    tracing::info!(
        "Sync status: unsafe_l2={}, safe_l2={}, finalized_l2={}",
        status.unsafe_l2.number,
        status.safe_l2.number,
        status.finalized_l2.number
    );

    // Verify the heads are non-zero (network has produced blocks)
    assert!(
        status.unsafe_l2.number > 0,
        "unsafe_l2 should be greater than 0"
    );

    Ok(())
}

/// Test that verifies all nodes in a running network have advancing heads.
/// This requires a network to be running with the default port layout.
#[tokio::test]
#[ignore = "requires a running network with multiple nodes"]
async fn test_all_nodes_advancing() -> Result<()> {
    // Initialize tracing for test output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init()
        .ok();

    // Default ports for kona-node RPC endpoints
    // sequencer: 7545, validator-1: 7547, validator-2: 7549, etc.
    let node_ports = vec![
        ("sequencer", 7545),
        ("validator-1", 7547),
        ("validator-2", 7549),
    ];

    // Get initial sync status
    let mut initial_status: Vec<(String, SyncStatus)> = Vec::new();
    for (label, port) in &node_ports {
        let url = format!("http://localhost:{}", port);
        match get_sync_status(&url).await {
            Ok(status) => {
                tracing::info!(
                    "{}: unsafe_l2={}, safe_l2={}, finalized_l2={}",
                    label,
                    status.unsafe_l2.number,
                    status.safe_l2.number,
                    status.finalized_l2.number
                );
                initial_status.push((label.to_string(), status));
            }
            Err(e) => {
                tracing::warn!("{} at port {}: not available ({})", label, port, e);
            }
        }
    }

    if initial_status.is_empty() {
        anyhow::bail!("No nodes available for testing. Is the network running?");
    }

    // Wait for some blocks
    tracing::info!("Waiting 30 seconds for blocks to be produced...");
    sleep(Duration::from_secs(30)).await;

    // Check that nodes have advanced
    let mut all_ok = true;
    for (label, port) in &node_ports {
        let url = format!("http://localhost:{}", port);
        let current = match get_sync_status(&url).await {
            Ok(status) => status,
            Err(_) => continue,
        };

        if let Some((_, initial)) = initial_status.iter().find(|(l, _)| l == label) {
            let unsafe_advanced = current.unsafe_l2.number > initial.unsafe_l2.number;
            let safe_advanced = current.safe_l2.number > initial.safe_l2.number;

            tracing::info!(
                "{}: unsafe {} -> {} ({}), safe {} -> {} ({})",
                label,
                initial.unsafe_l2.number,
                current.unsafe_l2.number,
                if unsafe_advanced { "OK" } else { "STALLED" },
                initial.safe_l2.number,
                current.safe_l2.number,
                if safe_advanced { "OK" } else { "STALLED" },
            );

            if !unsafe_advanced {
                tracing::error!("{}: unsafe head not advancing!", label);
                all_ok = false;
            }
        }
    }

    assert!(all_ok, "Not all nodes are advancing");
    Ok(())
}
