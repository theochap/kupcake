//! Command builder for kona-node consensus client.

use std::path::Path;

use alloy_core::primitives::Bytes;

/// Builder for kona-node consensus client commands.
#[derive(Debug, Clone)]
pub struct KonaNodeCmdBuilder {
    mode: String,
    l1_rpc: String,
    l1_beacon: String,
    l1_slot_duration: u64,
    l2_rpc: String,
    rollup_cfg: String,
    jwt_secret: String,
    rpc_port: u16,
    metrics_enabled: bool,
    metrics_port: u16,
    no_discovery: bool,
    unsafe_block_signer_key: Option<Bytes>,
    extra_args: Vec<String>,
}

impl KonaNodeCmdBuilder {
    /// Create a new kona-node command builder with required configuration.
    pub fn new(
        l1_rpc: impl Into<String>,
        l2_rpc: impl Into<String>,
        rollup_cfg: impl AsRef<Path>,
        jwt_secret: impl AsRef<Path>,
    ) -> Self {
        Self {
            mode: "validator".to_string(),
            l1_rpc: l1_rpc.into(),
            l1_beacon: String::new(),
            l1_slot_duration: 12,
            l2_rpc: l2_rpc.into(),
            rollup_cfg: rollup_cfg.as_ref().display().to_string(),
            jwt_secret: jwt_secret.as_ref().display().to_string(),
            rpc_port: 7545,
            metrics_enabled: true,
            metrics_port: 7300,
            no_discovery: true,
            unsafe_block_signer_key: None,
            extra_args: Vec::new(),
        }
    }

    /// Set the operating mode (sequencer, follower, etc.).
    pub fn mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = mode.into();
        self
    }

    /// Set the unsafe block signer key.
    pub fn unsafe_block_signer_key(mut self, key: Bytes) -> Self {
        self.unsafe_block_signer_key = Some(key.into());
        self
    }

    /// Set the L1 beacon API URL.
    pub fn l1_beacon(mut self, url: impl Into<String>) -> Self {
        self.l1_beacon = url.into();
        self
    }

    /// Set the L1 slot duration in seconds.
    pub fn l1_slot_duration(mut self, duration: u64) -> Self {
        self.l1_slot_duration = duration;
        self
    }

    /// Set the RPC server port.
    pub fn rpc_port(mut self, port: u16) -> Self {
        self.rpc_port = port;
        self
    }

    /// Enable or disable metrics.
    pub fn metrics(mut self, enabled: bool, port: u16) -> Self {
        self.metrics_enabled = enabled;
        self.metrics_port = port;
        self
    }

    /// Enable or disable P2P discovery.
    pub fn discovery(mut self, enabled: bool) -> Self {
        self.no_discovery = !enabled;
        self
    }

    /// Add extra arguments.
    pub fn extra_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.extra_args.extend(args.into_iter().map(|s| s.into()));
        self
    }

    /// Build the command as a vector of strings.
    pub fn build(self) -> Vec<String> {
        let mut cmd = Vec::new();

        // Metrics flags come before the subcommand
        if self.metrics_enabled {
            cmd.push("--metrics.enabled".to_string());
            cmd.push("--metrics.port".to_string());
            cmd.push(self.metrics_port.to_string());
        }

        // Subcommand
        cmd.push("node".to_string());

        if let Some(unsafe_block_signer_key) = self.unsafe_block_signer_key {
            cmd.push("--p2p.sequencer.key".to_string());
            cmd.push(hex::encode(unsafe_block_signer_key));
        }

        cmd.push("--mode".to_string());
        cmd.push(self.mode);

        // L1 configuration
        cmd.push("--l1".to_string());
        cmd.push(self.l1_rpc.clone());

        cmd.push("--l1-beacon".to_string());
        if self.l1_beacon.is_empty() {
            cmd.push(self.l1_rpc);
        } else {
            cmd.push(self.l1_beacon);
        }

        cmd.push("--l1.slot-duration".to_string());
        cmd.push(self.l1_slot_duration.to_string());

        // L2 configuration
        cmd.push("--l2".to_string());
        cmd.push(self.l2_rpc);

        // Rollup configuration
        cmd.push("--rollup-cfg".to_string());
        cmd.push(self.rollup_cfg);

        cmd.push("--l2.jwt-secret".to_string());
        cmd.push(self.jwt_secret);

        // P2P
        if self.no_discovery {
            cmd.push("--p2p.no-discovery".to_string());
        }

        // RPC
        cmd.push("--rpc.port".to_string());
        cmd.push(self.rpc_port.to_string());

        cmd.extend(self.extra_args);

        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kona_node_cmd_builder() {
        let cmd = KonaNodeCmdBuilder::new(
            "http://localhost:8545",
            "http://localhost:9551",
            "/data/rollup.json",
            "/data/jwt.hex",
        )
        .rpc_port(7545)
        .metrics(true, 7300)
        .build();

        assert!(cmd.contains(&"node".to_string()));
        assert!(cmd.contains(&"--mode".to_string()));
        assert!(cmd.contains(&"sequencer".to_string()));
    }
}
