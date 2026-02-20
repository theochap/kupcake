//! Spam module for generating continuous L2 traffic using Flashbots Contender.
//!
//! Runs a Contender Docker container against a deployed kupcake L2 network,
//! automatically funding the spammer account via the L1→L2 faucet deposit.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::{
    Deployer, DockerImage, KupDocker, KupDockerConfig, ServiceConfig,
    docker::CreateAndStartContainerOptions, faucet,
};

/// Default Docker image for Contender.
pub const CONTENDER_DEFAULT_IMAGE: &str = "flashbots/contender";
/// Default Docker tag for Contender.
pub const CONTENDER_DEFAULT_TAG: &str = "latest";

/// Named spam presets for quick workload selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum SpamPreset {
    Light,
    Medium,
    Heavy,
    Erc20,
    Uniswap,
    Stress,
}

impl SpamPreset {
    /// Return a list of all available presets.
    pub const fn all() -> &'static [SpamPreset] {
        &[
            SpamPreset::Light,
            SpamPreset::Medium,
            SpamPreset::Heavy,
            SpamPreset::Erc20,
            SpamPreset::Uniswap,
            SpamPreset::Stress,
        ]
    }

    /// Convert this preset into a fully populated `SpamConfig`.
    ///
    /// The `rpc_url` should be the Docker-internal RPC URL of the target sequencer
    /// (e.g. `http://container-name:port/`).
    pub fn to_config(self, rpc_url: &str) -> SpamConfig {
        let (scenario, tps, accounts) = match self {
            SpamPreset::Light => ("transfers", 5, 3),
            SpamPreset::Medium => ("transfers", 10, 5),
            SpamPreset::Heavy => ("transfers", 50, 20),
            SpamPreset::Erc20 => ("erc20", 10, 5),
            SpamPreset::Uniswap => ("uni_v2", 5, 5),
            SpamPreset::Stress => ("transfers", 100, 50),
        };

        SpamConfig {
            scenario: scenario.to_string(),
            tps,
            duration: 0,
            forever: true,
            accounts,
            min_balance: "0.1".to_string(),
            fund_amount: 100.0,
            funder_account_index: 10,
            report: false,
            contender_image: CONTENDER_DEFAULT_IMAGE.to_string(),
            contender_tag: CONTENDER_DEFAULT_TAG.to_string(),
            rpc_url: rpc_url.to_string(),
            extra_args: vec![],
        }
    }
}

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
    /// Docker-internal RPC URL of the target sequencer node.
    pub rpc_url: String,
    /// Extra arguments passed directly to contender.
    pub extra_args: Vec<String>,
}

/// Run the Contender spammer against a deployed L2 network.
///
/// The caller is responsible for resolving the target node's RPC URL and
/// setting it on `config.rpc_url` before calling this function.
pub async fn run_spam(deployer: &Deployer, config: &SpamConfig) -> Result<()> {
    if config.rpc_url.is_empty() {
        anyhow::bail!("rpc_url must be set on SpamConfig before calling run_spam");
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

    // Create KupDocker instance — this resolves the network by ID so the
    // Contender container joins the same network as the other services.
    let mut kup_docker = KupDocker::new(KupDockerConfig {
        ..deployer.docker.clone()
    })
    .await
    .context("Failed to initialize Docker client")?;

    tracing::info!(rpc_url = %config.rpc_url, "Targeting sequencer RPC");

    // Create contender data directory for DB persistence
    let contender_data_dir = deployer.outdata.join("contender");
    std::fs::create_dir_all(&contender_data_dir)
        .with_context(|| format!("Failed to create {}", contender_data_dir.display()))?;

    let container_name = container_name(deployer);

    // Build contender command
    let cmd = build_contender_cmd(config, &scenario_arg, &funder_private_key);

    // Build service config with volume mounts
    let contender_data_abs = contender_data_dir
        .canonicalize()
        .unwrap_or(contender_data_dir.clone());

    let mut service_config = ServiceConfig::new(DockerImage::new(
        &config.contender_image,
        &config.contender_tag,
    ))
    .cmd(cmd)
    .bind_str(format!(
        "{}:/root/.contender/:rw",
        contender_data_abs.display()
    ));

    // If custom scenario file, mount it read-only
    if let Some(ref file_path) = scenario_file {
        let abs_path = file_path
            .canonicalize()
            .with_context(|| format!("Scenario file not found: {}", file_path.display()))?;
        service_config = service_config.bind_str(format!(
            "{}:/scenarios/{}:ro",
            abs_path.display(),
            file_path.file_name().unwrap_or_default().to_string_lossy()
        ));
    }

    tracing::info!(container = %container_name, "Starting Contender container...");

    kup_docker
        .start_service(
            &container_name,
            service_config,
            CreateAndStartContainerOptions::default(),
        )
        .await
        .context("Failed to start Contender container")?;

    tracing::info!("Contender is running, streaming logs...");

    // Stream logs until container exits or Ctrl+C
    kup_docker.stream_logs(&container_name).await
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
fn build_contender_cmd(config: &SpamConfig, scenario_arg: &str, private_key: &str) -> Vec<String> {
    // Contender CLI: `contender spam [OPTIONS] [TESTFILE] [COMMAND]`
    // Options must come before the scenario subcommand/testfile.
    let mut cmd = vec![
        "spam".to_string(),
        "-r".to_string(),
        config.rpc_url.clone(),
        "-t".to_string(),
        "eip1559".to_string(),
        "-p".to_string(),
        private_key.to_string(),
        "--tps".to_string(),
        config.tps.to_string(),
        "-a".to_string(),
        config.accounts.to_string(),
        // We need to set the op flag
        "--op".to_string(),
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
            rpc_url: "http://test-reth:9545/".to_string(),
            extra_args: vec![],
        };

        let cmd = build_contender_cmd(&config, "transfers", "0xabc123");

        // Options come first, scenario subcommand goes last
        assert_eq!(cmd[0], "spam");
        assert_eq!(cmd.last().unwrap(), "transfers");

        // Verify options are present with correct values
        let r_idx = cmd.iter().position(|s| s == "-r").unwrap();
        assert_eq!(cmd[r_idx + 1], "http://test-reth:9545/");

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
            rpc_url: "http://test-reth:9545/".to_string(),
            extra_args: vec![],
        };

        let cmd = build_contender_cmd(&config, "transfers", "0xabc");

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
            rpc_url: "http://test-reth:9545/".to_string(),
            extra_args: vec![
                "--verbose".to_string(),
                "--seed".to_string(),
                "42".to_string(),
            ],
        };

        let cmd = build_contender_cmd(&config, "transfers", "0xkey");

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
    fn test_spam_preset_from_str() {
        use std::str::FromStr;

        assert_eq!(SpamPreset::from_str("light").unwrap(), SpamPreset::Light);
        assert_eq!(SpamPreset::from_str("medium").unwrap(), SpamPreset::Medium);
        assert_eq!(SpamPreset::from_str("heavy").unwrap(), SpamPreset::Heavy);
        assert_eq!(SpamPreset::from_str("erc20").unwrap(), SpamPreset::Erc20);
        assert_eq!(
            SpamPreset::from_str("uniswap").unwrap(),
            SpamPreset::Uniswap
        );
        assert_eq!(SpamPreset::from_str("stress").unwrap(), SpamPreset::Stress);

        // Invalid preset
        assert!(SpamPreset::from_str("invalid").is_err());
        assert!(SpamPreset::from_str("LIGHT").is_err());
    }

    #[test]
    fn test_spam_preset_to_config() {
        let config = SpamPreset::Light.to_config("http://test:8545/");
        assert_eq!(config.scenario, "transfers");
        assert_eq!(config.tps, 5);
        assert_eq!(config.accounts, 3);
        assert!(config.forever);

        let config = SpamPreset::Uniswap.to_config("http://test:8545/");
        assert_eq!(config.scenario, "uni_v2");
        assert_eq!(config.tps, 5);
        assert_eq!(config.accounts, 5);
        assert!(config.forever);

        let config = SpamPreset::Stress.to_config("http://test:8545/");
        assert_eq!(config.scenario, "transfers");
        assert_eq!(config.tps, 100);
        assert_eq!(config.accounts, 50);
        assert!(config.forever);
    }

    #[test]
    fn test_spam_preset_all() {
        let all = SpamPreset::all();
        assert_eq!(all.len(), 6);
        assert_eq!(all[0], SpamPreset::Light);
        assert_eq!(all[5], SpamPreset::Stress);
    }

    #[test]
    fn test_all_presets_produce_valid_configs() {
        for preset in SpamPreset::all() {
            let config = preset.to_config("http://test:8545/");
            assert!(!config.scenario.is_empty(), "{} has empty scenario", preset);
            assert!(config.tps > 0, "{} has zero tps", preset);
            assert!(config.accounts > 0, "{} has zero accounts", preset);
            assert!(config.forever, "{} should run forever", preset);
            assert!(config.fund_amount > 0.0, "{} has zero fund_amount", preset);
            assert_eq!(
                config.funder_account_index, 10,
                "{} should use funder index 10",
                preset
            );
            assert_eq!(
                config.contender_image, CONTENDER_DEFAULT_IMAGE,
                "{} should use default image",
                preset
            );
            assert_eq!(
                config.contender_tag, CONTENDER_DEFAULT_TAG,
                "{} should use default tag",
                preset
            );
            assert!(
                config.extra_args.is_empty(),
                "{} should have no extra args",
                preset
            );
        }
    }

    #[test]
    fn test_preset_display_matches_from_str() {
        use std::str::FromStr;
        for preset in SpamPreset::all() {
            let display = preset.to_string();
            let parsed = SpamPreset::from_str(&display).unwrap_or_else(|_| {
                panic!(
                    "Display '{}' for {:?} should round-trip through FromStr",
                    display, preset
                )
            });
            assert_eq!(*preset, parsed);
        }
    }

    #[test]
    fn test_preset_configs_use_known_scenarios() {
        let known_scenarios = ["transfers", "erc20", "uni_v2"];
        for preset in SpamPreset::all() {
            let config = preset.to_config("http://test:8545/");
            assert!(
                known_scenarios.contains(&config.scenario.as_str()),
                "{} uses unknown scenario '{}'",
                preset,
                config.scenario
            );
        }
    }

    #[test]
    fn test_preset_tps_ordering() {
        // Verify the presets have sensible TPS ordering for transfer-based ones
        let light = SpamPreset::Light.to_config("http://test:8545/");
        let medium = SpamPreset::Medium.to_config("http://test:8545/");
        let heavy = SpamPreset::Heavy.to_config("http://test:8545/");
        let stress = SpamPreset::Stress.to_config("http://test:8545/");

        assert!(light.tps < medium.tps, "light < medium TPS");
        assert!(medium.tps < heavy.tps, "medium < heavy TPS");
        assert!(heavy.tps < stress.tps, "heavy < stress TPS");
    }

    #[test]
    fn test_preset_to_config_generates_valid_contender_cmd() {
        // Verify that every preset generates a valid contender command
        for preset in SpamPreset::all() {
            let config = preset.to_config("http://test-reth:9545/");
            let (scenario_arg, _) = resolve_scenario(&config.scenario).unwrap();
            let cmd = build_contender_cmd(&config, &scenario_arg, "0xdeadbeef");

            assert_eq!(cmd[0], "spam", "{}: first arg should be 'spam'", preset);
            assert_eq!(
                cmd.last().unwrap(),
                &scenario_arg,
                "{}: last arg should be scenario",
                preset
            );
            assert!(
                cmd.contains(&"--forever".to_string()),
                "{}: should contain --forever",
                preset
            );
            assert!(
                !cmd.contains(&"--duration".to_string()),
                "{}: should NOT contain --duration when forever",
                preset
            );
            assert!(
                cmd.contains(&config.tps.to_string()),
                "{}: should contain tps value",
                preset
            );
        }
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
            snapshot: None,
            copy_snapshot: false,
        };

        assert_eq!(container_name(&deployer), "kup-test-contender");
    }
}
