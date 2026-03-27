//! Network inspection module for detailed status reporting.
//!
//! Provides a comprehensive snapshot of a deployed Kupcake network including
//! container states, host URLs, block heights, sync status, timestamps,
//! and optionally extended details like gas price, peer count, and L1 origin.

use std::fmt;

use anyhow::{Context, Result};
use bollard::Docker;
use comfy_table::{Attribute, Cell, Color, Table};
use serde::Serialize;
use serde_json::Value;

use crate::{ContainerState, Deployer, health, rpc};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Top-level inspection report for the entire network.
#[derive(Debug, Serialize)]
pub struct InspectReport {
    pub network_name: String,
    pub l1_chain_id: u64,
    pub l2_chain_id: u64,
    pub l1: Option<L1Inspect>,
    pub nodes: Vec<NodeInspect>,
    pub services: Vec<ServiceInspect>,
}

/// L1 (Anvil) inspection data.
#[derive(Debug, Serialize)]
pub struct L1Inspect {
    pub container_name: String,
    pub state: ContainerState,
    pub host_url: Option<String>,
    pub block_number: Option<u64>,
    pub timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_price: Option<String>,
}

/// L2 node (op-reth + kona-node pair) inspection data.
#[derive(Debug, Serialize)]
pub struct NodeInspect {
    pub role: String,
    pub label: String,
    pub execution: ExecutionInspect,
    pub consensus: ConsensusInspect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conductor: Option<ServiceInspect>,
}

/// Execution client (op-reth) inspection data.
#[derive(Debug, Serialize)]
pub struct ExecutionInspect {
    pub container_name: String,
    pub state: ContainerState,
    pub host_url: Option<String>,
    pub block_number: Option<u64>,
    pub is_syncing: Option<bool>,
    pub timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_tx_count: Option<u64>,
}

/// Consensus client (kona-node) inspection data.
#[derive(Debug, Serialize)]
pub struct ConsensusInspect {
    pub container_name: String,
    pub state: ContainerState,
    pub host_url: Option<String>,
    pub unsafe_l2: Option<u64>,
    pub safe_l2: Option<u64>,
    pub finalized_l2: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_l1: Option<BlockRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_l1: Option<BlockRef>,
}

/// A block reference (number + hash).
#[derive(Debug, Serialize)]
pub struct BlockRef {
    pub number: u64,
    pub hash: String,
}

/// Infrastructure service inspection data.
#[derive(Debug, Serialize)]
pub struct ServiceInspect {
    pub label: String,
    pub container_name: String,
    pub state: ContainerState,
    pub host_url: Option<String>,
}

/// Result from querying optimism_syncStatus.
#[derive(Default)]
struct SyncStatusResult {
    unsafe_l2: Option<u64>,
    safe_l2: Option<u64>,
    finalized_l2: Option<u64>,
    head_l1: Option<BlockRef>,
    current_l1: Option<BlockRef>,
}

/// Shared context threaded through all inspection functions.
struct InspectCtx<'a> {
    docker: &'a Docker,
    client: &'a reqwest::Client,
    verbose: bool,
}

// ---------------------------------------------------------------------------
// Inspection logic
// ---------------------------------------------------------------------------

/// Run a full inspection of a deployed network.
pub async fn inspect_network(
    deployer: &Deployer,
    verbose: bool,
    service_filter: Option<&str>,
) -> Result<InspectReport> {
    let docker =
        Docker::connect_with_local_defaults().context("Failed to connect to Docker daemon")?;
    let client = rpc::create_client()?;
    let ctx = InspectCtx {
        docker: &docker,
        client: &client,
        verbose,
    };

    let network_name = deployer
        .docker
        .net_name
        .strip_suffix("-network")
        .unwrap_or(&deployer.docker.net_name)
        .to_string();

    // L1 (Anvil)
    let l1 = inspect_l1(&ctx, deployer).await;

    // L2 nodes
    let mut nodes = Vec::new();
    for (i, seq) in deployer.l2_stack.sequencers.iter().enumerate() {
        let label = if i == 0 {
            "sequencer".to_string()
        } else {
            format!("sequencer-{i}")
        };
        let node = inspect_l2_node(
            &ctx,
            "sequencer",
            &label,
            &seq.op_reth.container_name,
            seq.op_reth.http_port,
            &seq.kona_node.container_name,
            seq.kona_node.rpc_port,
            seq.op_conductor
                .as_ref()
                .map(|c| (&c.container_name, c.rpc_port)),
        )
        .await;
        nodes.push(node);
    }
    for (i, val) in deployer.l2_stack.validators.iter().enumerate() {
        let label = format!("validator-{}", i + 1);
        let node = inspect_l2_node(
            &ctx,
            "validator",
            &label,
            &val.op_reth.container_name,
            val.op_reth.http_port,
            &val.kona_node.container_name,
            val.kona_node.rpc_port,
            None,
        )
        .await;
        nodes.push(node);
    }

    // Infrastructure services
    let mut services = vec![
        inspect_service(
            &ctx,
            "op-batcher",
            &deployer.l2_stack.op_batcher.container_name,
            deployer.l2_stack.op_batcher.rpc_port,
        )
        .await,
    ];
    if let Some(ref proposer) = deployer.l2_stack.op_proposer {
        services.push(
            inspect_service(
                &ctx,
                "op-proposer",
                &proposer.container_name,
                proposer.rpc_port,
            )
            .await,
        );
    }
    if let Some(ref challenger) = deployer.l2_stack.op_challenger {
        services.push(
            inspect_service(
                &ctx,
                "op-challenger",
                &challenger.container_name,
                challenger.metrics_port,
            )
            .await,
        );
    }

    let mut report = InspectReport {
        network_name,
        l1_chain_id: deployer.l1_chain_id,
        l2_chain_id: deployer.l2_chain_id,
        l1: Some(l1),
        nodes,
        services,
    };

    if let Some(filter) = service_filter {
        apply_filter(&mut report, filter);
    }

    Ok(report)
}

/// Apply a service name filter, keeping only matching entries.
fn apply_filter(report: &mut InspectReport, filter: &str) {
    let filter_lower = filter.to_lowercase();

    let l1_matches = report
        .l1
        .as_ref()
        .is_some_and(|l1| matches_filter(&filter_lower, &["anvil", "l1"], &l1.container_name));

    if !l1_matches {
        report.l1 = None;
    }

    report.nodes.retain(|node| {
        matches_filter(
            &filter_lower,
            &[&node.label, &node.role],
            &node.execution.container_name,
        ) || matches_filter(&filter_lower, &[], &node.consensus.container_name)
    });

    report
        .services
        .retain(|svc| matches_filter(&filter_lower, &[&svc.label], &svc.container_name));
}

/// Check if a filter matches any of the labels or the container name (case-insensitive substring).
fn matches_filter(filter: &str, labels: &[&str], container_name: &str) -> bool {
    labels.iter().any(|label| label.to_lowercase() == *filter)
        || container_name.to_lowercase().contains(filter)
}

// ---------------------------------------------------------------------------
// L1 inspection
// ---------------------------------------------------------------------------

async fn inspect_l1(ctx: &InspectCtx<'_>, deployer: &Deployer) -> L1Inspect {
    let name = &deployer.anvil.container_name;
    let state = container_state(ctx.docker, name).await;
    let host_url = health::build_host_rpc_url(ctx.docker, name, deployer.anvil.port).await;

    let (block_number, timestamp, gas_price) = match host_url.as_deref() {
        Some(url) if state == ContainerState::Running => {
            let bn = query_block_number(ctx.client, url).await;
            let ts = query_block_timestamp(ctx.client, url).await;
            let gas = if ctx.verbose {
                query_gas_price(ctx.client, url).await
            } else {
                None
            };
            (bn, ts, gas)
        }
        _ => (None, None, None),
    };

    L1Inspect {
        container_name: name.clone(),
        state,
        host_url,
        block_number,
        timestamp,
        gas_price,
    }
}

// ---------------------------------------------------------------------------
// L2 node inspection
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn inspect_l2_node(
    ctx: &InspectCtx<'_>,
    role: &str,
    label: &str,
    reth_name: &str,
    reth_port: u16,
    kona_name: &str,
    kona_port: u16,
    conductor: Option<(&String, u16)>,
) -> NodeInspect {
    let execution = inspect_execution(ctx, reth_name, reth_port).await;
    let consensus = inspect_consensus(ctx, kona_name, kona_port).await;
    let conductor_inspect = match conductor {
        Some((name, port)) => Some(inspect_service(ctx, "op-conductor", name, port).await),
        None => None,
    };

    NodeInspect {
        role: role.to_string(),
        label: label.to_string(),
        execution,
        consensus,
        conductor: conductor_inspect,
    }
}

async fn inspect_execution(
    ctx: &InspectCtx<'_>,
    container_name: &str,
    container_port: u16,
) -> ExecutionInspect {
    let state = container_state(ctx.docker, container_name).await;
    let host_url = health::build_host_rpc_url(ctx.docker, container_name, container_port).await;

    let (block_number, is_syncing, timestamp, gas_price, peer_count, pending_tx_count) =
        match host_url.as_deref() {
            Some(url) if state == ContainerState::Running => {
                let bn = query_block_number(ctx.client, url).await;
                let syncing = query_syncing(ctx.client, url).await;
                let ts = query_block_timestamp(ctx.client, url).await;
                let (gas, peers, pending) = if ctx.verbose {
                    (
                        query_gas_price(ctx.client, url).await,
                        query_peer_count(ctx.client, url).await,
                        query_pending_tx_count(ctx.client, url).await,
                    )
                } else {
                    (None, None, None)
                };
                (bn, syncing, ts, gas, peers, pending)
            }
            _ => (None, None, None, None, None, None),
        };

    ExecutionInspect {
        container_name: container_name.to_string(),
        state,
        host_url,
        block_number,
        is_syncing,
        timestamp,
        gas_price,
        peer_count,
        pending_tx_count,
    }
}

async fn inspect_consensus(
    ctx: &InspectCtx<'_>,
    container_name: &str,
    container_port: u16,
) -> ConsensusInspect {
    let state = container_state(ctx.docker, container_name).await;
    let host_url = health::build_host_rpc_url(ctx.docker, container_name, container_port).await;

    let sync = match host_url.as_deref() {
        Some(url) if state == ContainerState::Running => {
            query_full_sync_status(ctx.client, url, ctx.verbose).await
        }
        _ => SyncStatusResult::default(),
    };

    ConsensusInspect {
        container_name: container_name.to_string(),
        state,
        host_url,
        unsafe_l2: sync.unsafe_l2,
        safe_l2: sync.safe_l2,
        finalized_l2: sync.finalized_l2,
        head_l1: sync.head_l1,
        current_l1: sync.current_l1,
    }
}

// ---------------------------------------------------------------------------
// Infrastructure service inspection
// ---------------------------------------------------------------------------

async fn inspect_service(
    ctx: &InspectCtx<'_>,
    label: &str,
    container_name: &str,
    container_port: u16,
) -> ServiceInspect {
    let state = container_state(ctx.docker, container_name).await;
    let host_url = health::build_host_rpc_url(ctx.docker, container_name, container_port).await;

    ServiceInspect {
        label: label.to_string(),
        container_name: container_name.to_string(),
        state,
        host_url,
    }
}

// ---------------------------------------------------------------------------
// Docker helpers
// ---------------------------------------------------------------------------

/// Get the state of a container via Docker inspect.
async fn container_state(docker: &Docker, container_name: &str) -> ContainerState {
    match docker.inspect_container(container_name, None).await {
        Ok(info) => {
            let status = info
                .state
                .and_then(|s| s.status)
                .map(|s| s.to_string())
                .unwrap_or_default();
            match status.as_str() {
                "running" => ContainerState::Running,
                "paused" => ContainerState::Paused,
                "restarting" => ContainerState::Restarting,
                "exited" | "dead" | "created" => ContainerState::Stopped,
                _ => ContainerState::Stopped,
            }
        }
        Err(_) => ContainerState::NotFound,
    }
}

// ---------------------------------------------------------------------------
// RPC query helpers
// ---------------------------------------------------------------------------

/// Parse a hex string (with or without 0x prefix) to u64.
fn parse_hex_u64(hex: &str) -> Option<u64> {
    u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok()
}

/// Query eth_blockNumber and parse the hex result.
async fn query_block_number(client: &reqwest::Client, url: &str) -> Option<u64> {
    let result: String = rpc::json_rpc_call(client, url, "eth_blockNumber", vec![])
        .await
        .ok()?;
    parse_hex_u64(&result)
}

/// Query eth_getBlockByNumber("latest") and extract the timestamp.
async fn query_block_timestamp(client: &reqwest::Client, url: &str) -> Option<u64> {
    let block: Value = rpc::json_rpc_call(
        client,
        url,
        "eth_getBlockByNumber",
        vec![Value::String("latest".to_string()), Value::Bool(false)],
    )
    .await
    .ok()?;

    parse_hex_u64(block.get("timestamp")?.as_str()?)
}

/// Query eth_syncing — returns Some(false) if not syncing, Some(true) if syncing.
async fn query_syncing(client: &reqwest::Client, url: &str) -> Option<bool> {
    let result: Value = rpc::json_rpc_call(client, url, "eth_syncing", vec![])
        .await
        .ok()?;

    // eth_syncing returns `false` when not syncing, or an object when syncing
    Some(!matches!(result, Value::Bool(false)))
}

/// Query eth_gasPrice and return as decimal string (wei).
async fn query_gas_price(client: &reqwest::Client, url: &str) -> Option<String> {
    let result: String = rpc::json_rpc_call(client, url, "eth_gasPrice", vec![])
        .await
        .ok()?;
    parse_hex_u64(&result).map(|wei| format!("{wei}"))
}

/// Query net_peerCount.
async fn query_peer_count(client: &reqwest::Client, url: &str) -> Option<u64> {
    let result: String = rpc::json_rpc_call(client, url, "net_peerCount", vec![])
        .await
        .ok()?;
    parse_hex_u64(&result)
}

/// Query pending transaction count via eth_getBlockByNumber("pending").
async fn query_pending_tx_count(client: &reqwest::Client, url: &str) -> Option<u64> {
    let block: Value = rpc::json_rpc_call(
        client,
        url,
        "eth_getBlockByNumber",
        vec![Value::String("pending".to_string()), Value::Bool(false)],
    )
    .await
    .ok()?;

    let txs = block.get("transactions")?.as_array()?;
    Some(txs.len() as u64)
}

/// Query optimism_syncStatus — returns L2 heads and optionally L1 references.
async fn query_full_sync_status(
    client: &reqwest::Client,
    url: &str,
    verbose: bool,
) -> SyncStatusResult {
    let Ok(value): Result<Value, _> =
        rpc::json_rpc_call(client, url, "optimism_syncStatus", vec![]).await
    else {
        return SyncStatusResult::default();
    };

    let block_num = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.get("number"))
            .and_then(|v| v.as_u64())
    };

    let block_ref = |key: &str| -> Option<BlockRef> {
        let obj = value.get(key)?;
        let number = obj.get("number")?.as_u64()?;
        let hash = obj.get("hash")?.as_str()?.to_string();
        Some(BlockRef { number, hash })
    };

    let (head_l1, current_l1) = if verbose {
        (block_ref("head_l1"), block_ref("current_l1"))
    } else {
        (None, None)
    };

    SyncStatusResult {
        unsafe_l2: block_num("unsafe_l2"),
        safe_l2: block_num("safe_l2"),
        finalized_l2: block_num("finalized_l2"),
        head_l1,
        current_l1,
    }
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Return a colored `Cell` for a `ContainerState`.
fn state_cell(state: ContainerState) -> Cell {
    let (text, color) = match state {
        ContainerState::Running => ("Running", Color::Green),
        ContainerState::Paused => ("Paused", Color::Yellow),
        ContainerState::Stopped => ("Stopped", Color::Red),
        ContainerState::Restarting => ("Restarting", Color::Yellow),
        ContainerState::NotFound => ("Not Found", Color::Red),
    };
    Cell::new(text).fg(color)
}

/// Create a bold header cell.
fn header(text: &str) -> Cell {
    Cell::new(text).add_attribute(Attribute::Bold)
}

/// Display a value or "-" if `None`.
fn val_or_dash<T: fmt::Display>(opt: Option<T>) -> String {
    opt.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

/// Format L2 consensus heads compactly: "U:100 S:99 F:98".
fn format_l2_heads(c: &ConsensusInspect) -> String {
    [
        c.unsafe_l2.map(|v| format!("U:{v}")),
        c.safe_l2.map(|v| format!("S:{v}")),
        c.finalized_l2.map(|v| format!("F:{v}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

/// Format a Unix timestamp as a human-readable UTC string.
fn format_timestamp(ts: u64) -> String {
    i64::try_from(ts)
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// Tracks which verbose columns have data across all nodes.
struct VerboseColumns {
    gas: bool,
    peers: bool,
    pending: bool,
    l1_refs: bool,
}

impl VerboseColumns {
    fn from_nodes(nodes: &[NodeInspect]) -> Self {
        Self {
            gas: nodes.iter().any(|n| n.execution.gas_price.is_some()),
            peers: nodes.iter().any(|n| n.execution.peer_count.is_some()),
            pending: nodes.iter().any(|n| n.execution.pending_tx_count.is_some()),
            l1_refs: nodes
                .iter()
                .any(|n| n.consensus.head_l1.is_some() || n.consensus.current_l1.is_some()),
        }
    }

    fn append_headers(&self, headers: &mut Vec<Cell>) {
        if self.gas {
            headers.push(header("Gas (wei)"));
        }
        if self.peers {
            headers.push(header("Peers"));
        }
        if self.pending {
            headers.push(header("Pending Txs"));
        }
        if self.l1_refs {
            headers.push(header("L1 Head"));
            headers.push(header("L1 Current"));
        }
    }

    fn pad_empty(&self, row: &mut Vec<Cell>) {
        if self.gas {
            row.push(Cell::new(""));
        }
        if self.peers {
            row.push(Cell::new(""));
        }
        if self.pending {
            row.push(Cell::new(""));
        }
        if self.l1_refs {
            row.push(Cell::new(""));
            row.push(Cell::new(""));
        }
    }
}

// ---------------------------------------------------------------------------
// Table builders
// ---------------------------------------------------------------------------

/// Build the L1 (Anvil) table.
fn build_l1_table(l1: &L1Inspect) -> Table {
    let mut table = Table::new();

    let has_gas = l1.gas_price.is_some();

    let mut headers = vec![
        header("Container"),
        header("State"),
        header("Block"),
        header("Timestamp"),
        header("URL"),
    ];
    if has_gas {
        headers.push(header("Gas (wei)"));
    }
    table.set_header(headers);

    let mut row = vec![
        Cell::new(&l1.container_name),
        state_cell(l1.state),
        Cell::new(val_or_dash(l1.block_number)),
        Cell::new(
            l1.timestamp
                .map(format_timestamp)
                .unwrap_or_else(|| "-".into()),
        ),
        Cell::new(l1.host_url.as_deref().unwrap_or("-")),
    ];
    if has_gas {
        row.push(Cell::new(l1.gas_price.as_deref().unwrap_or("-")));
    }
    table.add_row(row);
    table
}

/// Build the L2 Nodes table.
fn build_nodes_table(nodes: &[NodeInspect]) -> Table {
    let mut table = Table::new();
    let verbose = VerboseColumns::from_nodes(nodes);

    let mut headers = vec![
        header("Node"),
        header("Layer"),
        header("Container"),
        header("State"),
        header("Block / Heads"),
        header("Info"),
        header("URL"),
    ];
    verbose.append_headers(&mut headers);
    table.set_header(headers);

    for node in nodes {
        // Execution row
        let syncing_info = node
            .execution
            .is_syncing
            .map(|s| format!("Syncing: {s}"))
            .unwrap_or_default();

        let mut exec_row = vec![
            Cell::new(&node.label).add_attribute(Attribute::Bold),
            Cell::new("op-reth"),
            Cell::new(&node.execution.container_name),
            state_cell(node.execution.state),
            Cell::new(val_or_dash(node.execution.block_number)),
            Cell::new(syncing_info),
            Cell::new(node.execution.host_url.as_deref().unwrap_or("-")),
        ];
        if verbose.gas {
            exec_row.push(Cell::new(val_or_dash(node.execution.gas_price.as_deref())));
        }
        if verbose.peers {
            exec_row.push(Cell::new(val_or_dash(node.execution.peer_count)));
        }
        if verbose.pending {
            exec_row.push(Cell::new(val_or_dash(node.execution.pending_tx_count)));
        }
        if verbose.l1_refs {
            exec_row.push(Cell::new(""));
            exec_row.push(Cell::new(""));
        }
        table.add_row(exec_row);

        // Consensus row
        let mut cons_row = vec![
            Cell::new(""),
            Cell::new("kona-node"),
            Cell::new(&node.consensus.container_name),
            state_cell(node.consensus.state),
            Cell::new(format_l2_heads(&node.consensus)),
            Cell::new(""),
            Cell::new(node.consensus.host_url.as_deref().unwrap_or("-")),
        ];
        verbose.pad_empty(&mut cons_row);
        if verbose.l1_refs {
            // Overwrite the last two empty cells with actual L1 ref data
            let len = cons_row.len();
            if let Some(ref r) = node.consensus.head_l1 {
                cons_row[len - 2] = Cell::new(format!("#{} ({})", r.number, r.hash));
            }
            if let Some(ref r) = node.consensus.current_l1 {
                cons_row[len - 1] = Cell::new(format!("#{} ({})", r.number, r.hash));
            }
        }
        table.add_row(cons_row);

        // Conductor row (if present)
        if let Some(ref cond) = node.conductor {
            let mut cond_row = vec![
                Cell::new(""),
                Cell::new("conductor"),
                Cell::new(&cond.container_name),
                state_cell(cond.state),
                Cell::new(""),
                Cell::new(""),
                Cell::new(cond.host_url.as_deref().unwrap_or("-")),
            ];
            verbose.pad_empty(&mut cond_row);
            table.add_row(cond_row);
        }
    }

    table
}

/// Build the Services table.
fn build_services_table(services: &[ServiceInspect]) -> Table {
    let mut table = Table::new();
    table.set_header(vec![
        header("Service"),
        header("Container"),
        header("State"),
        header("URL"),
    ]);

    for svc in services {
        table.add_row(vec![
            Cell::new(&svc.label),
            Cell::new(&svc.container_name),
            state_cell(svc.state),
            Cell::new(svc.host_url.as_deref().unwrap_or("-")),
        ]);
    }

    table
}

// ---------------------------------------------------------------------------
// Display implementation
// ---------------------------------------------------------------------------

impl fmt::Display for InspectReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Network: {} (L1: {}, L2: {})",
            self.network_name, self.l1_chain_id, self.l2_chain_id
        )?;

        if let Some(ref l1) = self.l1 {
            writeln!(f)?;
            writeln!(f, "L1 (Anvil)")?;
            writeln!(f, "{}", build_l1_table(l1))?;
        }

        if !self.nodes.is_empty() {
            writeln!(f)?;
            writeln!(f, "L2 Nodes")?;
            writeln!(f, "{}", build_nodes_table(&self.nodes))?;
        }

        if !self.services.is_empty() {
            writeln!(f)?;
            writeln!(f, "Services")?;
            writeln!(f, "{}", build_services_table(&self.services))?;
        }

        Ok(())
    }
}
