//! CLI-level integration tests for the --network flag redeploy behavior.
//!
//! These tests verify that `kupcake deploy --network {name}` correctly detects
//! and loads an existing configuration from `data-{name}/Kupcake.toml`.
//!
//! These tests build a deployer config programmatically, save it to the expected
//! location, then invoke the kupcake binary to verify it detects the config.
//!
//! Run with: cargo test --test network_redeploy_test

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use kupcake_deploy::{DeployerBuilder, DeploymentTarget, OutDataPath};

/// Build a minimal deployer config and save it to `{workdir}/data-{name}/Kupcake.toml`.
async fn create_saved_deployment(workdir: &std::path::Path, network_name: &str) -> Result<PathBuf> {
    let l1_chain_id = rand::Rng::random_range(&mut rand::rng(), 100000u64..=999999);
    let outdata_path = workdir.join(format!("data-{}", network_name));

    let deployer = DeployerBuilder::new(l1_chain_id)
        .network_name(network_name)
        .outdata(OutDataPath::Path(outdata_path))
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

    deployer.save_config()
}

/// Get the path to the kupcake binary built by cargo.
fn kupcake_bin() -> PathBuf {
    // cargo sets this env var during `cargo test`
    let mut path = PathBuf::from(env!("CARGO_BIN_EXE_kupcake"));
    if !path.exists() {
        // Fallback: look in target/debug
        path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/kupcake");
    }
    path
}

/// Test that `kupcake deploy --network {name} --help` works.
/// This is a basic smoke test that the binary is built and the CLI parses.
#[test]
fn test_cli_deploy_help() {
    let output = Command::new(kupcake_bin())
        .args(["deploy", "--help"])
        .output()
        .expect("Failed to run kupcake binary");

    assert!(
        output.status.success(),
        "kupcake deploy --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--network"),
        "Help output should mention --network flag"
    );
    assert!(
        stdout.contains("--config"),
        "Help output should mention --config flag"
    );
}

/// Test that `kupcake deploy --network {name}` with an existing config produces
/// the "Found existing deployment" log message instead of creating a new one.
///
/// This is a regression test: before the fix, --network would always create a
/// new deployment, ignoring any existing data-{name}/Kupcake.toml.
#[tokio::test]
async fn test_cli_network_flag_detects_existing_deployment() -> Result<()> {
    let workdir = tempdir::TempDir::new("kupcake-cli-test").context("Failed to create temp dir")?;
    let network_name = format!("kup-cli-test-{}", std::process::id());

    // Create a saved deployment config in the workdir
    create_saved_deployment(workdir.path(), &network_name).await?;

    // Verify the config file exists where we expect it
    let expected_config = workdir
        .path()
        .join(format!("data-{}/Kupcake.toml", network_name));
    assert!(
        expected_config.exists(),
        "Config file should exist at {}",
        expected_config.display()
    );

    // Run the binary with --network pointing to the existing deployment.
    // We use --detach so it would try to actually deploy (and fail since no Docker),
    // but we just check that it prints "Found existing deployment" in the logs
    // rather than trying a fresh build.
    //
    // The binary will fail (no Docker or wrong chain), but we can still check
    // whether it found the existing config by inspecting stderr.
    let output = Command::new(kupcake_bin())
        .args([
            "deploy",
            "--network",
            &network_name,
            "--detach",
            "-v",
            "debug",
        ])
        .current_dir(workdir.path())
        .output()
        .context("Failed to run kupcake binary")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let all_output = format!("{}{}", stdout, stderr);

    // The key assertion: the binary should detect the existing config.
    // tracing output may go to either stdout or stderr depending on the subscriber.
    assert!(
        all_output.contains("Found existing deployment")
            || all_output.contains("Loading deployment from config"),
        "Binary should detect existing deployment when --network matches a saved config.\n\
         Expected 'Found existing deployment' or 'Loading deployment from config' in output.\n\
         stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );

    Ok(())
}

/// Test that `kupcake deploy --network {name}` with NO existing config does NOT
/// print the "Found existing deployment" message (it should proceed as a fresh deploy).
#[tokio::test]
async fn test_cli_network_flag_fresh_deploy_when_no_config() -> Result<()> {
    let workdir = tempdir::TempDir::new("kupcake-cli-test").context("Failed to create temp dir")?;
    let network_name = format!("kup-fresh-test-{}", std::process::id());

    // Don't create any config - this should be a fresh deployment attempt.
    let output = Command::new(kupcake_bin())
        .args([
            "deploy",
            "--network",
            &network_name,
            "--detach",
            "-v",
            "debug",
        ])
        .current_dir(workdir.path())
        .output()
        .context("Failed to run kupcake binary")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let all_output = format!("{}{}", stdout, stderr);

    // Should NOT detect an existing deployment
    assert!(
        !all_output.contains("Found existing deployment"),
        "Binary should NOT detect an existing deployment when no config exists.\n\
         stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );

    Ok(())
}
