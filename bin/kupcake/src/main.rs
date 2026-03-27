//! kupcake is a CLI tool to help you bootstrap a rust-based op-stack chain in a few clicks.

mod cli;
mod completions;
mod config;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{CommandFactory, FromArgMatches};
use clap_complete::CompleteEnv;
use comfy_table::{Attribute, Cell, Table};

use cli::{
    BenchArgs, CleanupArgs, Cli, Commands, CompletionsArgs, DeployArgs, FaucetArgs, InspectArgs,
    L1Source, NodeAction, NodeArgs, PruneArgs, ShellArg, SnapshotArgs, SpamArgs,
};
use config::{apply_cli_overrides, deploy_config_to_builder, resolve_deploy_config};
use kupcake_deploy::{
    Deployer, DeployerBuilder, DeploymentResult, KupDocker, SpamPreset, cleanup_by_prefix,
};

#[tokio::main]
async fn main() -> Result<()> {
    // If the shell is requesting completions (COMPLETE env var set),
    // generate them and exit immediately.
    CompleteEnv::with_factory(Cli::command).complete();

    // Parse CLI with access to ArgMatches for value_source() introspection.
    let raw_matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&raw_matches)?;

    // Initialize the logger.
    tracing_subscriber::fmt()
        .with_max_level(cli.verbosity)
        .init();

    match cli.command {
        Some(Commands::Cleanup(args)) => run_cleanup(args).await,
        Some(Commands::Deploy(args)) => {
            // Extract the deploy subcommand's ArgMatches for figment integration
            let deploy_matches = raw_matches
                .subcommand_matches("deploy")
                .cloned()
                .unwrap_or_default();
            run_deploy(args, &deploy_matches).await
        }
        Some(Commands::Faucet(args)) => run_faucet(args).await,
        Some(Commands::Inspect(args)) => run_inspect(args).await,
        Some(Commands::Spam(args)) => run_spam_cmd(args).await,
        Some(Commands::Bench(args)) => run_bench(args).await,
        Some(Commands::Node(args)) => run_node(args).await,
        Some(Commands::List) => run_list().await,
        Some(Commands::Prune(args)) => run_prune(args).await,
        Some(Commands::Snapshot(args)) => run_snapshot(args).await,
        Some(Commands::Completions(args)) => run_completions(args),
        // Default to deploy with default args when no subcommand is provided
        None => run_deploy(DeployArgs::default(), &clap::ArgMatches::default()).await,
    }
}

async fn run_cleanup(args: CleanupArgs) -> Result<()> {
    tracing::info!("Cleaning up network with prefix: {}", args.prefix);

    let result = cleanup_by_prefix(&args.prefix).await?;

    if result.containers_removed.is_empty() && result.network_removed.is_none() {
        tracing::info!("Nothing to clean up");
    } else {
        if !result.containers_removed.is_empty() {
            tracing::info!("Removed {} container(s):", result.containers_removed.len());
            for name in &result.containers_removed {
                tracing::info!("  - {}", name);
            }
        }
        if let Some(network) = &result.network_removed {
            tracing::info!("Removed network: {}", network);
        }
        tracing::info!("Cleanup completed successfully");
    }

    Ok(())
}

async fn run_snapshot(args: SnapshotArgs) -> Result<()> {
    let config_path = resolve_config_path(&args.config);
    let deployer = Deployer::load_from_file(&config_path)?;

    let l2_stack_path = deployer.outdata.join("l2-stack");

    // Validate rollup.json exists
    let rollup_path = l2_stack_path.join("rollup.json");
    if !rollup_path.exists() {
        anyhow::bail!(
            "rollup.json not found at {}. Is this a valid deployment?",
            rollup_path.display()
        );
    }

    // Find primary sequencer's reth-data directory
    let sequencer_name = &deployer.l2_stack.sequencers[0].op_reth.container_name;
    let reth_data_path = l2_stack_path.join(format!("reth-data-{}", sequencer_name));
    if !reth_data_path.exists() {
        anyhow::bail!(
            "Reth data directory not found at {}",
            reth_data_path.display()
        );
    }

    // Resolve the reth-data path (follow symlinks to get real path)
    let reth_data_path = reth_data_path
        .canonicalize()
        .context("Failed to resolve reth data path")?;

    // Determine output path
    let network_name = deployer
        .docker
        .net_name
        .strip_suffix("-network")
        .unwrap_or(&deployer.docker.net_name);
    let output_path = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("{}-snapshot.tar.gz", network_name)));

    tracing::info!(
        output = %output_path.display(),
        "Creating snapshot archive"
    );

    // Build tar.gz archive
    let file = std::fs::File::create(&output_path)
        .with_context(|| format!("Failed to create {}", output_path.display()))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    // Add rollup.json
    archive
        .append_path_with_name(&rollup_path, "rollup.json")
        .context("Failed to add rollup.json to archive")?;

    // Add genesis.json if present
    let genesis_path = l2_stack_path.join("genesis.json");
    if genesis_path.exists() {
        archive
            .append_path_with_name(&genesis_path, "genesis.json")
            .context("Failed to add genesis.json to archive")?;
    }

    // Add intent.toml if present
    let intent_path = l2_stack_path.join("intent.toml");
    if intent_path.exists() {
        archive
            .append_path_with_name(&intent_path, "intent.toml")
            .context("Failed to add intent.toml to archive")?;
    }

    // Add Anvil state if present (needed for L1 history on restore)
    let anvil_state_path = deployer.outdata.join("anvil/state.json");
    if anvil_state_path.exists() {
        archive
            .append_path_with_name(&anvil_state_path, "anvil-state.json")
            .context("Failed to add anvil state to archive")?;
    }

    // Add reth-data directory
    let reth_dir_name = reth_data_path
        .file_name()
        .context("Invalid reth data directory name")?;
    archive
        .append_dir_all(reth_dir_name, &reth_data_path)
        .context("Failed to add reth data directory to archive")?;

    // Finalize the archive
    let encoder = archive.into_inner().context("Failed to finalize archive")?;
    encoder.finish().context("Failed to finish gzip encoding")?;

    let file_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);
    tracing::info!(
        path = %output_path.display(),
        size_mb = file_size / (1024 * 1024),
        "Snapshot created"
    );

    Ok(())
}

fn run_completions(args: CompletionsArgs) -> Result<()> {
    let bin_name = "kupcake";
    let snippet = match args.shell {
        ShellArg::Bash => format!(r#"eval "$(COMPLETE=bash {bin_name})""#),
        ShellArg::Zsh => format!(r#"eval "$(COMPLETE=zsh {bin_name})""#),
        ShellArg::Fish => format!("COMPLETE=fish {bin_name} | source"),
    };
    println!("{snippet}");
    Ok(())
}

/// Resolve a config argument to a path.
///
/// If it looks like a path (contains `/` or `.`), use it directly.
/// Otherwise treat it as a network name and resolve to `./data-{name}/Kupcake.toml`.
fn resolve_config_path(config: &str) -> PathBuf {
    if config.contains('/') || config.contains('.') {
        PathBuf::from(config)
    } else {
        PathBuf::from(format!("data-{}", config))
    }
}

async fn run_inspect(args: InspectArgs) -> Result<()> {
    let config_path = resolve_config_path(&args.config);
    let deployer = Deployer::load_from_file(&config_path)?;

    let report =
        kupcake_deploy::inspect::inspect_network(&deployer, args.verbose, args.service.as_deref())
            .await?;

    if args.json {
        let json =
            serde_json::to_string_pretty(&report).context("Failed to serialize inspect report")?;
        println!("{json}");
    } else {
        print!("{report}");
    }

    Ok(())
}

async fn run_node(args: NodeArgs) -> Result<()> {
    let config_path = resolve_config_path(&args.config);
    let mut deployer = Deployer::load_from_file(&config_path)?;

    // Create Docker client with no_cleanup (we're managing individual containers)
    let mut docker = KupDocker::new(kupcake_deploy::KupDockerConfig {
        no_cleanup: true,
        ..deployer.docker.clone()
    })
    .await?;

    match args.action {
        NodeAction::Add => {
            tracing::info!(config = %config_path.display(), "Adding new validator node...");
            let handler =
                kupcake_deploy::node_lifecycle::add_validator(&mut deployer, &mut docker).await?;

            tracing::info!("New validator added successfully:");
            if let Some(ref url) = handler.op_reth.http_host_url {
                tracing::info!("  op-reth HTTP: {}", url);
            }
            if let Some(ref url) = handler.kona_node.rpc_host_url {
                tracing::info!("  kona-node RPC: {}", url);
            }
        }
        NodeAction::Remove {
            identifier,
            cleanup_data,
        } => {
            tracing::info!(
                config = %config_path.display(),
                node = %identifier,
                "Removing node..."
            );
            kupcake_deploy::node_lifecycle::remove_node(
                &mut deployer,
                &docker,
                &identifier,
                cleanup_data,
            )
            .await?;
            tracing::info!("Node '{}' removed successfully", identifier);
        }
        NodeAction::Pause { identifier } => {
            kupcake_deploy::node_lifecycle::pause_node(&deployer, &docker, &identifier).await?;
        }
        NodeAction::Unpause { identifier } => {
            kupcake_deploy::node_lifecycle::unpause_node(&deployer, &docker, &identifier).await?;
        }
        NodeAction::Restart { identifier } => {
            kupcake_deploy::node_lifecycle::restart_node(&deployer, &docker, &identifier).await?;
        }
    }

    Ok(())
}

async fn run_faucet(args: FaucetArgs) -> Result<()> {
    let config_path = resolve_config_path(&args.config);

    let deployer = Deployer::load_from_file(&config_path)?;

    tracing::info!(
        config = %config_path.display(),
        to = %args.to,
        amount = args.amount,
        wait = args.wait,
        "Running faucet deposit..."
    );

    let docker = KupDocker::new(deployer.docker.clone()).await?;
    let result = kupcake_deploy::faucet::faucet_deposit(
        &docker,
        &deployer,
        &args.to,
        args.amount,
        args.wait,
    )
    .await?;

    tracing::info!(tx_hash = %result.l1_tx_hash, "Deposit sent on L1");
    if let Some(balance) = result.l2_balance {
        tracing::info!(l2_balance = %balance, "Deposit confirmed on L2");
    }

    Ok(())
}

async fn run_spam_cmd(args: SpamArgs) -> Result<()> {
    let config_path = resolve_config_path(&args.config);
    let deployer = Deployer::load_from_file(&config_path)?;

    let spam_config = args.into_config(&deployer)?;

    tracing::info!(
        config = %config_path.display(),
        scenario = %spam_config.scenario,
        tps = spam_config.tps,
        "Running spam..."
    );

    let mut docker = KupDocker::new(deployer.docker.clone()).await?;
    kupcake_deploy::spam::run_spam(&mut docker, &deployer, &spam_config).await?;
    Ok(())
}

async fn run_bench(args: BenchArgs) -> Result<()> {
    use kupcake_deploy::bench::{BenchConfig, run_bench as bench_run, to_toml};

    let images = args.docker_images.clone();
    let config = BenchConfig {
        iterations: args.iterations,
        warmup: args.warmup,
        label: args.label,
        deployment_target: args.deployment_target.into(),
        l2_node_count: args.l2_nodes,
        sequencer_count: args.sequencer_count,
        block_time: args.block_time,
        flashblocks: args.flashblocks,
        no_proposer: args.no_proposer,
        no_challenger: args.no_challenger,
        deployer_customizer: Some(Box::new(move |builder| {
            apply_image_overrides(builder, &images)
        })),
    };

    tracing::info!(
        iterations = config.iterations,
        warmup = config.warmup,
        deployment_target = %config.deployment_target,
        "Starting benchmark..."
    );

    let result = bench_run(config).await?;
    let toml_output = to_toml(&result)?;

    if let Some(path) = args.output {
        std::fs::write(&path, &toml_output)
            .with_context(|| format!("Failed to write bench results to {path}"))?;
        tracing::info!(path = %path, "Benchmark results written to file");
    } else {
        tracing::info!("Benchmark results:\n{toml_output}");
    }

    Ok(())
}

/// Apply Docker image overrides from CLI args to a DeployerBuilder.
fn apply_image_overrides(
    builder: DeployerBuilder,
    images: &cli::DockerImageOverrides,
) -> DeployerBuilder {
    builder
        .anvil_image(images.anvil_image.clone())
        .anvil_tag(images.anvil_tag.clone())
        .op_reth_image(images.op_reth_image.clone())
        .op_reth_tag(images.op_reth_tag.clone())
        .kona_node_image(images.kona_node_image.clone())
        .kona_node_tag(images.kona_node_tag.clone())
        .op_batcher_image(images.op_batcher_image.clone())
        .op_batcher_tag(images.op_batcher_tag.clone())
        .op_proposer_image(images.op_proposer_image.clone())
        .op_proposer_tag(images.op_proposer_tag.clone())
        .op_challenger_image(images.op_challenger_image.clone())
        .op_challenger_tag(images.op_challenger_tag.clone())
        .op_conductor_image(images.op_conductor_image.clone())
        .op_conductor_tag(images.op_conductor_tag.clone())
        .op_deployer_image(images.op_deployer_image.clone())
        .op_deployer_tag(images.op_deployer_tag.clone())
        .op_rbuilder_image(images.op_rbuilder_image.clone())
        .op_rbuilder_tag(images.op_rbuilder_tag.clone())
}

/// Resolve the config path for a deploy command.
///
/// Priority:
/// 1. Explicit `--config` flag → use that path directly
/// 2. `--network` name with an existing `data-{name}/Kupcake.toml` → auto-load it
/// 3. Neither → returns `None` (new deployment)
fn resolve_existing_config(config: Option<&str>, network: Option<&str>) -> Option<PathBuf> {
    if let Some(config) = config {
        return Some(PathBuf::from(config));
    }

    // When --network is given without --config, check if a saved config exists
    network.and_then(|name| {
        let candidate = resolve_config_path(name);
        let config_file = if candidate.is_dir() {
            candidate.join("Kupcake.toml")
        } else {
            candidate
        };
        if config_file.exists() {
            tracing::info!(
                network = %name,
                config = %config_file.display(),
                "Found existing deployment, loading saved configuration"
            );
            Some(config_file)
        } else {
            None
        }
    })
}

async fn run_deploy(args: DeployArgs, deploy_matches: &clap::ArgMatches) -> Result<()> {
    // Parse spam preset early so we fail fast on invalid names
    let spam_preset = args
        .spam
        .as_deref()
        .map(|s| {
            s.parse::<SpamPreset>().map_err(|_| {
                anyhow::anyhow!(
                    "Unknown spam preset '{}'. Available presets: {}",
                    s,
                    SpamPreset::all()
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
        })
        .transpose()?;

    // Resolve layered config: defaults → env vars → CLI args
    let mut deploy_config = resolve_deploy_config(&args, deploy_matches)?;
    deploy_config.resolve_long_running();

    let metrics_file = args.metrics_file.as_ref().map(PathBuf::from);
    let ports_file = args.ports_file.as_ref().map(PathBuf::from);

    // If a config file is provided, or a --network name matches an existing deployment, load it
    let resolved_config_path =
        resolve_existing_config(args.config.as_deref(), args.network.as_deref());

    if let Some(config_path) = resolved_config_path {
        let mut deployer = Deployer::load_from_file(&config_path)?;

        // Apply CLI overrides to the loaded config (new: CLI args no longer silently ignored)
        apply_cli_overrides(&mut deployer, &deploy_config);

        tracing::info!(
            config_path = %config_path.display(),
            outdata_path = %deployer.outdata.display(),
            l1_chain_id = deployer.l1_chain_id,
            l2_chain_id = deployer.l2_chain_id,
            "Loading deployment from config file..."
        );

        if let Some(preset) = spam_preset {
            let user_no_cleanup = deployer.docker.no_cleanup;
            deployer.docker.no_cleanup = true;
            let mut docker = KupDocker::new(deployer.docker.clone()).await?;
            let result = deployer.deploy(&mut docker, args.redeploy, false).await?;
            write_output_files(&result, &metrics_file, &ports_file)?;

            return run_spam_after_deploy(&config_path, preset, user_no_cleanup).await;
        }

        let mut docker = KupDocker::new(deployer.docker.clone()).await?;
        let result = deployer.deploy(&mut docker, args.redeploy, true).await?;
        write_output_files(&result, &metrics_file, &ports_file)?;
        return Ok(());
    }

    // Validate: --snapshot requires --l1 (fork mode)
    if deploy_config.snapshot.is_some() && deploy_config.l1.is_none() {
        anyhow::bail!(
            "--snapshot requires --l1 to be set (fork mode is required to restore from a snapshot)"
        );
    }

    // Determine L1 chain ID and RPC URL
    let l1_source = args.l1;
    let (l1_chain_id, l1_rpc_url) = resolve_l1_config(l1_source).await?;

    // Force no_cleanup when spam or detach mode
    if spam_preset.is_some() {
        deploy_config.no_cleanup = Some(true);
    }

    // Build the deployer via figment-resolved config
    let deployer = deploy_config_to_builder(&deploy_config, l1_chain_id, l1_rpc_url)
        .dashboards_path(PathBuf::from("grafana/dashboards"))
        .build()
        .await?;

    // Save the configuration to kupconf.toml before deploying
    let config_path = deployer.save_config()?;

    if let Some(preset) = spam_preset {
        let mut docker = KupDocker::new(deployer.docker.clone()).await?;
        let result = deployer.deploy(&mut docker, args.redeploy, false).await?;
        write_output_files(&result, &metrics_file, &ports_file)?;
        return run_spam_after_deploy(
            &config_path,
            preset,
            deploy_config.no_cleanup.unwrap_or(false),
        )
        .await;
    }

    let mut docker = KupDocker::new(deployer.docker.clone()).await?;
    let result = deployer.deploy(&mut docker, args.redeploy, true).await?;
    write_output_files(&result, &metrics_file, &ports_file)?;

    Ok(())
}

async fn run_list() -> Result<()> {
    let registry = kupcake_deploy::DevnetRegistry::new()?;
    let entries = registry.list()?;

    if entries.is_empty() {
        println!("No tracked devnets.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec![
        Cell::new("NAME").add_attribute(Attribute::Bold),
        Cell::new("STATE").add_attribute(Attribute::Bold),
        Cell::new("DATADIR").add_attribute(Attribute::Bold),
        Cell::new("CREATED").add_attribute(Attribute::Bold),
    ]);
    for entry in &entries {
        let state_cell = match entry.state {
            kupcake_deploy::DevnetState::Running => {
                Cell::new("Running").fg(comfy_table::Color::Green)
            }
            kupcake_deploy::DevnetState::Stopped => {
                Cell::new("Stopped").fg(comfy_table::Color::Red)
            }
        };
        table.add_row(vec![
            Cell::new(&entry.name),
            state_cell,
            Cell::new(entry.datadir.display().to_string()),
            Cell::new(&entry.created_at),
        ]);
    }
    println!("{table}");

    Ok(())
}

async fn run_prune(args: PruneArgs) -> Result<()> {
    let registry = kupcake_deploy::DevnetRegistry::new()?;
    let entries = registry.list()?;
    let stopped: Vec<_> = entries
        .iter()
        .filter(|e| e.state == kupcake_deploy::DevnetState::Stopped)
        .collect();

    if stopped.is_empty() {
        println!("No stopped devnets to prune.");
        return Ok(());
    }

    println!("The following stopped devnets will be removed:");
    for entry in &stopped {
        println!("  - {} ({})", entry.name, entry.datadir.display());
    }

    if !args.yes {
        use std::io::Write;
        print!("\nProceed? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let removed = registry.prune()?;
    println!("Pruned {} devnet(s):", removed.len());
    for entry in &removed {
        println!("  - {} (removed {})", entry.name, entry.datadir.display());
    }

    Ok(())
}

/// Write metrics and/or ports files if their paths are set.
fn write_output_files(
    result: &DeploymentResult,
    metrics_file: &Option<PathBuf>,
    ports_file: &Option<PathBuf>,
) -> Result<()> {
    if let Some(path) = metrics_file {
        result.metrics.write_to_file(path)?;
    }
    if let Some(path) = ports_file {
        result.endpoints().write_to_file(path)?;
    }
    Ok(())
}

/// Run spam after a successful deployment, then clean up if needed.
///
/// Reloads the deployer from the saved config, creates a SpamConfig from the preset,
/// runs spam until Ctrl+C, then optionally cleans up containers.
async fn run_spam_after_deploy(
    config_path: &std::path::Path,
    preset: SpamPreset,
    user_no_cleanup: bool,
) -> Result<()> {
    // Reload from disk since deploy() consumes the Deployer.
    // This also picks up any state written during deployment.
    let deployer = Deployer::load_from_file(config_path)?;

    // Resolve RPC URL from the primary sequencer
    let rpc_url = deployer.l2_stack.sequencers[0].op_reth.docker_rpc_url();

    let spam_config = preset.to_config(&rpc_url);

    tracing::info!(
        preset = %preset,
        scenario = %spam_config.scenario,
        tps = spam_config.tps,
        "Starting spam after deployment..."
    );

    let mut docker = KupDocker::new(deployer.docker.clone()).await?;
    let spam_result = kupcake_deploy::spam::run_spam(&mut docker, &deployer, &spam_config).await;

    // Clean up deployment containers if the user didn't explicitly set --no-cleanup
    if !user_no_cleanup {
        let prefix = deployer
            .docker
            .net_name
            .strip_suffix("-network")
            .unwrap_or(&deployer.docker.net_name);

        tracing::info!(prefix = %prefix, "Cleaning up deployment containers...");
        let _ = cleanup_by_prefix(prefix).await;
    }

    spam_result
}

/// Resolve L1 chain ID and RPC URL from CLI arguments.
///
/// Returns `(l1_chain_id, l1_rpc_url)` where `l1_rpc_url` is `None` for local mode.
async fn resolve_l1_config(l1_source: Option<L1Source>) -> Result<(u64, Option<String>)> {
    use rand::Rng;

    let Some(source) = l1_source else {
        // Local mode: no forking, random L1 chain ID
        let chain_id = rand::rng().random_range(10000..=99999);
        tracing::info!(
            l1_chain_id = chain_id,
            "Running in local mode without L1 forking"
        );
        return Ok((chain_id, None));
    };

    let rpc_url = source.rpc_url();

    // Detect chain ID via eth_chainId
    tracing::info!(rpc_url = %rpc_url, "Detecting L1 chain ID from RPC...");
    let chain_id = fetch_chain_id(&rpc_url).await?;
    tracing::info!(l1_chain_id = chain_id, rpc_url = %rpc_url, "Detected L1 chain ID");

    Ok((chain_id, Some(rpc_url)))
}

/// Fetch the chain ID from an Ethereum RPC endpoint using eth_chainId.
async fn fetch_chain_id(rpc_url: &str) -> Result<u64> {
    use anyhow::Context;
    use serde_json::{Value, json};

    let client = reqwest::Client::new();
    let response = client
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_chainId",
            "params": [],
            "id": 1
        }))
        .send()
        .await
        .context("Failed to send eth_chainId request")?;

    let body: Value = response
        .json()
        .await
        .context("Failed to parse eth_chainId response")?;

    let chain_id_hex = body["result"]
        .as_str()
        .context("eth_chainId response missing 'result' field")?;

    // Parse hex string (with or without 0x prefix) to u64
    let chain_id = u64::from_str_radix(chain_id_hex.trim_start_matches("0x"), 16)
        .context("Failed to parse chain ID from hex")?;

    Ok(chain_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_resolve_deploy_config_explicit_config() {
        // --config always wins, even if the file doesn't exist
        let result = resolve_existing_config(Some("./my-config.toml"), None);
        assert_eq!(result, Some(PathBuf::from("./my-config.toml")));
    }

    #[test]
    fn test_resolve_deploy_config_explicit_config_over_network() {
        // --config takes priority over --network
        let result = resolve_existing_config(Some("./my-config.toml"), Some("my-network"));
        assert_eq!(result, Some(PathBuf::from("./my-config.toml")));
    }

    #[test]
    fn test_resolve_deploy_config_network_no_existing_dir() {
        // --network with no existing data directory returns None (new deployment)
        let result = resolve_existing_config(None, Some("nonexistent-network-12345"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_deploy_config_network_with_existing_config() {
        // --network with an existing data-{name}/Kupcake.toml auto-loads it
        let network_name = format!("test-kupcake-{}", std::process::id());

        // Create a data-{name} directory with a Kupcake.toml in the current dir
        let data_dir = PathBuf::from(format!("data-{}", network_name));
        fs::create_dir_all(&data_dir).unwrap();
        let config_file = data_dir.join("Kupcake.toml");
        fs::write(&config_file, "# placeholder").unwrap();

        let result = resolve_existing_config(None, Some(&network_name));
        assert_eq!(result, Some(config_file));

        // Cleanup
        fs::remove_dir_all(&data_dir).unwrap();
    }

    #[test]
    fn test_resolve_deploy_config_none_when_no_args() {
        // No --config and no --network returns None
        let result = resolve_existing_config(None, None);
        assert_eq!(result, None);
    }
}
