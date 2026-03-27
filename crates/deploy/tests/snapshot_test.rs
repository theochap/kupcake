//! Integration tests for the snapshot export/restore workflow.
//!
//! Tests creating a snapshot from a deployed network and restoring
//! from it, verifying all nodes resume and advance correctly.
//!
//! Run with: cargo test --test snapshot_test

mod common;

use std::path::PathBuf;

use anyhow::{Context, Result};
use kupcake_deploy::{
    Deployer, DeployerBuilder, DeploymentTarget, KupDocker, OutDataPath, cleanup_by_prefix, rpc,
};

use common::*;

/// Poll `optimism_syncStatus` until `unsafe_l2 >= min_block` or timeout.
async fn wait_for_unsafe_l2(rpc_url: &str, min_block: u64, timeout_secs: u64) -> Result<u64> {
    rpc::wait_until_ready("unsafe_l2 progress", timeout_secs, || async {
        let status: kupcake_deploy::services::SyncStatus = rpc::json_rpc_call(
            &rpc::create_client()?,
            rpc_url,
            "optimism_syncStatus",
            vec![],
        )
        .await?;

        if status.unsafe_l2.number >= min_block {
            Ok(())
        } else {
            anyhow::bail!(
                "unsafe_l2={} < required {}",
                status.unsafe_l2.number,
                min_block
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

    Ok(status.unsafe_l2.number)
}

/// Create a snapshot archive from a deployed network's outdata directory.
/// The anvil_state_path should point to the dumped Anvil state file.
fn create_snapshot_archive(
    deployer: &Deployer,
    output_path: &PathBuf,
    anvil_state_path: Option<&std::path::Path>,
) -> Result<()> {
    let l2_stack_path = deployer.outdata.join("l2-stack");

    let rollup_path = l2_stack_path.join("rollup.json");
    anyhow::ensure!(
        rollup_path.exists(),
        "rollup.json not found at {}",
        rollup_path.display()
    );

    let sequencer_name = &deployer.l2_stack.sequencers[0].op_reth.container_name;
    let reth_data_path = l2_stack_path.join(format!("reth-data-{}", sequencer_name));
    anyhow::ensure!(
        reth_data_path.exists(),
        "Reth data directory not found at {}",
        reth_data_path.display()
    );

    let reth_data_path = reth_data_path
        .canonicalize()
        .context("Failed to resolve reth data path")?;

    let file = std::fs::File::create(output_path)
        .with_context(|| format!("Failed to create {}", output_path.display()))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    archive
        .append_path_with_name(&rollup_path, "rollup.json")
        .context("Failed to add rollup.json")?;

    let genesis_path = l2_stack_path.join("genesis.json");
    if genesis_path.exists() {
        archive
            .append_path_with_name(&genesis_path, "genesis.json")
            .context("Failed to add genesis.json")?;
    }

    let intent_path = l2_stack_path.join("intent.toml");
    if intent_path.exists() {
        archive
            .append_path_with_name(&intent_path, "intent.toml")
            .context("Failed to add intent.toml")?;
    }

    // Add Anvil state if provided (needed for L1 history on restore)
    if let Some(state_path) = anvil_state_path.filter(|p| p.exists()) {
        archive
            .append_path_with_name(state_path, "anvil-state.json")
            .context("Failed to add anvil state")?;
    }

    let reth_dir_name = reth_data_path
        .file_name()
        .context("Invalid reth data directory name")?;
    archive
        .append_dir_all(reth_dir_name, &reth_data_path)
        .context("Failed to add reth data directory")?;

    let encoder = archive.into_inner().context("Failed to finalize archive")?;
    encoder.finish().context("Failed to finish gzip")?;

    tracing::info!(
        path = %output_path.display(),
        size_bytes = std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0),
        "Snapshot archive created"
    );

    Ok(())
}

/// Extract a snapshot archive to a directory.
fn extract_snapshot(archive_path: &PathBuf, dest: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("Failed to open {}", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(dest)
        .with_context(|| format!("Failed to extract to {}", dest.display()))?;
    Ok(())
}

/// Get the kona-node RPC host URL from a deployment result.
fn sequencer_kona_rpc(deployment: &kupcake_deploy::DeploymentResult) -> Result<String> {
    deployment
        .l2_stack
        .sequencers
        .first()
        .and_then(|n| n.kona_node.rpc_host_url.as_ref())
        .context("No host URL for sequencer kona-node")
        .map(|u| u.to_string())
}

/// Test the full snapshot export → restore workflow using genesis mode.
///
/// 1. Deploy a network, wait for blocks to advance
/// 2. Create a snapshot archive
/// 3. Stop the network
/// 4. Deploy a new network from the snapshot
/// 5. Verify the restored network advances beyond the snapshot point
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_snapshot_genesis_mode() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("snap-gen");
    tracing::info!(
        "=== Starting snapshot genesis test: {} ===",
        ctx.network_name
    );

    // --- Deploy original network ---
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2)
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
    let (_docker, deployment) = ctx.deploy(deployer).await?;

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    let kona_rpc = sequencer_kona_rpc(&deployment)?;

    // Wait for some blocks to advance
    tracing::info!("=== Waiting for blocks to advance... ===");
    let snapshot_block = wait_for_unsafe_l2(&kona_rpc, 5, 120)
        .await
        .context("Blocks never advanced before snapshot")?;
    tracing::info!(snapshot_block, "Taking snapshot at block");

    // --- Dump Anvil state and create snapshot ---
    let anvil_rpc = deployment
        .anvil
        .l1_host_url
        .as_ref()
        .context("No Anvil host URL")?
        .to_string();
    let anvil_state_dump = ctx.outdata_path.join("anvil-state-dump.json");
    rpc::anvil_dump_state(&anvil_rpc, &anvil_state_dump).await?;

    let loaded_deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let snapshot_archive = ctx.outdata_path.join("snapshot.tar.gz");
    create_snapshot_archive(
        &loaded_deployer,
        &snapshot_archive,
        Some(anvil_state_dump.as_path()),
    )?;

    // --- Stop original network ---
    tracing::info!("=== Stopping original network... ===");
    drop(deployment);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;

    // --- Extract snapshot and deploy new network from it ---
    let snapshot_dir = ctx.outdata_path.join("snapshot-extracted");
    extract_snapshot(&snapshot_archive, &snapshot_dir)?;

    // Use a new network name but same chain IDs — the snapshot encodes chain config
    let restore_name = format!("{}-restored", ctx.network_name);
    let restore_outdata = ctx.outdata_path.parent().unwrap().join(&restore_name);
    tracing::info!("=== Restoring from snapshot into: {} ===", restore_name);

    let restored_deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&restore_name)
        .outdata(OutDataPath::Path(restore_outdata.clone()))
        .l2_node_count(2)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .deployment_target(DeploymentTarget::Genesis)
        .snapshot(&snapshot_dir)
        .copy_snapshot(true)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await
        .context("Failed to build restored deployer")?;

    restored_deployer.save_config()?;
    let mut docker2 = KupDocker::new(restored_deployer.docker.clone()).await?;
    let deployment2 = restored_deployer
        .deploy(&mut docker2, false, false)
        .await
        .context("Failed to deploy from snapshot")?;

    tracing::info!("=== Waiting for restored nodes to be ready... ===");
    wait_for_all_nodes(&deployment2).await;

    let kona_rpc2 = sequencer_kona_rpc(&deployment2)?;

    // --- Verify restored network advances beyond snapshot point ---
    tracing::info!(
        "=== Waiting for restored network to advance beyond block {}... ===",
        snapshot_block
    );
    let restored_block = wait_for_unsafe_l2(&kona_rpc2, snapshot_block + 1, 120)
        .await
        .context("Restored network did not advance beyond snapshot block")?;

    tracing::info!(
        snapshot_block,
        restored_block,
        "Restored network is advancing"
    );

    assert!(
        restored_block > snapshot_block,
        "Restored network should advance beyond snapshot block: {} <= {}",
        restored_block,
        snapshot_block
    );

    // --- Cleanup ---
    tracing::info!("=== Cleaning up... ===");
    drop(deployment2);
    drop(docker2);
    cleanup_by_prefix(&restore_name).await?;

    tracing::info!("=== test_snapshot_genesis_mode passed ===");
    Ok(())
}

/// Test the full snapshot export → restore workflow using live mode.
///
/// Same as genesis test but uses DeploymentTarget::Live to ensure
/// snapshots work with both deployment targets.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_snapshot_live_mode() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("snap-live");
    tracing::info!("=== Starting snapshot live test: {} ===", ctx.network_name);

    // --- Deploy original network (live mode) ---
    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .deployment_target(DeploymentTarget::Live)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;
    let (_docker, deployment) = ctx.deploy(deployer).await?;

    tracing::info!("=== Waiting for nodes to be ready... ===");
    wait_for_all_nodes(&deployment).await;

    let kona_rpc = sequencer_kona_rpc(&deployment)?;

    // Wait for some blocks to advance
    tracing::info!("=== Waiting for blocks to advance... ===");
    let snapshot_block = wait_for_unsafe_l2(&kona_rpc, 5, 120)
        .await
        .context("Blocks never advanced before snapshot")?;
    tracing::info!(snapshot_block, "Taking snapshot at block");

    // --- Dump Anvil state and create snapshot ---
    let anvil_rpc = deployment
        .anvil
        .l1_host_url
        .as_ref()
        .context("No Anvil host URL")?
        .to_string();
    let anvil_state_dump = ctx.outdata_path.join("anvil-state-dump.json");
    rpc::anvil_dump_state(&anvil_rpc, &anvil_state_dump).await?;

    let loaded_deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let snapshot_archive = ctx.outdata_path.join("snapshot.tar.gz");
    create_snapshot_archive(
        &loaded_deployer,
        &snapshot_archive,
        Some(anvil_state_dump.as_path()),
    )?;

    // --- Stop original network ---
    tracing::info!("=== Stopping original network... ===");
    drop(deployment);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;

    // --- Extract snapshot and deploy new network from it ---
    let snapshot_dir = ctx.outdata_path.join("snapshot-extracted");
    extract_snapshot(&snapshot_archive, &snapshot_dir)?;

    // Use a new network name but same chain IDs — the snapshot encodes chain config
    let restore_name = format!("{}-restored", ctx.network_name);
    let restore_outdata = ctx.outdata_path.parent().unwrap().join(&restore_name);
    tracing::info!("=== Restoring from snapshot into: {} ===", restore_name);

    let restored_deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&restore_name)
        .outdata(OutDataPath::Path(restore_outdata.clone()))
        .l2_node_count(2)
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .deployment_target(DeploymentTarget::Live)
        .snapshot(&snapshot_dir)
        .copy_snapshot(true)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await
        .context("Failed to build restored deployer")?;

    restored_deployer.save_config()?;
    let mut docker2 = KupDocker::new(restored_deployer.docker.clone()).await?;
    let deployment2 = restored_deployer
        .deploy(&mut docker2, false, false)
        .await
        .context("Failed to deploy from snapshot")?;

    tracing::info!("=== Waiting for restored nodes to be ready... ===");
    wait_for_all_nodes(&deployment2).await;

    let kona_rpc2 = sequencer_kona_rpc(&deployment2)?;

    // --- Verify restored network advances beyond snapshot point ---
    tracing::info!(
        "=== Waiting for restored network to advance beyond block {}... ===",
        snapshot_block
    );
    let restored_block = wait_for_unsafe_l2(&kona_rpc2, snapshot_block + 1, 120)
        .await
        .context("Restored network did not advance beyond snapshot block")?;

    tracing::info!(
        snapshot_block,
        restored_block,
        "Restored network is advancing"
    );

    assert!(
        restored_block > snapshot_block,
        "Restored network should advance beyond snapshot block: {} <= {}",
        restored_block,
        snapshot_block
    );

    // --- Cleanup ---
    tracing::info!("=== Cleaning up... ===");
    drop(deployment2);
    drop(docker2);
    cleanup_by_prefix(&restore_name).await?;

    tracing::info!("=== test_snapshot_live_mode passed ===");
    Ok(())
}
