//! Command builder for op-proposer.

/// Builder for op-proposer commands.
#[derive(Debug, Clone)]
pub struct OpProposerCmdBuilder {
    l1_eth_rpc: String,
    rollup_rpc: String,
    private_key: String,
    game_factory_address: String,
    game_type: u8,
    proposal_interval: String,
    rpc_addr: String,
    rpc_port: u16,
    metrics_enabled: bool,
    metrics_addr: String,
    metrics_port: u16,
    extra_args: Vec<String>,
}

impl OpProposerCmdBuilder {
    /// Create a new op-proposer command builder.
    pub fn new(
        l1_eth_rpc: impl Into<String>,
        rollup_rpc: impl Into<String>,
        private_key: impl Into<String>,
        game_factory_address: impl Into<String>,
    ) -> Self {
        Self {
            l1_eth_rpc: l1_eth_rpc.into(),
            rollup_rpc: rollup_rpc.into(),
            private_key: private_key.into(),
            game_factory_address: game_factory_address.into(),
            game_type: 254, // Permissioned game type
            proposal_interval: "12s".to_string(),
            rpc_addr: "0.0.0.0".to_string(),
            rpc_port: 8560,
            metrics_enabled: true,
            metrics_addr: "0.0.0.0".to_string(),
            metrics_port: 7302,
            extra_args: Vec::new(),
        }
    }

    /// Set the game type.
    pub fn game_type(mut self, game_type: u8) -> Self {
        self.game_type = game_type;
        self
    }

    /// Set the proposal interval.
    pub fn proposal_interval(mut self, interval: impl Into<String>) -> Self {
        self.proposal_interval = interval.into();
        self
    }

    /// Set the RPC server address.
    pub fn rpc_addr(mut self, addr: impl Into<String>) -> Self {
        self.rpc_addr = addr.into();
        self
    }

    /// Set the RPC server port.
    pub fn rpc_port(mut self, port: u16) -> Self {
        self.rpc_port = port;
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
            "op-proposer".to_string(),
            "--l1-eth-rpc".to_string(),
            self.l1_eth_rpc,
            "--rollup-rpc".to_string(),
            self.rollup_rpc,
            "--private-key".to_string(),
            self.private_key,
            "--game-factory-address".to_string(),
            self.game_factory_address,
            "--game-type".to_string(),
            self.game_type.to_string(),
            "--proposal-interval".to_string(),
            self.proposal_interval,
            // RPC
            "--rpc.addr".to_string(),
            self.rpc_addr,
            "--rpc.port".to_string(),
            self.rpc_port.to_string(),
        ];

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
    fn test_op_proposer_cmd_builder() {
        let cmd = OpProposerCmdBuilder::new(
            "http://localhost:8545",
            "http://localhost:7545",
            "0xdeadbeef",
            "0x1234567890abcdef",
        )
        .rpc_port(8560)
        .build();

        assert!(cmd.contains(&"op-proposer".to_string()));
        assert!(cmd.contains(&"--l1-eth-rpc".to_string()));
        assert!(cmd.contains(&"--game-factory-address".to_string()));
    }
}

