//! Integration tests for node lifecycle management.
//!
//! Tests adding, removing, pausing, unpausing, and restarting L2 nodes
//! on a running network.
//!
//! Run with: cargo test --test node_lifecycle_test

mod common;

use std::time::Duration;

use anyhow::{Context, Result};
use kupcake_deploy::{
    ContainerState, Deployer, DeployerBuilder, DeploymentTarget, KupDocker, KupDockerConfig,
    OutDataPath, cleanup_by_prefix, health, node_lifecycle, status,
};
use tokio::time::sleep;

use common::*;

/// Deploy a minimal network for lifecycle tests: 1 sequencer + 1 validator.
async fn deploy_minimal_network(
    ctx: &TestContext,
) -> Result<(KupDocker, kupcake_deploy::DeploymentResult)> {
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2) // 1 sequencer + 1 validator
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .deployment_target(DeploymentTarget::Genesis)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;
    ctx.deploy(deployer).await
}

/// Create a KupDocker client for node lifecycle operations (no cleanup on drop).
async fn lifecycle_docker(deployer: &Deployer) -> Result<KupDocker> {
    KupDocker::new(KupDockerConfig {
        no_cleanup: true,
        ..deployer.docker.clone()
    })
    .await
    .context("Failed to create Docker client for lifecycle operations")
}

/// Test adding a new validator node and verifying it syncs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_add_validator_and_sync() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("add-val");
    tracing::info!("=== Starting add validator test: {} ===", ctx.network_name);

    let (_docker, deployment) = deploy_minimal_network(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    // Load the deployer config (it should have P2P keys persisted)
    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let mut docker = lifecycle_docker(&deployer).await?;

    // Add a new validator
    let handler = node_lifecycle::add_validator(&mut deployer, &mut docker).await?;
    assert!(
        handler.kona_node.rpc_host_url.is_some()
            || !handler.kona_node.rpc_url.to_string().is_empty(),
        "New validator should have a kona-node RPC URL"
    );

    // Verify the config was updated
    let reloaded = Deployer::load_from_file(&ctx.outdata_path)?;
    assert_eq!(
        reloaded.l2_stack.validators.len(),
        2,
        "Should have 2 validators after adding one"
    );

    // Wait for the new validator to start syncing (block > 0)
    tracing::info!("Waiting for new validator to sync...");
    wait_for_all_nodes_advancing(&reloaded, 120).await?;

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_add_validator_and_sync passed ===");
    Ok(())
}

/// Test removing a validator node.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_remove_validator() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("rm-val");
    tracing::info!(
        "=== Starting remove validator test: {} ===",
        ctx.network_name
    );

    // Deploy with 1 sequencer + 2 validators
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(3) // 1 seq + 2 val
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .deployment_target(DeploymentTarget::Genesis)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await?;
    deployer.save_config()?;
    let (_docker, deployment) = ctx.deploy(deployer).await?;
    wait_for_all_nodes(&deployment).await;

    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    assert_eq!(deployer.l2_stack.validators.len(), 2);

    let docker = lifecycle_docker(&deployer).await?;

    // Remove validator-2
    node_lifecycle::remove_node(&mut deployer, &docker, "validator-2", false).await?;

    assert_eq!(
        deployer.l2_stack.validators.len(),
        1,
        "Should have 1 validator after removal"
    );

    // Verify config was persisted
    let reloaded = Deployer::load_from_file(&ctx.outdata_path)?;
    assert_eq!(reloaded.l2_stack.validators.len(), 1);

    // Verify the network is still healthy (sequencer + validator-1)
    let health = health::health_check(&reloaded).await?;
    assert!(
        health.l1.running,
        "L1 should still be running after validator removal"
    );

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_remove_validator passed ===");
    Ok(())
}

/// Test that removing a validator persists across network restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_remove_validator_persists_across_restart() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("rm-persist");
    tracing::info!(
        "=== Starting remove persistence test: {} ===",
        ctx.network_name
    );

    // Deploy with 1 seq + 2 val
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(3)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .deployment_target(DeploymentTarget::Genesis)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await?;
    deployer.save_config()?;
    let (_docker, deployment) = ctx.deploy(deployer).await?;
    wait_for_all_nodes(&deployment).await;

    // Remove validator-2
    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let docker = lifecycle_docker(&deployer).await?;
    node_lifecycle::remove_node(&mut deployer, &docker, "validator-2", false).await?;
    drop(docker);

    // Stop everything
    drop(deployment);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;

    // Reload config and verify validator-2 is still gone
    let reloaded = Deployer::load_from_file(&ctx.outdata_path)?;
    assert_eq!(
        reloaded.l2_stack.validators.len(),
        1,
        "Removed validator should stay removed after config reload"
    );

    tracing::info!("=== test_remove_validator_persists_across_restart passed ===");
    Ok(())
}

/// Test pausing and unpausing a validator node.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_pause_unpause_validator() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("pause");
    tracing::info!("=== Starting pause/unpause test: {} ===", ctx.network_name);

    let (_docker, deployment) = deploy_minimal_network(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    let deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let docker = lifecycle_docker(&deployer).await?;

    // Pause validator-1
    node_lifecycle::pause_node(&deployer, &docker, "validator-1").await?;

    // Verify it's paused
    let val_reth_name = &deployer.l2_stack.validators[0].op_reth.container_name;
    let state = docker.get_container_state(val_reth_name).await;
    assert_eq!(
        state,
        ContainerState::Paused,
        "Validator op-reth should be paused"
    );

    // Unpause
    node_lifecycle::unpause_node(&deployer, &docker, "validator-1").await?;

    // Verify it's running again
    let state = docker.get_container_state(val_reth_name).await;
    assert_eq!(
        state,
        ContainerState::Running,
        "Validator op-reth should be running after unpause"
    );

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_pause_unpause_validator passed ===");
    Ok(())
}

/// Test restarting a validator node.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_restart_validator() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("restart-val");
    tracing::info!("=== Starting restart test: {} ===", ctx.network_name);

    let (_docker, deployment) = deploy_minimal_network(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    let deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let docker = lifecycle_docker(&deployer).await?;

    // Restart validator-1
    node_lifecycle::restart_node(&deployer, &docker, "validator-1").await?;

    // Give it a moment to come back up
    sleep(Duration::from_secs(5)).await;

    // Verify it's running
    let val_reth_name = &deployer.l2_stack.validators[0].op_reth.container_name;
    let state = docker.get_container_state(val_reth_name).await;
    assert_eq!(
        state,
        ContainerState::Running,
        "Validator op-reth should be running after restart"
    );

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_restart_validator passed ===");
    Ok(())
}

/// Test that removing the primary sequencer is not allowed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_cannot_remove_primary_sequencer() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("no-rm-seq");
    tracing::info!(
        "=== Starting cannot-remove-sequencer test: {} ===",
        ctx.network_name
    );

    let (_docker, deployment) = deploy_minimal_network(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let docker = lifecycle_docker(&deployer).await?;

    // Try to remove the primary sequencer
    let result = node_lifecycle::remove_node(&mut deployer, &docker, "sequencer", false).await;
    assert!(
        result.is_err(),
        "Should not be able to remove primary sequencer"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Cannot remove the primary sequencer"),
        "Error should mention primary sequencer, got: {}",
        err
    );

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_cannot_remove_primary_sequencer passed ===");
    Ok(())
}

/// Test that removing a nonexistent node returns an error.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_cannot_remove_nonexistent_node() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("no-rm-ghost");
    tracing::info!(
        "=== Starting cannot-remove-nonexistent test: {} ===",
        ctx.network_name
    );

    let (_docker, deployment) = deploy_minimal_network(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let docker = lifecycle_docker(&deployer).await?;

    // Try to remove a nonexistent validator
    let result = node_lifecycle::remove_node(&mut deployer, &docker, "validator-99", false).await;
    assert!(
        result.is_err(),
        "Should not be able to remove nonexistent node"
    );

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_cannot_remove_nonexistent_node passed ===");
    Ok(())
}

/// Test adding multiple validators sequentially.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_add_multiple_validators() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("add-multi");
    tracing::info!(
        "=== Starting add multiple validators test: {} ===",
        ctx.network_name
    );

    // Deploy with just 1 sequencer (no validators)
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(1) // 1 sequencer only
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .deployment_target(DeploymentTarget::Genesis)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await?;
    deployer.save_config()?;
    let (_docker, deployment) = ctx.deploy(deployer).await?;
    wait_for_all_nodes(&deployment).await;

    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let mut docker = lifecycle_docker(&deployer).await?;

    // Add 2 validators
    node_lifecycle::add_validator(&mut deployer, &mut docker).await?;
    node_lifecycle::add_validator(&mut deployer, &mut docker).await?;

    assert_eq!(
        deployer.l2_stack.validators.len(),
        2,
        "Should have 2 validators"
    );

    // Reload and verify config
    let reloaded = Deployer::load_from_file(&ctx.outdata_path)?;
    assert_eq!(reloaded.l2_stack.validators.len(), 2);

    // Wait for all nodes to be advancing
    tracing::info!("Waiting for all nodes to sync...");
    wait_for_all_nodes_advancing(&reloaded, 120).await?;

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_add_multiple_validators passed ===");
    Ok(())
}

/// Test removing the last validator (network should continue with just the sequencer).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_remove_last_validator() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("rm-last");
    tracing::info!(
        "=== Starting remove last validator test: {} ===",
        ctx.network_name
    );

    let (_docker, deployment) = deploy_minimal_network(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let docker = lifecycle_docker(&deployer).await?;

    // Remove the only validator
    node_lifecycle::remove_node(&mut deployer, &docker, "validator-1", false).await?;

    assert_eq!(
        deployer.l2_stack.validators.len(),
        0,
        "Should have 0 validators"
    );

    // Verify the sequencer is still healthy
    let health = health::health_check(&deployer).await?;
    assert!(health.l1.running, "L1 should still be running");
    assert!(
        health
            .nodes
            .iter()
            .any(|n| n.role == "sequencer" && n.execution.running),
        "Sequencer should still be running"
    );

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_remove_last_validator passed ===");
    Ok(())
}

/// Test the status command reports correct container states.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_status_command() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("status");
    tracing::info!("=== Starting status command test: {} ===", ctx.network_name);

    let (_docker, deployment) = deploy_minimal_network(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    let deployer = Deployer::load_from_file(&ctx.outdata_path)?;

    // Get network status
    let net_status = status::network_status(&deployer).await?;

    // Verify L1 is running
    assert_eq!(
        net_status.l1.state,
        ContainerState::Running,
        "L1 should be running"
    );

    // Verify all nodes are running
    for node in &net_status.nodes {
        assert_eq!(
            node.execution.state,
            ContainerState::Running,
            "Node {} op-reth should be running",
            node.label
        );
        assert_eq!(
            node.consensus.state,
            ContainerState::Running,
            "Node {} kona-node should be running",
            node.label
        );
    }

    // Verify we have the right number of nodes
    assert_eq!(
        net_status.nodes.len(),
        2,
        "Should have 2 nodes (1 seq + 1 val)"
    );

    // Verify display formatting works
    let display = format!("{}", net_status);
    assert!(
        display.contains("[ok]"),
        "Status should show [ok] for running containers"
    );
    assert!(
        display.contains("sequencer"),
        "Status should mention sequencer"
    );
    assert!(
        display.contains("validator"),
        "Status should mention validator"
    );

    tracing::info!("=== Cleaning up ===");
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_status_command passed ===");
    Ok(())
}

/// Test resolve_node with various identifiers (unit test, no Docker needed).
///
/// Uses the DeployerBuilder to construct a valid Deployer, avoiding brittle TOML fixtures.
#[tokio::test]
async fn test_resolve_node_identifiers() -> Result<()> {
    init_test_tracing();

    // Build a deployer with 1 sequencer + 2 validators
    let ctx = TestContext::new("resolve");
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(3) // 1 seq + 2 val
        .sequencer_count(1)
        .block_time(2)
        .deployment_target(DeploymentTarget::Genesis)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await?;

    // Test valid identifiers
    let loc = node_lifecycle::resolve_node(&deployer, "sequencer")?;
    assert_eq!(loc, node_lifecycle::NodeLocation::Sequencer(0));

    let loc = node_lifecycle::resolve_node(&deployer, "validator-1")?;
    assert_eq!(loc, node_lifecycle::NodeLocation::Validator(0));

    let loc = node_lifecycle::resolve_node(&deployer, "validator-2")?;
    assert_eq!(loc, node_lifecycle::NodeLocation::Validator(1));

    // Test invalid identifiers
    assert!(node_lifecycle::resolve_node(&deployer, "validator-99").is_err());
    assert!(node_lifecycle::resolve_node(&deployer, "validator-0").is_err());
    assert!(node_lifecycle::resolve_node(&deployer, "unknown-node").is_err());
    assert!(node_lifecycle::resolve_node(&deployer, "sequencer-5").is_err());

    Ok(())
}
