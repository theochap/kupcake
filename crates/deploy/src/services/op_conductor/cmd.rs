//! Command builder for op-conductor.

/// Builder for op-conductor commands.
#[derive(Debug, Clone)]
pub struct OpConductorCmdBuilder {
    /// Node RPC endpoint for the op-node managed by this conductor.
    node_rpc: String,
    /// Execution RPC endpoint for the execution client.
    execution_rpc: String,
    /// Unique server ID for this Raft node.
    raft_server_id: String,
    /// Directory for storing Raft data.
    raft_storage_dir: String,
    /// Path to rollup configuration file.
    rollup_config: String,
    /// Whether to bootstrap a new Raft cluster.
    raft_bootstrap: bool,
    /// Consensus listen/advertise address (must be resolvable by other nodes).
    consensus_addr: String,
    /// Consensus port.
    consensus_port: u16,
    /// RPC listen address.
    rpc_addr: String,
    /// RPC port.
    rpc_port: u16,
    /// Health check interval.
    healthcheck_interval: String,
    /// Unsafe interval - interval allowed between unsafe head and now measured in seconds.
    healthcheck_unsafe_interval: String,
    /// Minimum number of peers required to be considered healthy.
    healthcheck_min_peer_count: String,
    /// Paused mode - start with sequencer paused.
    paused: bool,
    /// Log level.
    log_level: String,
    /// Extra arguments to pass to op-conductor.
    extra_args: Vec<String>,
}

impl OpConductorCmdBuilder {
    /// Create a new op-conductor command builder.
    pub fn new(
        node_rpc: impl Into<String>,
        execution_rpc: impl Into<String>,
        raft_server_id: impl Into<String>,
        raft_storage_dir: impl Into<String>,
        rollup_config: impl Into<String>,
    ) -> Self {
        Self {
            node_rpc: node_rpc.into(),
            execution_rpc: execution_rpc.into(),
            raft_server_id: raft_server_id.into(),
            raft_storage_dir: raft_storage_dir.into(),
            rollup_config: rollup_config.into(),
            raft_bootstrap: false,
            consensus_addr: "0.0.0.0".to_string(),
            consensus_port: 50050,
            rpc_addr: "0.0.0.0".to_string(),
            rpc_port: 8547,
            healthcheck_interval: "5".to_string(),
            healthcheck_unsafe_interval: "600".to_string(),
            healthcheck_min_peer_count: "1".to_string(),
            paused: false,
            log_level: "DEBUG".to_string(),
            extra_args: Vec::new(),
        }
    }

    /// Set whether to bootstrap a new Raft cluster.
    ///
    /// Set to true for the first/leader node in the cluster.
    pub fn raft_bootstrap(mut self, bootstrap: bool) -> Self {
        self.raft_bootstrap = bootstrap;
        self
    }

    /// Set the consensus listen address.
    pub fn consensus_addr(mut self, addr: impl Into<String>) -> Self {
        self.consensus_addr = addr.into();
        self
    }

    /// Set the consensus port.
    pub fn consensus_port(mut self, port: u16) -> Self {
        self.consensus_port = port;
        self
    }

    /// Set the RPC listen address.
    pub fn rpc_addr(mut self, addr: impl Into<String>) -> Self {
        self.rpc_addr = addr.into();
        self
    }

    /// Set the RPC port.
    pub fn rpc_port(mut self, port: u16) -> Self {
        self.rpc_port = port;
        self
    }

    /// Set the health check interval.
    pub fn healthcheck_interval(mut self, interval: impl Into<String>) -> Self {
        self.healthcheck_interval = interval.into();
        self
    }

    /// Set the unsafe interval.
    pub fn healthcheck_unsafe_interval(mut self, interval: impl Into<String>) -> Self {
        self.healthcheck_unsafe_interval = interval.into();
        self
    }

    /// Set the minimum peer count.
    pub fn healthcheck_min_peer_count(mut self, count: impl Into<String>) -> Self {
        self.healthcheck_min_peer_count = count.into();
        self
    }

    /// Set whether to start in paused mode.
    pub fn paused(mut self, paused: bool) -> Self {
        self.paused = paused;
        self
    }

    /// Set the log level.
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }

    /// Add extra arguments.
    pub fn extra_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.extra_args.extend(args.into_iter().map(|s| s.into()));
        self
    }

    /// Build the command as a vector of strings.
    pub fn build(self) -> Vec<String> {
        let mut cmd = vec![
            "op-conductor".to_string(),
            // Node RPC
            "--node.rpc".to_string(),
            self.node_rpc,
            // Execution RPC
            "--execution.rpc".to_string(),
            self.execution_rpc,
            // Raft configuration
            "--raft.server.id".to_string(),
            self.raft_server_id,
            "--raft.storage.dir".to_string(),
            self.raft_storage_dir,
            // Rollup config
            "--rollup.config".to_string(),
            self.rollup_config,
            // Consensus network
            "--consensus.addr".to_string(),
            self.consensus_addr,
            "--consensus.port".to_string(),
            self.consensus_port.to_string(),
            // RPC
            "--rpc.addr".to_string(),
            self.rpc_addr,
            "--rpc.port".to_string(),
            self.rpc_port.to_string(),
            // Health check
            "--healthcheck.interval".to_string(),
            self.healthcheck_interval,
            "--healthcheck.unsafe-interval".to_string(),
            self.healthcheck_unsafe_interval,
            "--healthcheck.min-peer-count".to_string(),
            self.healthcheck_min_peer_count,
            // Log level
            "--log.level".to_string(),
            self.log_level,
        ];

        if self.raft_bootstrap {
            cmd.push("--raft.bootstrap".to_string());
        }

        if self.paused {
            cmd.push("--paused".to_string());
        }

        cmd.extend(self.extra_args);

        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_conductor_cmd_builder() {
        let cmd = OpConductorCmdBuilder::new(
            "http://localhost:7545",
            "http://localhost:8545",
            "sequencer-0",
            "/data/raft",
            "/data/rollup.json",
        )
        .raft_bootstrap(true)
        .rpc_port(8547)
        .build();

        assert!(cmd.contains(&"op-conductor".to_string()));
        assert!(cmd.contains(&"--node.rpc".to_string()));
        assert!(cmd.contains(&"--raft.bootstrap".to_string()));
    }

    #[test]
    fn test_op_conductor_cmd_builder_no_bootstrap() {
        let cmd = OpConductorCmdBuilder::new(
            "http://localhost:7545",
            "http://localhost:8545",
            "sequencer-1",
            "/data/raft",
            "/data/rollup.json",
        )
        .rpc_port(8547)
        .build();

        assert!(!cmd.contains(&"--raft.bootstrap".to_string()));
    }
}
