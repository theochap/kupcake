//! Network status reporting for deployed Kupcake networks.

use std::fmt;

use anyhow::{Context, Result};
use bollard::Docker;

use crate::{Deployer, docker::ContainerState};

/// Status of a single service (container).
pub struct ServiceStatus {
    /// Human-readable label.
    pub label: String,
    /// Docker container name.
    pub container_name: String,
    /// Current container state.
    pub state: ContainerState,
}

/// Status of an L2 node (op-reth + kona-node pair).
pub struct NodeStatus {
    /// Role (sequencer or validator).
    pub role: String,
    /// Human-readable label (e.g., "sequencer", "validator-1").
    pub label: String,
    /// Execution client status.
    pub execution: ServiceStatus,
    /// Consensus client status.
    pub consensus: ServiceStatus,
    /// Op-conductor status (if present).
    pub conductor: Option<ServiceStatus>,
}

/// Status of the entire network.
pub struct NetworkStatus {
    /// Network name (Docker network prefix).
    pub network_name: String,
    /// L1 (Anvil) status.
    pub l1: ServiceStatus,
    /// L2 node statuses.
    pub nodes: Vec<NodeStatus>,
    /// Infrastructure service statuses (batcher, proposer, challenger).
    pub services: Vec<ServiceStatus>,
}

/// Get the state of a container via Docker inspect.
async fn container_state(docker: &Docker, name: &str) -> ContainerState {
    match docker.inspect_container(name, None).await {
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

/// Query the status of all services in a deployed network.
pub async fn network_status(deployer: &Deployer) -> Result<NetworkStatus> {
    let docker =
        Docker::connect_with_local_defaults().context("Failed to connect to Docker daemon")?;

    let network_name = deployer
        .docker
        .net_name
        .strip_suffix("-network")
        .unwrap_or(&deployer.docker.net_name)
        .to_string();

    // L1 status
    let l1 = ServiceStatus {
        label: "anvil".to_string(),
        container_name: deployer.anvil.container_name.clone(),
        state: container_state(&docker, &deployer.anvil.container_name).await,
    };

    // L2 nodes
    let mut nodes = Vec::new();

    for (i, seq) in deployer.l2_stack.sequencers.iter().enumerate() {
        let label = if i == 0 {
            "sequencer".to_string()
        } else {
            format!("sequencer-{}", i)
        };

        let conductor = if let Some(ref cond) = seq.op_conductor {
            Some(ServiceStatus {
                label: "op-conductor".to_string(),
                container_name: cond.container_name.clone(),
                state: container_state(&docker, &cond.container_name).await,
            })
        } else {
            None
        };

        nodes.push(NodeStatus {
            role: "sequencer".to_string(),
            label: label.clone(),
            execution: ServiceStatus {
                label: "op-reth".to_string(),
                container_name: seq.op_reth.container_name.clone(),
                state: container_state(&docker, &seq.op_reth.container_name).await,
            },
            consensus: ServiceStatus {
                label: "kona-node".to_string(),
                container_name: seq.kona_node.container_name.clone(),
                state: container_state(&docker, &seq.kona_node.container_name).await,
            },
            conductor,
        });
    }

    for (i, val) in deployer.l2_stack.validators.iter().enumerate() {
        let label = format!("validator-{}", i + 1);
        nodes.push(NodeStatus {
            role: "validator".to_string(),
            label: label.clone(),
            execution: ServiceStatus {
                label: "op-reth".to_string(),
                container_name: val.op_reth.container_name.clone(),
                state: container_state(&docker, &val.op_reth.container_name).await,
            },
            consensus: ServiceStatus {
                label: "kona-node".to_string(),
                container_name: val.kona_node.container_name.clone(),
                state: container_state(&docker, &val.kona_node.container_name).await,
            },
            conductor: None,
        });
    }

    // Infrastructure services
    let mut services = vec![ServiceStatus {
        label: "op-batcher".to_string(),
        container_name: deployer.l2_stack.op_batcher.container_name.clone(),
        state: container_state(&docker, &deployer.l2_stack.op_batcher.container_name).await,
    }];

    if let Some(ref proposer) = deployer.l2_stack.op_proposer {
        services.push(ServiceStatus {
            label: "op-proposer".to_string(),
            container_name: proposer.container_name.clone(),
            state: container_state(&docker, &proposer.container_name).await,
        });
    }

    if let Some(ref challenger) = deployer.l2_stack.op_challenger {
        services.push(ServiceStatus {
            label: "op-challenger".to_string(),
            container_name: challenger.container_name.clone(),
            state: container_state(&docker, &challenger.container_name).await,
        });
    }

    Ok(NetworkStatus {
        network_name,
        l1,
        nodes,
        services,
    })
}

// -- Display implementations --

fn state_icon(state: ContainerState) -> &'static str {
    match state {
        ContainerState::Running => "[ok]",
        ContainerState::Paused => "[PAUSED]",
        ContainerState::Stopped => "[STOPPED]",
        ContainerState::Restarting => "[RESTARTING]",
        ContainerState::NotFound => "[NOT FOUND]",
    }
}

impl fmt::Display for NetworkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Network: {}", self.network_name)?;
        writeln!(f)?;

        writeln!(f, "=== L1 ===")?;
        writeln!(
            f,
            "  {} {} ({})",
            state_icon(self.l1.state),
            self.l1.label,
            self.l1.container_name
        )?;
        writeln!(f)?;

        writeln!(f, "=== L2 Nodes ===")?;
        for node in &self.nodes {
            writeln!(f, "  [{}] ({})", node.label, node.role)?;
            writeln!(
                f,
                "    {} {} ({})",
                state_icon(node.execution.state),
                node.execution.label,
                node.execution.container_name
            )?;
            writeln!(
                f,
                "    {} {} ({})",
                state_icon(node.consensus.state),
                node.consensus.label,
                node.consensus.container_name
            )?;
            if let Some(ref cond) = node.conductor {
                writeln!(
                    f,
                    "    {} {} ({})",
                    state_icon(cond.state),
                    cond.label,
                    cond.container_name
                )?;
            }
        }
        writeln!(f)?;

        writeln!(f, "=== Services ===")?;
        for svc in &self.services {
            writeln!(
                f,
                "  {} {} ({})",
                state_icon(svc.state),
                svc.label,
                svc.container_name
            )?;
        }

        Ok(())
    }
}
