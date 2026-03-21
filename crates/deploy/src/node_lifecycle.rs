//! Node lifecycle management for adding, removing, pausing, and restarting L2 nodes
//! on a running network.

use anyhow::{Context, Result};

use crate::{
    Deployer, KupDocker,
    service::KupcakeService,
    services::l2_node::{ConductorContext, L2NodeBuilder, L2NodeHandler, L2NodeInput},
};

/// Location of a node within the deployer config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeLocation {
    /// A sequencer node at the given index (0-based).
    Sequencer(usize),
    /// A validator node at the given index (0-based).
    Validator(usize),
}

impl std::fmt::Display for NodeLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeLocation::Sequencer(0) => write!(f, "sequencer"),
            NodeLocation::Sequencer(i) => write!(f, "sequencer-{}", i),
            NodeLocation::Validator(i) => write!(f, "validator-{}", i + 1),
        }
    }
}

/// Resolve a human-friendly node identifier to its location in the deployer config.
///
/// Accepted formats:
/// - `"sequencer"` → `Sequencer(0)`
/// - `"sequencer-N"` → `Sequencer(N)`
/// - `"validator-N"` → `Validator(N-1)` (user-facing 1-indexed → 0-indexed)
pub fn resolve_node(deployer: &Deployer, identifier: &str) -> Result<NodeLocation> {
    let identifier = identifier.trim();

    if identifier == "sequencer" {
        if deployer.l2_stack.sequencers.is_empty() {
            anyhow::bail!("No sequencers in the deployment");
        }
        return Ok(NodeLocation::Sequencer(0));
    }

    if let Some(suffix) = identifier.strip_prefix("sequencer-") {
        let index: usize = suffix
            .parse()
            .with_context(|| format!("Invalid sequencer index: '{}'", suffix))?;
        if index >= deployer.l2_stack.sequencers.len() {
            anyhow::bail!(
                "Sequencer index {} out of range (have {} sequencer(s))",
                index,
                deployer.l2_stack.sequencers.len()
            );
        }
        return Ok(NodeLocation::Sequencer(index));
    }

    if let Some(suffix) = identifier.strip_prefix("validator-") {
        let one_indexed: usize = suffix
            .parse()
            .with_context(|| format!("Invalid validator index: '{}'", suffix))?;
        if one_indexed == 0 {
            anyhow::bail!("Validator indices are 1-based (use 'validator-1' for the first)");
        }
        let index = one_indexed - 1;
        if index >= deployer.l2_stack.validators.len() {
            anyhow::bail!(
                "Validator index {} out of range (have {} validator(s))",
                one_indexed,
                deployer.l2_stack.validators.len()
            );
        }
        return Ok(NodeLocation::Validator(index));
    }

    anyhow::bail!(
        "Unknown node identifier '{}'. Use 'sequencer', 'sequencer-N', or 'validator-N'",
        identifier
    )
}

/// Get the L2 node builder at a given location.
fn get_node_builder<'a>(deployer: &'a Deployer, loc: &NodeLocation) -> &'a L2NodeBuilder {
    match loc {
        NodeLocation::Sequencer(i) => &deployer.l2_stack.sequencers[*i],
        NodeLocation::Validator(i) => &deployer.l2_stack.validators[*i],
    }
}

/// Get the container names for a node (op-reth, kona-node, optional conductor).
pub fn node_container_names(deployer: &Deployer, loc: &NodeLocation) -> Vec<String> {
    let node = get_node_builder(deployer, loc);
    let mut names = vec![
        node.op_reth.container_name.clone(),
        node.kona_node.container_name.clone(),
    ];
    if let Some(ref conductor) = node.op_conductor {
        names.push(conductor.container_name.clone());
    }
    names
}

/// Add a new validator node to a running network.
///
/// Creates a new validator, deploys it with P2P bootnodes from existing nodes,
/// updates the deployer config, and saves to Kupcake.toml.
pub async fn add_validator(
    deployer: &mut Deployer,
    docker: &mut KupDocker,
) -> Result<L2NodeHandler> {
    let validator_index = deployer.l2_stack.validators.len() + 1;
    tracing::info!(
        validator_index,
        "Adding new validator node to running network"
    );

    // Derive the network prefix from the existing primary sequencer's container name.
    // E.g., "kup-mynet-op-reth" → "kup-mynet"
    let primary_reth_name = &deployer.l2_stack.sequencers[0].op_reth.container_name;
    let network_prefix = primary_reth_name
        .strip_suffix("-op-reth")
        .unwrap_or(primary_reth_name);

    // Create new validator builder with network-prefixed container names
    let mut new_validator = L2NodeBuilder::validator();
    new_validator.op_reth.container_name =
        format!("{}-op-reth-validator-{}", network_prefix, validator_index);
    new_validator.kona_node.container_name =
        format!("{}-kona-node-validator-{}", network_prefix, validator_index);

    // Copy Docker image config and settings from existing nodes
    let primary = &deployer.l2_stack.sequencers[0];
    new_validator.op_reth.docker_image = primary.op_reth.docker_image.clone();
    new_validator.kona_node.docker_image = primary.kona_node.docker_image.clone();
    new_validator.kona_node.l1_slot_duration = primary.kona_node.l1_slot_duration;

    // Compute enodes from all existing nodes' persisted P2P keys
    let op_reth_enodes = deployer.l2_stack.compute_op_reth_enodes();
    let kona_node_enodes = deployer.l2_stack.compute_kona_node_enodes();

    if op_reth_enodes.is_empty() {
        tracing::warn!(
            "No persisted P2P keys found in config. \
             The new validator may not discover peers. \
             Consider redeploying the network to persist P2P keys."
        );
    }

    tracing::info!(
        op_reth_bootnodes = op_reth_enodes.len(),
        kona_node_bootnodes = kona_node_enodes.len(),
        "Computed bootnodes from existing network"
    );

    // Derive accounts from mnemonic for the block signer key
    let accounts = Deployer::derive_accounts()?;
    let unsafe_block_signer_key = hex::encode(&accounts.unsafe_block_signer.private_key);

    // Get the primary sequencer's internal RPC URL for the validator to follow
    let sequencer_rpc = deployer.l2_stack.sequencers[0].op_reth.docker_rpc_url();
    let sequencer_rpc_url =
        url::Url::parse(&sequencer_rpc).context("Failed to parse primary sequencer RPC URL")?;

    // Get L1 (Anvil) internal RPC URL from config
    let l1_rpc_url = format!(
        "http://{}:{}/",
        deployer.anvil.container_name, deployer.anvil.port
    );

    let l2_nodes_data_path = deployer.outdata.join("l2-stack");

    let input = L2NodeInput {
        l1_rpc_url,
        l1_host_url: None, // New validators don't need host URL for L1 config (already generated)
        unsafe_block_signer_key,
        sequencer_rpc: Some(sequencer_rpc_url),
        kona_node_enodes,
        op_reth_enodes,
        l1_chain_id: deployer.l1_chain_id,
        conductor_context: ConductorContext::None,
        sequencer_flashblocks_relay_url: None,
        op_reth_p2p_secret_key: None,
    };

    let handler = new_validator
        .deploy(docker, &l2_nodes_data_path, input)
        .await
        .with_context(|| format!("Failed to deploy validator-{}", validator_index))?;

    tracing::info!(
        op_reth_container = %handler.op_reth.container_name,
        kona_node_container = %handler.kona_node.container_name,
        op_reth_enode = %handler.op_reth.enode(),
        "New validator node deployed"
    );

    // Persist the new validator's P2P keys into the builder before saving
    let mut builder = new_validator;
    builder.op_reth.p2p_secret_key = Some(handler.op_reth.p2p_keypair.private_key.clone());
    builder.kona_node.p2p_secret_key = Some(handler.kona_node.p2p_keypair.private_key.clone());

    deployer.l2_stack.validators.push(builder);
    deployer
        .save_config()
        .context("Failed to save updated config after adding validator")?;

    tracing::info!(
        validator_count = deployer.l2_stack.validators.len(),
        "Config updated and saved"
    );

    // Restart Prometheus with updated scrape targets if monitoring is enabled
    if deployer.monitoring.enabled {
        let targets = deployer.build_metrics_targets_from_config();
        let monitoring_path = deployer.outdata.join("monitoring");
        deployer
            .monitoring
            .restart_prometheus(docker, &monitoring_path, &targets)
            .await
            .context("Failed to restart Prometheus after adding validator")?;
    }

    Ok(handler)
}

/// Remove a node from a running network.
///
/// Stops and removes the node's containers, removes it from the config,
/// and optionally cleans up data directories.
pub async fn remove_node(
    deployer: &mut Deployer,
    docker: &KupDocker,
    identifier: &str,
    cleanup_data: bool,
) -> Result<()> {
    let loc = resolve_node(deployer, identifier)?;

    // Validate: cannot remove the primary sequencer
    if loc == NodeLocation::Sequencer(0) {
        anyhow::bail!(
            "Cannot remove the primary sequencer (sequencer). \
             Other services (op-batcher, op-proposer) depend on it."
        );
    }

    // Validate: cannot remove the only sequencer
    if matches!(loc, NodeLocation::Sequencer(_)) && deployer.l2_stack.sequencers.len() <= 1 {
        anyhow::bail!("Cannot remove the only sequencer from the network");
    }

    let container_names = node_container_names(deployer, &loc);
    let node = get_node_builder(deployer, &loc);
    let op_reth_name = node.op_reth.container_name.clone();

    tracing::info!(
        node = %loc,
        containers = ?container_names,
        "Removing node from network"
    );

    // Stop and remove all containers for this node
    for name in &container_names {
        if let Err(e) = docker.stop_and_remove_container(&name.to_string()).await {
            tracing::warn!(container = %name, error = %e, "Failed to stop/remove container (may already be stopped)");
        }
    }

    // Remove from config
    match loc {
        NodeLocation::Sequencer(i) => {
            deployer.l2_stack.sequencers.remove(i);
        }
        NodeLocation::Validator(i) => {
            deployer.l2_stack.validators.remove(i);
        }
    }

    // Clean up data directories if requested
    if cleanup_data {
        let l2_data = deployer.outdata.join("l2-stack");
        let reth_data = l2_data.join(format!("reth-data-{}", op_reth_name));
        if reth_data.exists() {
            std::fs::remove_dir_all(&reth_data)
                .with_context(|| format!("Failed to remove reth data: {}", reth_data.display()))?;
            tracing::info!(path = %reth_data.display(), "Removed reth data directory");
        }

        // Remove JWT file
        let jwt_pattern = format!("jwt-{}.hex", op_reth_name);
        let jwt_path = l2_data.join(&jwt_pattern);
        if jwt_path.exists() {
            std::fs::remove_file(&jwt_path)
                .with_context(|| format!("Failed to remove JWT file: {}", jwt_path.display()))?;
        }
    }

    deployer
        .save_config()
        .context("Failed to save updated config after removing node")?;

    tracing::info!(
        node = %loc,
        sequencer_count = deployer.l2_stack.sequencers.len(),
        validator_count = deployer.l2_stack.validators.len(),
        "Node removed and config updated"
    );

    Ok(())
}

/// Pause all containers for a node (Docker pause — freezes the process).
pub async fn pause_node(deployer: &Deployer, docker: &KupDocker, identifier: &str) -> Result<()> {
    let loc = resolve_node(deployer, identifier)?;
    let names = node_container_names(deployer, &loc);

    tracing::info!(node = %loc, "Pausing node");

    for name in &names {
        docker
            .pause_container(name)
            .await
            .with_context(|| format!("Failed to pause container '{}'", name))?;
    }

    tracing::info!(node = %loc, "Node paused");
    Ok(())
}

/// Unpause all containers for a paused node.
pub async fn unpause_node(deployer: &Deployer, docker: &KupDocker, identifier: &str) -> Result<()> {
    let loc = resolve_node(deployer, identifier)?;
    let names = node_container_names(deployer, &loc);

    tracing::info!(node = %loc, "Unpausing node");

    for name in &names {
        docker
            .unpause_container(name)
            .await
            .with_context(|| format!("Failed to unpause container '{}'", name))?;
    }

    tracing::info!(node = %loc, "Node unpaused");
    Ok(())
}

/// Restart all containers for a node (Docker restart — stop + start).
pub async fn restart_node(deployer: &Deployer, docker: &KupDocker, identifier: &str) -> Result<()> {
    let loc = resolve_node(deployer, identifier)?;
    let names = node_container_names(deployer, &loc);

    tracing::info!(node = %loc, "Restarting node");

    for name in &names {
        docker
            .restart_container(name)
            .await
            .with_context(|| format!("Failed to restart container '{}'", name))?;
    }

    tracing::info!(node = %loc, "Node restarted");
    Ok(())
}
