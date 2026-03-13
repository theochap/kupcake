//! Integration tests for live deployment mode.
//!
//! Live mode deploys contracts to a running Anvil instance via transactions,
//! which is slower than genesis mode but supports forking remote L1 chains.
//! These tests are separated from the main integration tests because they take
//! significantly longer to run.
//!
//! Run with: cargo test --test live_integration_test

mod common;

use anyhow::{Context, Result};
use kupcake_deploy::{DeployerBuilder, DeploymentTarget, OutDataPath, cleanup_by_prefix};

use common::*;

/// Test that deployment skipping works correctly in Live mode.
/// This test verifies:
/// - Deploy a network once (Live mode)
/// - Stop and cleanup
/// - Redeploy with same configuration (should skip contract deployment)
/// - Verify deployment version file exists and hash matches
/// - Network is healthy and advances
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_live_deployment_skipping() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("skip-test");
    tracing::info!(
        "=== Starting live deployment skipping test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .deployment_target(DeploymentTarget::Live)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await
        .context("Failed to build deployer")?;

    // First deployment - should deploy contracts
    tracing::info!("=== First deployment: deploying contracts ===");
    let config_path = deployer.save_config()?;
    tracing::info!("Configuration saved to: {}", config_path.display());

    let (docker, _deployment) = ctx.deploy(deployer).await?;
    tracing::info!("=== First deployment completed successfully ===");

    // Verify deployment version file and get hash
    let first_hash = ctx
        .get_deployment_hash()
        .inspect(|hash| tracing::info!("First deployment hash: {}", hash))?;

    // Stop and cleanup the network by dropping the first deployment and docker.
    // docker must be dropped explicitly here so there is only one KupDocker owner
    // of the network at a time — the second ctx.deploy() will reuse the same
    // network name, so without this the two instances would both try to remove it.
    tracing::info!("=== Cleaning up first deployment... ===");
    drop(_deployment);
    drop(docker);

    // Second deployment - should skip contract deployment
    tracing::info!("=== Second deployment: should skip contract deployment ===");
    let loaded_deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to load deployer from config file")?;
    tracing::info!("Configuration loaded from: {}", config_path.display());

    let start_time = std::time::Instant::now();
    let (mut _docker, deployment) = ctx.deploy(loaded_deployer).await?;
    tracing::info!(
        "=== Second deployment completed in {:?} ===",
        start_time.elapsed()
    );

    // Verify hash matches (contracts were skipped)
    let second_hash = ctx.get_deployment_hash()?;
    if first_hash != second_hash {
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
        anyhow::bail!("Failed to get sync status from redeployed network");
    }
    tracing::info!("✓ Network is healthy");

    tracing::info!("=== Test passed! Live deployment skipping works correctly. ===");
    Ok(())
}

/// Test live deployment mode (local, no fork): Anvil starts first, then contracts
/// are deployed to the running L1. This is the original deployment flow.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_live_local_deployment() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("live-local");

    tracing::info!(
        "=== Starting live local deployment test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .deployment_target(DeploymentTarget::Live)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await
        .context("Failed to build deployer in live mode")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network in live mode... ===");
    let (_docker, deployment) = ctx.deploy(deployer).await?;

    // Reload for health helpers
    let deployer =
        kupcake_deploy::Deployer::load_from_file(&ctx.outdata_path.join("Kupcake.toml"))?;

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting for blocks to advance... ===");
    wait_for_all_nodes_advancing(&deployer, 60).await?;

    let statuses = collect_all_sync_status(&deployment).await;
    assert!(
        !statuses.is_empty(),
        "Should have sync status from at least one node"
    );
    for (label, status) in &statuses {
        tracing::info!(
            "{}: unsafe_l2={}, safe_l2={}, finalized_l2={}",
            label,
            status.unsafe_l2.number,
            status.safe_l2.number,
            status.finalized_l2.number,
        );
        assert!(
            status.unsafe_l2.number > 0,
            "{} should have produced blocks (unsafe_l2 > 0)",
            label,
        );
    }

    tracing::info!("=== Cleaning up... ===");
    cleanup_by_prefix(&ctx.network_name).await?;

    tracing::info!("=== Test passed! Live local deployment works correctly. ===");
    Ok(())
}

/// Test live deployment mode with a Sepolia fork: Anvil forks Sepolia L1,
/// then contracts are deployed to the forked chain.
///
/// Requires network access to a Sepolia RPC endpoint.
/// Set `KUPCAKE_TEST_SEPOLIA_RPC` to override the default public RPC.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_live_sepolia_fork_deployment() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let sepolia_rpc = std::env::var("KUPCAKE_TEST_SEPOLIA_RPC")
        .unwrap_or_else(|_| "https://ethereum-sepolia-rpc.publicnode.com".to_string());

    let ctx = TestContext::new("live-sepolia");

    tracing::info!(
        "=== Starting live Sepolia fork test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    let deployer = DeployerBuilder::new(11155111) // Sepolia chain ID
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l1_rpc_url(&sepolia_rpc)
        .l2_node_count(2)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .deployment_target(DeploymentTarget::Live)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await
        .context("Failed to build deployer with Sepolia fork")?;

    deployer.save_config()?;

    tracing::info!("=== Deploying network with Sepolia fork... ===");
    let (_docker, deployment) = ctx.deploy(deployer).await?;

    // Reload for health helpers
    let deployer =
        kupcake_deploy::Deployer::load_from_file(&ctx.outdata_path.join("Kupcake.toml"))?;

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    tracing::info!("=== Waiting for blocks to advance... ===");
    wait_for_all_nodes_advancing(&deployer, 60).await?;

    let statuses = collect_all_sync_status(&deployment).await;
    assert!(
        !statuses.is_empty(),
        "Should have sync status from at least one node"
    );
    for (label, status) in &statuses {
        tracing::info!(
            "{}: unsafe_l2={}, safe_l2={}, finalized_l2={}",
            label,
            status.unsafe_l2.number,
            status.safe_l2.number,
            status.finalized_l2.number,
        );
        assert!(
            status.unsafe_l2.number > 0,
            "{} should have produced blocks (unsafe_l2 > 0)",
            label,
        );
    }

    tracing::info!("=== Cleaning up... ===");
    cleanup_by_prefix(&ctx.network_name).await?;

    tracing::info!("=== Test passed! Live Sepolia fork deployment works correctly. ===");
    Ok(())
}
