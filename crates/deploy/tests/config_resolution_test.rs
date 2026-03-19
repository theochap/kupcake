//! Integration tests for configuration save/load and network name resolution.
//!
//! These tests verify that a deployer configuration can be saved to the default
//! data directory (`data-{network-name}/Kupcake.toml`) and correctly loaded back,
//! which is the mechanism used when redeploying by network name (`--network`).
//!
//! These tests do NOT require Docker and can run quickly.
//!
//! Run with: cargo test --test config_resolution_test

mod common;

use std::path::PathBuf;

use anyhow::{Context, Result};
use kupcake_deploy::{Deployer, DeployerBuilder, DeploymentTarget, OutDataPath};

use common::*;

/// Test that a deployer config saved to `data-{name}/Kupcake.toml` can be loaded
/// back using `Deployer::load_from_file` with the directory path.
///
/// This is the core mechanism behind redeploying by network name.
#[tokio::test]
async fn test_config_save_and_load_by_directory() -> Result<()> {
    init_test_tracing();
    let ctx = TestContext::new("cfg-dir");

    let deployer = ctx.build_deployer().await?;
    let config_path = deployer.save_config()?;

    // Load by directory path (the way --network resolution works)
    let loaded = Deployer::load_from_file(&ctx.outdata_path)
        .context("Failed to load config from directory")?;

    assert_eq!(deployer.l1_chain_id, loaded.l1_chain_id);
    assert_eq!(deployer.l2_chain_id, loaded.l2_chain_id);
    assert_eq!(deployer.outdata, loaded.outdata);
    assert_eq!(deployer.docker.net_name, loaded.docker.net_name);

    tracing::info!(
        config_path = %config_path.display(),
        "Config round-trip via directory path successful"
    );
    Ok(())
}

/// Test that loading by file path also works (--config /path/to/Kupcake.toml).
#[tokio::test]
async fn test_config_save_and_load_by_file_path() -> Result<()> {
    init_test_tracing();
    let ctx = TestContext::new("cfg-file");

    let deployer = ctx.build_deployer().await?;
    let config_path = deployer.save_config()?;

    let loaded =
        Deployer::load_from_file(&config_path).context("Failed to load config from file path")?;

    assert_eq!(deployer, loaded);

    tracing::info!("Config round-trip via file path successful");
    Ok(())
}

/// Test that a deployer built with a specific network name saves to the expected
/// `data-{name}/` directory, and that the config file is at the expected path.
///
/// This verifies the invariant that `--network foo` will produce a config at
/// `data-foo/Kupcake.toml`, which is what `resolve_deploy_config` checks.
#[tokio::test]
async fn test_config_saved_at_expected_network_path() -> Result<()> {
    init_test_tracing();

    let l1_chain_id = generate_random_l1_chain_id();
    let network_name = format!("kup-cfg-path-{}", l1_chain_id);

    // Use a temp dir as the base so we don't pollute the working directory,
    // but mirror the default data-{name} convention inside it.
    let base_tmp = std::env::var("KUPCAKE_TEST_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let outdata_path = base_tmp.join(format!("data-{}", network_name));

    let deployer = DeployerBuilder::new(l1_chain_id)
        .network_name(&network_name)
        .outdata(OutDataPath::Path(outdata_path.clone()))
        .l2_node_count(1)
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

    let config_path = deployer.save_config()?;

    // Verify the config file exists at {outdata}/Kupcake.toml
    assert!(config_path.exists(), "Config file should exist after save");
    assert_eq!(
        config_path.file_name().unwrap().to_str().unwrap(),
        "Kupcake.toml"
    );
    assert_eq!(config_path.parent().unwrap(), deployer.outdata.as_path());

    tracing::info!(
        config = %config_path.display(),
        "Config saved at expected path"
    );
    Ok(())
}

/// Test the full redeploy-by-network-name scenario:
/// 1. Build a deployer with a known network name
/// 2. Save its config
/// 3. Load the config back (simulating what --network does)
/// 4. Verify all deployment-relevant parameters match
///
/// This is a regression test for the bug where `--network {name}` would create
/// a fresh deployment instead of loading the existing config, causing mismatched
/// chain IDs and failed redeployments.
#[tokio::test]
async fn test_redeploy_by_network_name_preserves_chain_ids() -> Result<()> {
    init_test_tracing();
    let ctx = TestContext::new("redeploy-name");

    let original = ctx.build_deployer().await?;
    let original_l1_chain_id = original.l1_chain_id;
    let original_l2_chain_id = original.l2_chain_id;
    let original_net_name = original.docker.net_name.clone();

    // Save config (simulates first deploy)
    original.save_config()?;

    // Simulate what the CLI does when --network is given:
    // resolve_config_path(name) -> data-{name} -> load Kupcake.toml
    let loaded = Deployer::load_from_file(&ctx.outdata_path)
        .context("Failed to load config by network directory")?;

    // The critical assertion: chain IDs must match.
    // Before the fix, --network would create a new DeployerBuilder with random chain IDs.
    assert_eq!(
        loaded.l1_chain_id, original_l1_chain_id,
        "L1 chain ID must match after loading by network name"
    );
    assert_eq!(
        loaded.l2_chain_id, original_l2_chain_id,
        "L2 chain ID must match after loading by network name"
    );
    assert_eq!(
        loaded.docker.net_name, original_net_name,
        "Docker network name must match after loading by network name"
    );

    // Also verify full equality (all fields preserved)
    assert_eq!(original, loaded, "Full deployer config must round-trip");

    tracing::info!(
        l1_chain_id = original_l1_chain_id,
        l2_chain_id = original_l2_chain_id,
        "Redeploy by network name correctly preserves chain IDs"
    );
    Ok(())
}

/// Test that loading from a non-existent network directory fails gracefully.
#[tokio::test]
async fn test_load_nonexistent_network_fails() -> Result<()> {
    init_test_tracing();

    let path = PathBuf::from("/tmp/data-kup-nonexistent-12345");
    let result = Deployer::load_from_file(&path);

    assert!(
        result.is_err(),
        "Loading from non-existent directory should fail"
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("not found"),
        "Error should mention 'not found', got: {}",
        err_msg
    );

    tracing::info!("Non-existent network directory correctly rejected");
    Ok(())
}
