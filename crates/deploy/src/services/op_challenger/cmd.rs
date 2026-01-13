//! Command builder for op-challenger.

/// Builder for op-challenger commands.
#[derive(Debug, Clone)]
pub struct OpChallengerCmdBuilder {
    l1_eth_rpc: String,
    l2_eth_rpc: String,
    rollup_rpc: String,
    private_key: String,
    game_factory_address: String,
    trace_type: String,
    game_allowlist: Vec<u8>,
    metrics_enabled: bool,
    metrics_addr: String,
    metrics_port: u16,
    extra_args: Vec<String>,
}

impl OpChallengerCmdBuilder {
    /// Create a new op-challenger command builder.
    pub fn new(
        l1_eth_rpc: impl Into<String>,
        l2_eth_rpc: impl Into<String>,
        rollup_rpc: impl Into<String>,
        private_key: impl Into<String>,
        game_factory_address: impl Into<String>,
    ) -> Self {
        Self {
            l1_eth_rpc: l1_eth_rpc.into(),
            l2_eth_rpc: l2_eth_rpc.into(),
            rollup_rpc: rollup_rpc.into(),
            private_key: private_key.into(),
            game_factory_address: game_factory_address.into(),
            trace_type: "permissioned".to_string(),
            game_allowlist: vec![254], // Permissioned game type
            metrics_enabled: true,
            metrics_addr: "0.0.0.0".to_string(),
            metrics_port: 7303,
            extra_args: Vec::new(),
        }
    }

    /// Set the trace type.
    pub fn trace_type(mut self, trace_type: impl Into<String>) -> Self {
        self.trace_type = trace_type.into();
        self
    }

    /// Set the game allowlist.
    pub fn game_allowlist(mut self, games: impl IntoIterator<Item = u8>) -> Self {
        self.game_allowlist = games.into_iter().collect();
        self
    }

    /// Configure metrics.
    pub fn metrics(mut self, enabled: bool, addr: impl Into<String>, port: u16) -> Self {
        self.metrics_enabled = enabled;
        self.metrics_addr = addr.into();
        self.metrics_port = port;
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
            "op-challenger".to_string(),
            "--l1-eth-rpc".to_string(),
            self.l1_eth_rpc,
            "--l2-eth-rpc".to_string(),
            self.l2_eth_rpc,
            "--rollup-rpc".to_string(),
            self.rollup_rpc,
            "--private-key".to_string(),
            self.private_key,
            "--game-factory-address".to_string(),
            self.game_factory_address,
            "--trace-type".to_string(),
            self.trace_type,
        ];

        // Game allowlist
        for game in self.game_allowlist {
            cmd.push("--game-allowlist".to_string());
            cmd.push(game.to_string());
        }

        // Metrics
        if self.metrics_enabled {
            cmd.push("--metrics.enabled".to_string());
            cmd.push("--metrics.addr".to_string());
            cmd.push(self.metrics_addr);
            cmd.push("--metrics.port".to_string());
            cmd.push(self.metrics_port.to_string());
        }

        cmd.extend(self.extra_args);

        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_challenger_cmd_builder() {
        let cmd = OpChallengerCmdBuilder::new(
            "http://localhost:8545",
            "http://localhost:9545",
            "http://localhost:7545",
            "0xdeadbeef",
            "0x1234567890abcdef",
        )
        .build();

        assert!(cmd.contains(&"op-challenger".to_string()));
        assert!(cmd.contains(&"--l1-eth-rpc".to_string()));
        assert!(cmd.contains(&"--game-factory-address".to_string()));
    }
}

