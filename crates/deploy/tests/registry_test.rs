//! Integration tests for the devnet registry.

mod common;

use std::path::Path;

use anyhow::{Context, Result};
use kupcake_deploy::{DevnetRegistry, DevnetState, cleanup_by_prefix};

use common::*;

#[test]
fn test_prune_removes_stopped_and_datadirs() {
    use tempdir::TempDir;

    let base = TempDir::new("kupcake-registry-integ").unwrap();
    let registry = DevnetRegistry::with_base_path(base.path().to_path_buf()).unwrap();

    // Create two stopped devnets with real temp datadirs
    let dir1 = base.path().join("data-net1");
    let dir2 = base.path().join("data-net2");
    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::create_dir_all(&dir2).unwrap();

    registry.register("net1", &dir1).unwrap();
    registry.register("net2", &dir2).unwrap();
    registry.mark_stopped("net1").unwrap();
    registry.mark_stopped("net2").unwrap();

    let removed = registry.prune().unwrap();
    assert_eq!(removed.len(), 2);
    assert!(!dir1.exists());
    assert!(!dir2.exists());

    let remaining = registry.list().unwrap();
    assert!(remaining.is_empty());
}

#[test]
fn test_concurrent_registry_access() {
    use std::thread;
    use tempdir::TempDir;

    let base = TempDir::new("kupcake-registry-concurrent").unwrap();
    let base_path = base.path().to_path_buf();

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let bp = base_path.clone();
            thread::spawn(move || {
                let registry = DevnetRegistry::with_base_path(bp).unwrap();
                let name = format!("concurrent-net-{}", i);
                registry
                    .register(&name, Path::new(&format!("/tmp/data-{}", name)))
                    .unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let registry = DevnetRegistry::with_base_path(base_path).unwrap();
    let entries = registry.list().unwrap();
    assert_eq!(entries.len(), 4);

    // All should be running
    assert!(entries.iter().all(|e| e.state == DevnetState::Running));
}

/// Helper: find a registry entry by name.
fn find_entry(name: &str) -> Result<Option<kupcake_deploy::DevnetEntry>> {
    let registry = DevnetRegistry::new()?;
    let entries = registry.list()?;
    Ok(entries.into_iter().find(|e| e.name == name))
}

/// Test the full registry lifecycle through an actual deployment:
///
/// 1. Deploy a network → registry shows Running
/// 2. Drop KupDocker (triggers cleanup) → registry shows Stopped
/// 3. Re-deploy the same network → registry shows Running again
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_deploy_stop_restart_registry_lifecycle() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("reg-lifecycle");
    let network_name = ctx.network_name.clone();

    tracing::info!(
        "=== Registry lifecycle test: network={}, chain_id={} ===",
        network_name,
        ctx.l1_chain_id
    );

    // Clean up any stale entry from a previous failed run
    let registry = DevnetRegistry::new()?;
    let _ = registry.remove(&network_name);
    drop(registry);

    // ── Step 1: Deploy → should be Running ──
    tracing::info!("=== Step 1: Deploying network ===");
    let deployer = ctx.build_deployer().await?;
    let config_path = deployer.save_config()?;

    let (docker, _deployment) = ctx.deploy(deployer).await?;

    let entry = find_entry(&network_name)?.with_context(|| {
        format!(
            "Network '{}' not found in registry after deploy",
            network_name
        )
    })?;
    assert_eq!(
        entry.state,
        DevnetState::Running,
        "After deploy, registry should show Running"
    );
    tracing::info!("Step 1 passed: registry shows Running");

    // ── Step 2: Drop docker (cleanup) → should be Stopped ──
    tracing::info!("=== Step 2: Dropping KupDocker to trigger cleanup ===");
    drop(_deployment);
    drop(docker);

    let entry = find_entry(&network_name)?.with_context(|| {
        format!(
            "Network '{}' not found in registry after stop",
            network_name
        )
    })?;
    assert_eq!(
        entry.state,
        DevnetState::Stopped,
        "After cleanup, registry should show Stopped"
    );
    assert!(
        entry.stopped_at.is_some(),
        "stopped_at should be set after cleanup"
    );
    tracing::info!("Step 2 passed: registry shows Stopped");

    // ── Step 3: Re-deploy → should be Running again ──
    tracing::info!("=== Step 3: Restarting network from saved config ===");
    let deployer = kupcake_deploy::Deployer::load_from_file(&config_path)
        .context("Failed to reload config")?;
    let (docker, _deployment) = ctx.deploy(deployer).await?;

    let entry = find_entry(&network_name)?.with_context(|| {
        format!(
            "Network '{}' not found in registry after restart",
            network_name
        )
    })?;
    assert_eq!(
        entry.state,
        DevnetState::Running,
        "After restart, registry should show Running again"
    );
    assert!(
        entry.stopped_at.is_none(),
        "stopped_at should be cleared after restart"
    );
    tracing::info!("Step 3 passed: registry shows Running again");

    // ── Cleanup ──
    drop(_deployment);
    drop(docker);

    // Remove registry entry so we don't pollute the real registry
    let registry = DevnetRegistry::new()?;
    let _ = registry.remove(&network_name);

    // Clean up containers just in case
    let _ = cleanup_by_prefix(&network_name).await;

    tracing::info!("=== Registry lifecycle test passed ===");
    Ok(())
}
