//! Health check module for verifying a deployed Kupcake network.

use std::fmt;

use anyhow::{Context, Result};
use bollard::Docker;
use serde_json::Value;

use crate::{Deployer, rpc};

/// Health report for the entire network.
pub struct HealthReport {
    /// L1 (Anvil) health status.
    pub l1: L1Health,
    /// Per-node health status (sequencers and validators).
    pub nodes: Vec<NodeHealth>,
    /// Service health (batcher, proposer, challenger).
    pub services: Vec<ServiceHealth>,
    /// Overall health: all containers running, chain IDs match, blocks advancing.
    pub healthy: bool,
}

/// Health status for the L1 (Anvil) node.
pub struct L1Health {
    pub container_name: String,
    pub running: bool,
    pub chain_id: Option<u64>,
    pub expected_chain_id: u64,
    pub block_number: Option<u64>,
}

impl L1Health {
    pub fn chain_id_match(&self) -> bool {
        self.chain_id == Some(self.expected_chain_id)
    }
}

/// Health status for an L2 node (op-reth + kona-node pair).
pub struct NodeHealth {
    pub role: String,
    pub label: String,
    pub execution: ExecutionHealth,
    pub consensus: ConsensusHealth,
}

/// Health status for an op-reth execution client.
pub struct ExecutionHealth {
    pub container_name: String,
    pub running: bool,
    pub chain_id: Option<u64>,
    pub expected_chain_id: u64,
    pub block_number: Option<u64>,
}

impl ExecutionHealth {
    pub fn chain_id_match(&self) -> bool {
        self.chain_id == Some(self.expected_chain_id)
    }
}

/// Health status for a kona-node consensus client.
pub struct ConsensusHealth {
    pub container_name: String,
    pub running: bool,
    pub unsafe_l2: Option<u64>,
    pub safe_l2: Option<u64>,
    pub finalized_l2: Option<u64>,
}

/// Health status for an infrastructure service (batcher, proposer, challenger).
pub struct ServiceHealth {
    pub name: String,
    pub container_name: String,
    pub running: bool,
}

/// Shared EVM node RPC data (chain_id + block_number).
struct EvmNodeRpc {
    chain_id: Option<u64>,
    block_number: Option<u64>,
}

/// Run a full health check against a deployed network.
pub async fn health_check(deployer: &Deployer) -> Result<HealthReport> {
    let docker = Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker daemon")?;

    let client = rpc::create_client()?;

    // Check L1 (Anvil)
    let l1 = {
        let name = &deployer.anvil.container_name;
        let running = is_running(&docker, name).await;
        let evm = query_evm_node(&docker, &client, name, deployer.anvil.port, running).await;
        L1Health {
            container_name: name.clone(),
            running,
            chain_id: evm.chain_id,
            expected_chain_id: deployer.l1_chain_id,
            block_number: evm.block_number,
        }
    };

    // Check L2 nodes (sequencers + validators)
    let mut nodes = Vec::new();

    for (i, seq) in deployer.l2_stack.sequencers.iter().enumerate() {
        let label = if i == 0 {
            "sequencer".to_string()
        } else {
            format!("sequencer-{}", i)
        };
        let node = check_l2_node(
            &docker,
            &client,
            "sequencer",
            &label,
            &seq.op_reth.container_name,
            seq.op_reth.http_port,
            &seq.kona_node.container_name,
            seq.kona_node.rpc_port,
            deployer.l2_chain_id,
        )
        .await;
        nodes.push(node);
    }

    for (i, val) in deployer.l2_stack.validators.iter().enumerate() {
        let label = format!("validator-{}", i + 1);
        let node = check_l2_node(
            &docker,
            &client,
            "validator",
            &label,
            &val.op_reth.container_name,
            val.op_reth.http_port,
            &val.kona_node.container_name,
            val.kona_node.rpc_port,
            deployer.l2_chain_id,
        )
        .await;
        nodes.push(node);
    }

    // Check infrastructure services
    let services = vec![
        check_service(&docker, "op-batcher", &deployer.l2_stack.op_batcher.container_name).await,
        check_service(&docker, "op-proposer", &deployer.l2_stack.op_proposer.container_name).await,
        check_service(
            &docker,
            "op-challenger",
            &deployer.l2_stack.op_challenger.container_name,
        )
        .await,
    ];

    let healthy = compute_healthy(&l1, &nodes, &services);

    Ok(HealthReport {
        l1,
        nodes,
        services,
        healthy,
    })
}

/// Services that are not required to be running for a healthy network.
/// op-challenger is excluded because it requires additional configuration
/// (prestates) that is not yet automated.
const NON_CRITICAL_SERVICES: &[&str] = &["op-challenger"];

fn compute_healthy(l1: &L1Health, nodes: &[NodeHealth], services: &[ServiceHealth]) -> bool {
    l1.running
        && l1.chain_id_match()
        && l1.block_number.is_some()
        && nodes.iter().all(|node| {
            node.execution.running
                && node.execution.chain_id_match()
                && node.execution.block_number.unwrap_or(0) > 0
                && node.consensus.running
        })
        && services
            .iter()
            .filter(|s| !NON_CRITICAL_SERVICES.contains(&s.name.as_str()))
            .all(|s| s.running)
}

/// Query chain_id and block_number from an EVM node if it's running.
async fn query_evm_node(
    docker: &Docker,
    client: &reqwest::Client,
    container_name: &str,
    container_port: u16,
    running: bool,
) -> EvmNodeRpc {
    if !running {
        return EvmNodeRpc {
            chain_id: None,
            block_number: None,
        };
    }

    let Some(url) = build_host_rpc_url(docker, container_name, container_port).await else {
        return EvmNodeRpc {
            chain_id: None,
            block_number: None,
        };
    };

    EvmNodeRpc {
        chain_id: query_chain_id(client, &url).await,
        block_number: query_block_number(client, &url).await,
    }
}

/// Check a complete L2 node (op-reth + kona-node pair).
#[allow(clippy::too_many_arguments)]
async fn check_l2_node(
    docker: &Docker,
    client: &reqwest::Client,
    role: &str,
    label: &str,
    reth_name: &str,
    reth_port: u16,
    kona_name: &str,
    kona_port: u16,
    expected_chain_id: u64,
) -> NodeHealth {
    let reth_running = is_running(docker, reth_name).await;
    let evm = query_evm_node(docker, client, reth_name, reth_port, reth_running).await;

    let kona_running = is_running(docker, kona_name).await;
    let (unsafe_l2, safe_l2, finalized_l2) = if kona_running {
        match build_host_rpc_url(docker, kona_name, kona_port).await {
            Some(url) => query_sync_status(client, &url).await,
            None => (None, None, None),
        }
    } else {
        (None, None, None)
    };

    NodeHealth {
        role: role.to_string(),
        label: label.to_string(),
        execution: ExecutionHealth {
            container_name: reth_name.to_string(),
            running: reth_running,
            chain_id: evm.chain_id,
            expected_chain_id,
            block_number: evm.block_number,
        },
        consensus: ConsensusHealth {
            container_name: kona_name.to_string(),
            running: kona_running,
            unsafe_l2,
            safe_l2,
            finalized_l2,
        },
    }
}

/// Check if an infrastructure service container is running.
async fn check_service(docker: &Docker, name: &str, container_name: &str) -> ServiceHealth {
    ServiceHealth {
        name: name.to_string(),
        container_name: container_name.to_string(),
        running: is_running(docker, container_name).await,
    }
}

/// Check if a container is running via Docker inspect.
async fn is_running(docker: &Docker, container_name: &str) -> bool {
    docker
        .inspect_container(container_name, None)
        .await
        .ok()
        .and_then(|info| info.state)
        .and_then(|s| s.running)
        .unwrap_or(false)
}

/// Build a host-accessible RPC URL by inspecting the container's bound ports.
///
/// Returns the `http://localhost:<host_port>/` URL if the container has
/// the given port published to the host.
async fn build_host_rpc_url(
    docker: &Docker,
    container_name: &str,
    container_port: u16,
) -> Option<String> {
    let inspect = docker.inspect_container(container_name, None).await.ok()?;
    let ports = inspect.network_settings?.ports?;
    let key = format!("{}/tcp", container_port);
    let bindings = ports.get(&key)?.as_ref()?;

    bindings
        .iter()
        .find_map(|b| b.host_port.as_ref().filter(|p| !p.is_empty()))
        .map(|host_port| format!("http://localhost:{}/", host_port))
}

/// Query eth_chainId and parse the hex result to u64.
async fn query_chain_id(client: &reqwest::Client, url: &str) -> Option<u64> {
    let result: String = rpc::json_rpc_call(client, url, "eth_chainId", vec![])
        .await
        .ok()?;
    u64::from_str_radix(result.trim_start_matches("0x"), 16).ok()
}

/// Query eth_blockNumber and parse the hex result to u64.
async fn query_block_number(client: &reqwest::Client, url: &str) -> Option<u64> {
    let result: String = rpc::json_rpc_call(client, url, "eth_blockNumber", vec![])
        .await
        .ok()?;
    u64::from_str_radix(result.trim_start_matches("0x"), 16).ok()
}

/// Query optimism_syncStatus and extract block numbers.
async fn query_sync_status(
    client: &reqwest::Client,
    url: &str,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    let Ok(value): Result<Value, _> =
        rpc::json_rpc_call(client, url, "optimism_syncStatus", vec![]).await
    else {
        return (None, None, None);
    };

    let block_num = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.get("number"))
            .and_then(|v| v.as_u64())
    };

    (block_num("unsafe_l2"), block_num("safe_l2"), block_num("finalized_l2"))
}

// -- Display implementations for printing health reports --

impl fmt::Display for HealthReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.healthy { "HEALTHY" } else { "UNHEALTHY" };
        writeln!(f, "Network Status: {}", status)?;
        writeln!(f)?;

        writeln!(f, "=== L1 (Anvil) ===")?;
        writeln!(f, "{}", self.l1)?;

        writeln!(f, "=== L2 Nodes ===")?;
        for node in &self.nodes {
            writeln!(f, "{}", node)?;
        }

        writeln!(f, "=== Services ===")?;
        for service in &self.services {
            writeln!(f, "{}", service)?;
        }

        Ok(())
    }
}

impl fmt::Display for L1Health {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = status_icon(self.running);
        write!(f, "  {} {} ", status, self.container_name)?;

        if let Some(chain_id) = self.chain_id {
            let cid_status = if self.chain_id_match() { "ok" } else { "MISMATCH" };
            write!(f, "chain_id={} ({}) ", chain_id, cid_status)?;
        }

        if let Some(bn) = self.block_number {
            write!(f, "block={}", bn)?;
        }

        Ok(())
    }
}

impl fmt::Display for NodeHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  [{}]", self.label)?;

        // Execution
        let ex = &self.execution;
        let status = status_icon(ex.running);
        write!(f, "    {} {} ", status, ex.container_name)?;
        if let Some(chain_id) = ex.chain_id {
            let cid_status = if ex.chain_id_match() { "ok" } else { "MISMATCH" };
            write!(f, "chain_id={} ({}) ", chain_id, cid_status)?;
        }
        if let Some(bn) = ex.block_number {
            write!(f, "block={}", bn)?;
        }
        writeln!(f)?;

        // Consensus
        let cn = &self.consensus;
        let status = status_icon(cn.running);
        write!(f, "    {} {} ", status, cn.container_name)?;
        if let Some(v) = cn.unsafe_l2 {
            write!(f, "unsafe={} ", v)?;
        }
        if let Some(v) = cn.safe_l2 {
            write!(f, "safe={} ", v)?;
        }
        if let Some(v) = cn.finalized_l2 {
            write!(f, "finalized={}", v)?;
        }

        Ok(())
    }
}

impl fmt::Display for ServiceHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = status_icon(self.running);
        write!(f, "  {} {} ({})", status, self.name, self.container_name)?;
        if !self.running && NON_CRITICAL_SERVICES.contains(&self.name.as_str()) {
            write!(f, " [non-critical]")?;
        }
        Ok(())
    }
}

fn status_icon(running: bool) -> &'static str {
    if running {
        "[ok]"
    } else {
        "[DOWN]"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_l1() -> L1Health {
        L1Health {
            container_name: "kup-test-anvil".to_string(),
            running: true,
            chain_id: Some(11155111),
            expected_chain_id: 11155111,
            block_number: Some(100),
        }
    }

    fn healthy_node() -> NodeHealth {
        NodeHealth {
            role: "sequencer".to_string(),
            label: "sequencer".to_string(),
            execution: ExecutionHealth {
                container_name: "kup-test-op-reth".to_string(),
                running: true,
                chain_id: Some(42069),
                expected_chain_id: 42069,
                block_number: Some(50),
            },
            consensus: ConsensusHealth {
                container_name: "kup-test-kona-node".to_string(),
                running: true,
                unsafe_l2: Some(50),
                safe_l2: Some(40),
                finalized_l2: Some(30),
            },
        }
    }

    fn healthy_services() -> Vec<ServiceHealth> {
        vec![
            ServiceHealth {
                name: "op-batcher".to_string(),
                container_name: "kup-test-op-batcher".to_string(),
                running: true,
            },
            ServiceHealth {
                name: "op-proposer".to_string(),
                container_name: "kup-test-op-proposer".to_string(),
                running: true,
            },
            ServiceHealth {
                name: "op-challenger".to_string(),
                container_name: "kup-test-op-challenger".to_string(),
                running: true,
            },
        ]
    }

    #[test]
    fn test_healthy_report() {
        assert!(compute_healthy(
            &healthy_l1(),
            &[healthy_node()],
            &healthy_services()
        ));
    }

    #[test]
    fn test_unhealthy_stopped_container() {
        let mut services = healthy_services();
        // Stop op-batcher (critical service)
        services[0].running = false;
        assert!(!compute_healthy(&healthy_l1(), &[healthy_node()], &services));
    }

    #[test]
    fn test_healthy_with_op_challenger_down() {
        let mut services = healthy_services();
        // op-challenger is non-critical
        services[2].running = false;
        assert!(compute_healthy(&healthy_l1(), &[healthy_node()], &services));
    }

    #[test]
    fn test_unhealthy_chain_id_mismatch() {
        let l1 = L1Health {
            chain_id: Some(999),
            ..healthy_l1()
        };
        assert!(!compute_healthy(&l1, &[], &[]));
    }

    #[test]
    fn test_unhealthy_zero_blocks() {
        let mut node = healthy_node();
        node.execution.block_number = Some(0);
        assert!(!compute_healthy(&healthy_l1(), &[node], &[]));
    }

    #[test]
    fn test_unhealthy_l1_not_running() {
        let l1 = L1Health {
            running: false,
            chain_id: None,
            block_number: None,
            ..healthy_l1()
        };
        assert!(!compute_healthy(&l1, &[], &[]));
    }

    #[test]
    fn test_chain_id_match_method() {
        let l1 = healthy_l1();
        assert!(l1.chain_id_match());

        let l1_mismatch = L1Health {
            chain_id: Some(999),
            ..healthy_l1()
        };
        assert!(!l1_mismatch.chain_id_match());

        let l1_none = L1Health {
            chain_id: None,
            ..healthy_l1()
        };
        assert!(!l1_none.chain_id_match());
    }
}
