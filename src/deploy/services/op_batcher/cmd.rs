//! Command builder for op-batcher.

/// Builder for op-batcher commands.
#[derive(Debug, Clone)]
pub struct OpBatcherCmdBuilder {
    l1_eth_rpc: String,
    l2_eth_rpc: String,
    rollup_rpc: String,
    private_key: String,
    rpc_addr: String,
    rpc_port: u16,
    rpc_enable_admin: bool,
    metrics_enabled: bool,
    metrics_addr: String,
    metrics_port: u16,
    data_availability_type: String,
    max_l1_tx_size_bytes: Option<u64>,
    target_num_frames: Option<u64>,
    sub_safety_margin: Option<u64>,
    poll_interval: Option<String>,
    extra_args: Vec<String>,
}

impl OpBatcherCmdBuilder {
    /// Create a new op-batcher command builder.
    pub fn new(
        l1_eth_rpc: impl Into<String>,
        l2_eth_rpc: impl Into<String>,
        rollup_rpc: impl Into<String>,
        private_key: impl Into<String>,
    ) -> Self {
        Self {
            l1_eth_rpc: l1_eth_rpc.into(),
            l2_eth_rpc: l2_eth_rpc.into(),
            rollup_rpc: rollup_rpc.into(),
            private_key: private_key.into(),
            rpc_addr: "0.0.0.0".to_string(),
            rpc_port: 8548,
            rpc_enable_admin: true,
            metrics_enabled: true,
            metrics_addr: "0.0.0.0".to_string(),
            metrics_port: 7301,
            data_availability_type: "blobs".to_string(),
            max_l1_tx_size_bytes: None,
            target_num_frames: None,
            sub_safety_margin: None,
            poll_interval: None,
            extra_args: Vec::new(),
        }
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

    /// Enable or disable admin RPC.
    pub fn rpc_enable_admin(mut self, enabled: bool) -> Self {
        self.rpc_enable_admin = enabled;
        self
    }

    /// Configure metrics.
    pub fn metrics(mut self, enabled: bool, addr: impl Into<String>, port: u16) -> Self {
        self.metrics_enabled = enabled;
        self.metrics_addr = addr.into();
        self.metrics_port = port;
        self
    }

    /// Set the data availability type (blobs, calldata).
    pub fn data_availability_type(mut self, da_type: impl Into<String>) -> Self {
        self.data_availability_type = da_type.into();
        self
    }

    /// Set the max L1 transaction size in bytes.
    pub fn max_l1_tx_size_bytes(mut self, size: u64) -> Self {
        self.max_l1_tx_size_bytes = Some(size);
        self
    }

    /// Set the target number of frames per channel.
    pub fn target_num_frames(mut self, frames: u64) -> Self {
        self.target_num_frames = Some(frames);
        self
    }

    /// Set the sub-safety margin.
    pub fn sub_safety_margin(mut self, margin: u64) -> Self {
        self.sub_safety_margin = Some(margin);
        self
    }

    /// Set the poll interval.
    pub fn poll_interval(mut self, interval: impl Into<String>) -> Self {
        self.poll_interval = Some(interval.into());
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
            "op-batcher".to_string(),
            "--l1-eth-rpc".to_string(),
            self.l1_eth_rpc,
            "--l2-eth-rpc".to_string(),
            self.l2_eth_rpc,
            "--rollup-rpc".to_string(),
            self.rollup_rpc,
            "--private-key".to_string(),
            self.private_key,
            // RPC
            "--rpc.addr".to_string(),
            self.rpc_addr,
            "--rpc.port".to_string(),
            self.rpc_port.to_string(),
        ];

        if self.rpc_enable_admin {
            cmd.push("--rpc.enable-admin".to_string());
        }

        // Metrics
        if self.metrics_enabled {
            cmd.push("--metrics.enabled".to_string());
            cmd.push("--metrics.addr".to_string());
            cmd.push(self.metrics_addr);
            cmd.push("--metrics.port".to_string());
            cmd.push(self.metrics_port.to_string());
        }

        // Batcher configuration
        cmd.push("--data-availability-type".to_string());
        cmd.push(self.data_availability_type);

        // For local devnet, disable DA throttling
        cmd.push("--throttle.unsafe-da-bytes-lower-threshold".to_string());
        cmd.push("0".to_string());

        if let Some(size) = self.max_l1_tx_size_bytes {
            cmd.push("--max-l1-tx-size-bytes".to_string());
            cmd.push(size.to_string());
        }

        if let Some(frames) = self.target_num_frames {
            cmd.push("--target-num-frames".to_string());
            cmd.push(frames.to_string());
        }

        if let Some(margin) = self.sub_safety_margin {
            cmd.push("--sub-safety-margin".to_string());
            cmd.push(margin.to_string());
        }

        if let Some(interval) = self.poll_interval {
            cmd.push("--poll-interval".to_string());
            cmd.push(interval);
        }

        cmd.extend(self.extra_args);

        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_batcher_cmd_builder() {
        let cmd = OpBatcherCmdBuilder::new(
            "http://localhost:8545",
            "http://localhost:9545",
            "http://localhost:7545",
            "0xdeadbeef",
        )
        .rpc_port(8548)
        .build();

        assert!(cmd.contains(&"op-batcher".to_string()));
        assert!(cmd.contains(&"--l1-eth-rpc".to_string()));
    }
}
