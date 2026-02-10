//! Spam module for generating continuous L2 traffic using Flashbots Contender.
//!
//! Runs a Contender Docker container against a deployed kupcake L2 network,
//! automatically funding the spammer account via the L1→L2 faucet deposit.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bollard::{
    Docker,
    container::{
        Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StopContainerOptions,
    },
    secret::HostConfig,
};
use futures::StreamExt;
use serde_json::Value;

use crate::{Deployer, faucet};

/// Default Docker image for Contender.
pub const CONTENDER_DEFAULT_IMAGE: &str = "flashbots/contender";
/// Default Docker tag for Contender.
pub const CONTENDER_DEFAULT_TAG: &str = "latest";

/// Configuration for a spam run.
pub struct SpamConfig {
    /// Scenario name (built-in) or path to custom TOML file.
    pub scenario: String,
    /// Transactions per second.
    pub tps: u64,
    /// Duration in seconds (ignored if `forever` is true).
    pub duration: u64,
    /// Run indefinitely until Ctrl+C.
    pub forever: bool,
    /// Number of spammer accounts to use.
    pub accounts: u64,
    /// Minimum balance (ETH) for spammer accounts.
    pub min_balance: String,
    /// Amount of ETH to fund the funder account on L2.
    pub fund_amount: f64,
    /// Index of the funder account in anvil.json.
    pub funder_account_index: usize,
    /// Generate a report after completion.
    pub report: bool,
    /// Docker image for Contender.
    pub contender_image: String,
    /// Docker tag for Contender.
    pub contender_tag: String,
    /// Target sequencer index (into deployer.l2_stack.sequencers).
    pub target_node: usize,
    /// Extra arguments passed directly to contender.
    pub extra_args: Vec<String>,
}

/// Run the Contender spammer against a deployed L2 network.
pub async fn run_spam(deployer: &Deployer, config: &SpamConfig) -> Result<()> {
    // Validate target node index
    if config.target_node >= deployer.l2_stack.sequencers.len() {
        anyhow::bail!(
            "Target node index {} is out of range (only {} sequencer(s) available)",
            config.target_node,
            deployer.l2_stack.sequencers.len()
        );
    }

    // Load funder account from anvil.json
    let (funder_address, funder_private_key) =
        load_funder_account(&deployer.outdata, config.funder_account_index)?;

    tracing::info!(
        funder_address = %funder_address,
        funder_index = config.funder_account_index,
        fund_amount = config.fund_amount,
        "Funding spammer account on L2 via faucet deposit..."
    );

    // Fund the funder on L2 via faucet deposit (wait for confirmation)
    faucet::faucet_deposit(deployer, &funder_address, config.fund_amount, true)
        .await
        .context("Failed to fund spammer account on L2")?;

    tracing::info!("Funder account funded on L2");

    // Resolve scenario (built-in name vs custom file path)
    let (scenario_arg, scenario_file) = resolve_scenario(&config.scenario)?;

    // Connect to Docker
    let docker = Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker daemon")?;

    // Build internal Docker RPC URL for the target sequencer
    let seq = &deployer.l2_stack.sequencers[config.target_node];
    let rpc_url = format!("http://{}:{}/", seq.op_reth.container_name, seq.op_reth.http_port);
    tracing::info!(rpc_url = %rpc_url, "Targeting sequencer RPC (Docker-internal)");

    // Create contender data directory for DB persistence
    let contender_data_dir = deployer.outdata.join("contender");
    std::fs::create_dir_all(&contender_data_dir)
        .with_context(|| format!("Failed to create {}", contender_data_dir.display()))?;

    let container_name = container_name(deployer);

    // Remove stale contender container if it exists
    remove_stale_container(&docker, &container_name).await;

    // Build contender command
    let cmd = build_contender_cmd(config, &scenario_arg, &rpc_url, &funder_private_key);

    // Build volume mounts
    let mut binds = vec![format!(
        "{}:/root/.contender/:rw",
        contender_data_dir
            .canonicalize()
            .unwrap_or(contender_data_dir.clone())
            .display()
    )];

    // If custom scenario file, mount it read-only
    if let Some(ref file_path) = scenario_file {
        let abs_path = file_path
            .canonicalize()
            .with_context(|| format!("Scenario file not found: {}", file_path.display()))?;
        binds.push(format!(
            "{}:/scenarios/{}:ro",
            abs_path.display(),
            file_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
    }

    let image_ref = format!("{}:{}", config.contender_image, config.contender_tag);

    // Pull the image
    tracing::info!(image = %image_ref, "Pulling Contender image...");
    pull_image(&docker, &config.contender_image, &config.contender_tag).await?;

    // Create container config
    let host_config = HostConfig {
        binds: Some(binds),
        network_mode: Some(deployer.docker.net_name.clone()),
        ..Default::default()
    };

    let container_config = Config {
        image: Some(image_ref),
        cmd: Some(cmd),
        host_config: Some(host_config),
        ..Default::default()
    };

    tracing::info!(container = %container_name, "Starting Contender container...");

    // Create and start container
    docker
        .create_container(
            Some(CreateContainerOptions {
                name: container_name.as_str(),
                ..Default::default()
            }),
            container_config,
        )
        .await
        .context("Failed to create Contender container")?;

    docker
        .start_container::<String>(&container_name, None)
        .await
        .context("Failed to start Contender container")?;

    tracing::info!("Contender is running, streaming logs...");

    // Stream logs and handle completion/Ctrl+C
    let result = if config.forever {
        stream_until_ctrl_c(&docker, &container_name).await
    } else {
        stream_until_exit(&docker, &container_name).await
    };

    // Cleanup container
    cleanup_container(&docker, &container_name).await;

    result
}

/// Load a funder account (address + private key) from `anvil.json` at the given index.
fn load_funder_account(outdata: &Path, index: usize) -> Result<(String, String)> {
    let anvil_path = outdata.join("anvil/anvil.json");
    let content = std::fs::read_to_string(&anvil_path)
        .with_context(|| format!("Failed to read {}", anvil_path.display()))?;
    let data: Value = serde_json::from_str(&content).context("Failed to parse anvil.json")?;

    let address = data["available_accounts"]
        .get(index)
        .and_then(|v| v.as_str())
        .with_context(|| {
            format!(
                "Account index {} not found in anvil.json (available_accounts)",
                index
            )
        })?
        .to_string();

    let private_key = data["private_keys"]
        .get(index)
        .and_then(|v| v.as_str())
        .with_context(|| {
            format!(
                "Private key index {} not found in anvil.json (private_keys)",
                index
            )
        })?
        .to_string();

    Ok((address, private_key))
}

/// Resolve a scenario argument to a contender argument and optional file path.
///
/// Built-in scenarios (e.g., "transfers", "erc20") are passed directly.
/// Custom files (containing `/` or ending in `.toml`) are resolved as file paths.
fn resolve_scenario(scenario: &str) -> Result<(String, Option<PathBuf>)> {
    if scenario.contains('/') || scenario.ends_with(".toml") {
        let path = PathBuf::from(scenario);
        let file_name = path
            .file_name()
            .context("Invalid scenario file path")?
            .to_string_lossy()
            .to_string();
        // Contender will see it at /scenarios/<filename>
        return Ok((format!("/scenarios/{}", file_name), Some(path)));
    }
    Ok((scenario.to_string(), None))
}

/// Derive the contender container name from the deployer's network name.
fn container_name(deployer: &Deployer) -> String {
    let prefix = deployer
        .docker
        .net_name
        .strip_suffix("-network")
        .unwrap_or(&deployer.docker.net_name);
    format!("{}-contender", prefix)
}

/// Build the contender CLI command arguments.
fn build_contender_cmd(
    config: &SpamConfig,
    scenario_arg: &str,
    rpc_url: &str,
    private_key: &str,
) -> Vec<String> {
    // Contender CLI: `contender spam [OPTIONS] [TESTFILE] [COMMAND]`
    // Options must come before the scenario subcommand/testfile.
    let mut cmd = vec![
        "spam".to_string(),
        "-r".to_string(),
        rpc_url.to_string(),
        "-p".to_string(),
        private_key.to_string(),
        "--tps".to_string(),
        config.tps.to_string(),
        "-a".to_string(),
        config.accounts.to_string(),
        "--min-balance".to_string(),
        format!("{} ether", config.min_balance),
    ];

    if !config.forever {
        cmd.push("--duration".to_string());
        cmd.push(config.duration.to_string());
    } else {
        cmd.push("--forever".to_string());
    }

    if config.report {
        cmd.push("--report".to_string());
    }

    cmd.extend(config.extra_args.iter().cloned());

    // Scenario subcommand or testfile goes last
    cmd.push(scenario_arg.to_string());

    cmd
}

/// Pull a Docker image if not already available locally.
async fn pull_image(docker: &Docker, image: &str, tag: &str) -> Result<()> {
    use bollard::image::CreateImageOptions;

    let full_image = format!("{}:{}", image, tag);

    if docker.inspect_image(&full_image).await.is_ok() {
        tracing::debug!(image = %full_image, "Image already available locally");
        return Ok(());
    }

    let mut stream = docker.create_image(
        Some(CreateImageOptions {
            from_image: image.to_string(),
            tag: tag.to_string(),
            ..Default::default()
        }),
        None,
        None,
    );

    while let Some(result) = stream.next().await {
        let info = result.map_err(|e| anyhow::anyhow!("Failed to pull image '{}': {}", full_image, e))?;
        if let Some(status) = &info.status {
            tracing::trace!(status, "Image pull");
        }
    }

    Ok(())
}

/// Remove a stale container if it exists (ignore errors).
async fn remove_stale_container(docker: &Docker, container_name: &str) {
    let _ = docker
        .stop_container(
            container_name,
            Some(StopContainerOptions { t: 5 }),
        )
        .await;
    let _ = docker
        .remove_container(
            container_name,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;
}

/// Stream container logs to stdout until the container exits.
async fn stream_until_exit(docker: &Docker, container_name: &str) -> Result<()> {
    let log_options = LogsOptions::<String> {
        stdout: true,
        stderr: true,
        follow: true,
        ..Default::default()
    };

    let mut stream = docker.logs(container_name, Some(log_options));

    while let Some(result) = stream.next().await {
        match result {
            Ok(log) => print!("{}", log),
            Err(e) => {
                tracing::debug!(error = %e, "Log stream ended");
                break;
            }
        }
    }

    // Check exit code
    let inspect = docker
        .inspect_container(container_name, None)
        .await
        .context("Failed to inspect Contender container")?;

    let exit_code = inspect
        .state
        .and_then(|s| s.exit_code)
        .unwrap_or(-1);

    if exit_code != 0 {
        anyhow::bail!("Contender exited with code {}", exit_code);
    }

    tracing::info!("Contender completed successfully");
    Ok(())
}

/// Stream container logs until Ctrl+C, then stop the container.
async fn stream_until_ctrl_c(docker: &Docker, container_name: &str) -> Result<()> {
    let log_options = LogsOptions::<String> {
        stdout: true,
        stderr: true,
        follow: true,
        ..Default::default()
    };

    let mut stream = docker.logs(container_name, Some(log_options));

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            log_item = stream.next() => {
                match log_item {
                    Some(Ok(log)) => print!("{}", log),
                    Some(Err(e)) => {
                        tracing::debug!(error = %e, "Log stream ended");
                        break;
                    }
                    None => break,
                }
            }
            _ = &mut ctrl_c => {
                tracing::info!("Received Ctrl+C, stopping Contender...");
                break;
            }
        }
    }

    Ok(())
}

/// Stop and remove the contender container (best-effort cleanup).
async fn cleanup_container(docker: &Docker, container_name: &str) {
    tracing::debug!(container = %container_name, "Cleaning up Contender container");

    let _ = docker
        .stop_container(
            container_name,
            Some(StopContainerOptions { t: 10 }),
        )
        .await;

    let _ = docker
        .remove_container(
            container_name,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_scenario_builtin() {
        let (arg, file) = resolve_scenario("transfers").unwrap();
        assert_eq!(arg, "transfers");
        assert!(file.is_none());

        let (arg, file) = resolve_scenario("erc20").unwrap();
        assert_eq!(arg, "erc20");
        assert!(file.is_none());

        let (arg, file) = resolve_scenario("uni_v2").unwrap();
        assert_eq!(arg, "uni_v2");
        assert!(file.is_none());
    }

    #[test]
    fn test_resolve_scenario_custom_file() {
        let (arg, file) = resolve_scenario("./my-scenario.toml").unwrap();
        assert_eq!(arg, "/scenarios/my-scenario.toml");
        assert_eq!(file.unwrap(), PathBuf::from("./my-scenario.toml"));

        let (arg, file) = resolve_scenario("/home/user/scenarios/custom.toml").unwrap();
        assert_eq!(arg, "/scenarios/custom.toml");
        assert_eq!(
            file.unwrap(),
            PathBuf::from("/home/user/scenarios/custom.toml")
        );

        let (arg, file) = resolve_scenario("path/to/scenario.toml").unwrap();
        assert_eq!(arg, "/scenarios/scenario.toml");
        assert_eq!(file.unwrap(), PathBuf::from("path/to/scenario.toml"));
    }

    #[test]
    fn test_build_contender_cmd_basic() {
        let config = SpamConfig {
            scenario: "transfers".to_string(),
            tps: 10,
            duration: 30,
            forever: false,
            accounts: 5,
            min_balance: "0.1".to_string(),
            fund_amount: 100.0,
            funder_account_index: 10,
            report: false,
            contender_image: CONTENDER_DEFAULT_IMAGE.to_string(),
            contender_tag: CONTENDER_DEFAULT_TAG.to_string(),
            target_node: 0,
            extra_args: vec![],
        };

        let cmd = build_contender_cmd(&config, "transfers", "http://reth:9545/", "0xabc123");

        // Options come first, scenario subcommand goes last
        assert_eq!(cmd[0], "spam");
        assert_eq!(cmd.last().unwrap(), "transfers");

        // Verify options are present with correct values
        let r_idx = cmd.iter().position(|s| s == "-r").unwrap();
        assert_eq!(cmd[r_idx + 1], "http://reth:9545/");

        let p_idx = cmd.iter().position(|s| s == "-p").unwrap();
        assert_eq!(cmd[p_idx + 1], "0xabc123");

        let tps_idx = cmd.iter().position(|s| s == "--tps").unwrap();
        assert_eq!(cmd[tps_idx + 1], "10");

        let dur_idx = cmd.iter().position(|s| s == "--duration").unwrap();
        assert_eq!(cmd[dur_idx + 1], "30");

        let a_idx = cmd.iter().position(|s| s == "-a").unwrap();
        assert_eq!(cmd[a_idx + 1], "5");

        let mb_idx = cmd.iter().position(|s| s == "--min-balance").unwrap();
        assert_eq!(cmd[mb_idx + 1], "0.1 ether");

        assert!(!cmd.contains(&"--report".to_string()));
        assert!(!cmd.contains(&"--forever".to_string()));
    }

    #[test]
    fn test_build_contender_cmd_forever_mode() {
        let config = SpamConfig {
            scenario: "transfers".to_string(),
            tps: 100,
            duration: 30,
            forever: true,
            accounts: 10,
            min_balance: "0.1".to_string(),
            fund_amount: 100.0,
            funder_account_index: 10,
            report: false,
            contender_image: CONTENDER_DEFAULT_IMAGE.to_string(),
            contender_tag: CONTENDER_DEFAULT_TAG.to_string(),
            target_node: 0,
            extra_args: vec![],
        };

        let cmd = build_contender_cmd(&config, "transfers", "http://reth:9545/", "0xabc");

        // --duration should NOT be present in forever mode, --forever should be
        assert!(!cmd.contains(&"--duration".to_string()));
        assert!(cmd.contains(&"--forever".to_string()));
        // Scenario still goes last
        assert_eq!(cmd.last().unwrap(), "transfers");
    }

    #[test]
    fn test_build_contender_cmd_with_report_and_extra_args() {
        let config = SpamConfig {
            scenario: "transfers".to_string(),
            tps: 50,
            duration: 60,
            forever: false,
            accounts: 20,
            min_balance: "1.0".to_string(),
            fund_amount: 100.0,
            funder_account_index: 10,
            report: true,
            contender_image: CONTENDER_DEFAULT_IMAGE.to_string(),
            contender_tag: CONTENDER_DEFAULT_TAG.to_string(),
            target_node: 0,
            extra_args: vec!["--verbose".to_string(), "--seed".to_string(), "42".to_string()],
        };

        let cmd = build_contender_cmd(&config, "transfers", "http://reth:9545/", "0xkey");

        assert!(cmd.contains(&"--report".to_string()));
        assert!(cmd.contains(&"--verbose".to_string()));
        assert!(cmd.contains(&"--seed".to_string()));
        assert!(cmd.contains(&"42".to_string()));
        // Scenario goes after extra args at the end
        assert_eq!(cmd.last().unwrap(), "transfers");
    }

    #[test]
    fn test_load_funder_account() {
        let dir = tempdir::TempDir::new("spam-test").unwrap();
        let anvil_dir = dir.path().join("anvil");
        std::fs::create_dir_all(&anvil_dir).unwrap();

        let anvil_json = serde_json::json!({
            "available_accounts": [
                "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
                "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            ],
            "private_keys": [
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
                "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
            ]
        });

        std::fs::write(
            anvil_dir.join("anvil.json"),
            serde_json::to_string_pretty(&anvil_json).unwrap(),
        )
        .unwrap();

        let (addr, key) = load_funder_account(dir.path(), 0).unwrap();
        assert_eq!(addr, "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
        assert_eq!(
            key,
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
        );

        let (addr, key) = load_funder_account(dir.path(), 1).unwrap();
        assert_eq!(addr, "0x70997970C51812dc3A010C7d01b50e0d17dc79C8");
        assert_eq!(
            key,
            "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
        );
    }

    #[test]
    fn test_load_funder_account_out_of_range() {
        let dir = tempdir::TempDir::new("spam-test").unwrap();
        let anvil_dir = dir.path().join("anvil");
        std::fs::create_dir_all(&anvil_dir).unwrap();

        let anvil_json = serde_json::json!({
            "available_accounts": [
                "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
            ],
            "private_keys": [
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
            ]
        });

        std::fs::write(
            anvil_dir.join("anvil.json"),
            serde_json::to_string_pretty(&anvil_json).unwrap(),
        )
        .unwrap();

        let result = load_funder_account(dir.path(), 5);
        assert!(result.is_err());
    }

    #[test]
    fn test_container_name() {
        use crate::KupDockerConfig;

        // Create a minimal deployer for testing container_name derivation
        let deployer = Deployer {
            l1_chain_id: 1,
            l2_chain_id: 42069,
            outdata: PathBuf::from("/tmp/test"),
            anvil: Default::default(),
            op_deployer: Default::default(),
            docker: KupDockerConfig {
                net_name: "kup-test-network".to_string(),
                no_cleanup: false,
                publish_all_ports: false,
            },
            l2_stack: Default::default(),
            monitoring: Default::default(),
            dashboards_path: None,
            detach: false,
        };

        assert_eq!(container_name(&deployer), "kup-test-contender");
    }
}
