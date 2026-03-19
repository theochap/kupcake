//! Integration test for network restart with Anvil clock alignment.
//!
//! Verifies that when a network is stopped and restarted, the L2 chain
//! continues advancing without stalling due to an L1 timestamp gap.
//!
//! Run with: cargo test --test restart_test

mod common;

use std::time::Duration;

use anyhow::{Context, Result};
use kupcake_deploy::{DeployerBuilder, DeploymentTarget, OutDataPath, cleanup_by_prefix, rpc};
use tokio::time::sleep;

use common::*;

/// Poll `optimism_syncStatus` until `safe_l2 >= min_safe` or timeout.
async fn wait_for_safe_l2(rpc_url: &str, min_safe: u64, timeout_secs: u64) -> Result<u64> {
    rpc::wait_until_ready("safe_l2 progress", timeout_secs, || async {
        let status: kupcake_deploy::services::SyncStatus = rpc::json_rpc_call(
            &rpc::create_client()?,
            rpc_url,
            "optimism_syncStatus",
            vec![],
        )
        .await?;

        if status.safe_l2.number >= min_safe {
            Ok(())
        } else {
            anyhow::bail!("safe_l2={} < required {}", status.safe_l2.number, min_safe)
        }
    })
    .await?;

    let status: kupcake_deploy::services::SyncStatus = rpc::json_rpc_call(
        &rpc::create_client()?,
        rpc_url,
        "optimism_syncStatus",
        vec![],
    )
    .await?;

    Ok(status.safe_l2.number)
}

/// Poll `optimism_syncStatus` until both `unsafe_l2 > prev_unsafe` and
/// `safe_l2 > prev_safe` are satisfied, confirming the chain is making
/// real forward progress (not just reporting stale pre-restart values).
async fn wait_for_progress_beyond(
    rpc_url: &str,
    prev_unsafe: u64,
    prev_safe: u64,
    timeout_secs: u64,
) -> Result<(u64, u64)> {
    rpc::wait_until_ready("post-restart progress", timeout_secs, || async {
        let status: kupcake_deploy::services::SyncStatus = rpc::json_rpc_call(
            &rpc::create_client()?,
            rpc_url,
            "optimism_syncStatus",
            vec![],
        )
        .await?;

        let unsafe_ok = status.unsafe_l2.number > prev_unsafe;
        let safe_ok = status.safe_l2.number > prev_safe;

        if unsafe_ok && safe_ok {
            Ok(())
        } else {
            anyhow::bail!(
                "waiting for progress: unsafe_l2={}/{}, safe_l2={}/{}",
                status.unsafe_l2.number,
                prev_unsafe,
                status.safe_l2.number,
                prev_safe,
            )
        }
    })
    .await?;

    let status: kupcake_deploy::services::SyncStatus = rpc::json_rpc_call(
        &rpc::create_client()?,
        rpc_url,
        "optimism_syncStatus",
        vec![],
    )
    .await?;

    Ok((status.unsafe_l2.number, status.safe_l2.number))
}

/// Test that restarting a network from persisted Anvil state does not stall.
///
/// Scenario:
/// 1. Deploy a live network; run for ~2 minutes until safe_l2 is advancing
/// 2. Record pre-restart unsafe/safe heads
/// 3. Stop (state is dumped via anvil_dumpState on KupDocker drop)
/// 4. Wait 60 seconds (creates a meaningful L1 timestamp gap)
/// 5. Restart from saved config (Anvil restores via --load-state, clock aligned)
/// 6. Verify both unsafe_l2 and safe_l2 advance beyond their pre-restart values
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_restart_no_stall() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("restart");
    tracing::info!(
        "=== Starting restart test with network: {} (L1 chain ID: {}) ===",
        ctx.network_name,
        ctx.l1_chain_id
    );

    // --- First deployment ---
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

    let config_path = deployer.save_config()?;

    tracing::info!("=== First deployment: deploying network ===");
    let (docker, deployment) = ctx.deploy(deployer).await?;

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    // Use the sequencer's kona-node RPC URL (host-accessible) for direct sync status polling.
    let kona_rpc = deployment
        .l2_stack
        .sequencers
        .first()
        .and_then(|n| n.kona_node.rpc_host_url.as_ref())
        .context("No host URL for sequencer kona-node")?
        .to_string();

    // Run for ~2 minutes, waiting until the safe head advances so we know the
    // batcher has submitted at least one batch and derivation is healthy.
    tracing::info!("=== Waiting ~2 min for safe_l2 to advance (batcher confirming batches)... ===");
    let pre_restart_safe = wait_for_safe_l2(&kona_rpc, 1, 120)
        .await
        .context("safe_l2 never advanced before restart — batcher may not be working")?;

    let status: kupcake_deploy::services::SyncStatus = rpc::json_rpc_call(
        &rpc::create_client()?,
        &kona_rpc,
        "optimism_syncStatus",
        vec![],
    )
    .await?;
    let pre_restart_unsafe = status.unsafe_l2.number;

    tracing::info!(
        pre_restart_unsafe,
        pre_restart_safe,
        "Pre-restart sync status"
    );

    // --- Stop (drop triggers state dump + container cleanup) ---
    tracing::info!("=== Stopping first deployment (state will be dumped)... ===");
    drop(deployment);
    drop(docker);

    let state_path = ctx.outdata_path.join("anvil/state.json");
    assert!(
        state_path.exists(),
        "Anvil state file should exist after dump: {}",
        state_path.display()
    );
    tracing::info!(
        "Anvil state dumped ({} bytes)",
        state_path.metadata()?.len()
    );

    // Wait 60 seconds to create a meaningful timestamp gap (5× the block_time).
    tracing::info!("=== Waiting 60s to create timestamp gap... ===");
    sleep(Duration::from_secs(60)).await;

    // --- Second deployment (restart from persisted state) ---
    tracing::info!("=== Second deployment: restarting from persisted state ===");
    let loaded_deployer =
        kupcake_deploy::Deployer::load_from_file(&config_path).context("Failed to load config")?;

    let (_docker, deployment) = ctx.deploy(loaded_deployer).await?;

    tracing::info!("=== Waiting for restarted nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    let kona_rpc_after = deployment
        .l2_stack
        .sequencers
        .first()
        .and_then(|n| n.kona_node.rpc_host_url.as_ref())
        .context("No host URL for sequencer kona-node after restart")?
        .to_string();

    // Wait up to 2 minutes for both unsafe and safe heads to advance past
    // their pre-restart values, proving the chain is making real progress.
    tracing::info!(
        "=== Waiting up to 2 min for unsafe_l2 > {} and safe_l2 > {}... ===",
        pre_restart_unsafe,
        pre_restart_safe
    );
    let (post_unsafe, post_safe) =
        wait_for_progress_beyond(&kona_rpc_after, pre_restart_unsafe, pre_restart_safe, 120)
            .await
            .context(
                "Chain did not advance past pre-restart heads within 2 minutes after restart",
            )?;

    tracing::info!(
        post_unsafe,
        post_safe,
        pre_restart_unsafe,
        pre_restart_safe,
        "Post-restart sync status — chain is advancing"
    );

    tracing::info!("=== Cleaning up... ===");
    cleanup_by_prefix(&ctx.network_name).await?;

    tracing::info!("=== Test passed! Network restart works correctly. ===");
    Ok(())
}
