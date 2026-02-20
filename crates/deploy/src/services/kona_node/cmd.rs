//! Command builder for kona-node consensus client.

use std::path::Path;

use alloy_core::primitives::Bytes;

pub const DEFAULT_P2P_PORT: u16 = 9222;

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
    p2p_ip: String,
    p2p_port: u16,
    metrics_enabled: bool,
    metrics_port: u16,
    no_discovery: bool,
    bootnodes: Vec<String>,
    /// P2P private key (32 bytes hex-encoded)
    p2p_priv_key: Option<String>,
    unsafe_block_signer_key: Option<Bytes>,
    /// Conductor RPC URL (enables conductor control when set)
    conductor_rpc: Option<String>,
    /// Start sequencer in stopped state (for conductor-managed sequencers)
    sequencer_stopped: bool,
    extra_args: Vec<String>,
    /// Path to L1 chain config file (for custom/local L1 chains)
    l1_config_file: Option<String>,
    /// Whether flashblocks support is enabled.
    flashblocks_enabled: bool,
    /// URL of the flashblocks builder (op-rbuilder or sequencer relay).
    flashblocks_builder_url: Option<String>,
    /// Host to bind the flashblocks relay server on.
    flashblocks_host: Option<String>,
    /// Port for the flashblocks relay server.
    flashblocks_port: Option<u16>,
}

impl KonaNodeCmdBuilder {
    /// Create a new kona-node command builder with required configuration.
    pub fn new(
        l1_rpc: impl Into<String>,
        l2_rpc: impl Into<String>,
        p2p_ip: impl Into<String>,
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
            p2p_port: DEFAULT_P2P_PORT,
            metrics_enabled: true,
            metrics_port: 7300,
            no_discovery: false,
            bootnodes: Vec::new(),
            p2p_priv_key: None,
            p2p_ip: p2p_ip.into(),
            unsafe_block_signer_key: None,
            conductor_rpc: None,
            sequencer_stopped: false,
            extra_args: Vec::new(),
            l1_config_file: None,
            flashblocks_enabled: false,
            flashblocks_builder_url: None,
            flashblocks_host: None,
            flashblocks_port: None,
        }
    }

    /// Set the L1 chain config file path (for custom/local L1 chains like Anvil).
    pub fn l1_config_file(mut self, path: impl Into<String>) -> Self {
        self.l1_config_file = Some(path.into());
        self
    }

    /// Set the operating mode (sequencer, follower, etc.).
    pub fn mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = mode.into();
        self
    }

    /// Set the unsafe block signer key.
    pub fn unsafe_block_signer_key(mut self, key: Bytes) -> Self {
        self.unsafe_block_signer_key = Some(key);
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

    /// Set the P2P bootnodes (enode URLs).
    pub fn bootnodes(mut self, bootnodes: Vec<String>) -> Self {
        self.bootnodes = bootnodes;
        self
    }

    /// Set the P2P private key (32 bytes hex-encoded).
    pub fn p2p_priv_key(mut self, key: impl Into<String>) -> Self {
        self.p2p_priv_key = Some(key.into());
        self
    }

    /// Set the conductor RPC URL for conductor-managed sequencers.
    ///
    /// When set, enables conductor control mode (`--conductor.enabled`)
    /// and configures the conductor RPC endpoint.
    pub fn conductor_rpc(mut self, url: impl Into<String>) -> Self {
        self.conductor_rpc = Some(url.into());
        self
    }

    /// Start the sequencer in stopped state (for conductor-managed sequencers).
    ///
    /// When true, the sequencer will not produce blocks until the conductor
    /// activates it. Used with `conductor_rpc` for high-availability setups.
    pub fn sequencer_stopped(mut self, stopped: bool) -> Self {
        self.sequencer_stopped = stopped;
        self
    }

    /// Enable flashblocks support.
    pub fn flashblocks(mut self, enabled: bool) -> Self {
        self.flashblocks_enabled = enabled;
        self
    }

    /// Set the flashblocks builder URL (op-rbuilder WS or sequencer relay WS).
    pub fn flashblocks_builder_url(mut self, url: impl Into<String>) -> Self {
        self.flashblocks_builder_url = Some(url.into());
        self
    }

    /// Configure this node as a flashblocks relay server.
    pub fn flashblocks_relay(mut self, host: impl Into<String>, port: u16) -> Self {
        self.flashblocks_host = Some(host.into());
        self.flashblocks_port = Some(port);
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

        cmd.push("--l1.slot-duration-override".to_string());
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

        // P2P bootnodes
        if !self.bootnodes.is_empty() {
            cmd.push("--p2p.bootnodes".to_string());
            cmd.push(self.bootnodes.join(","));
        }

        // P2P private key
        if let Some(p2p_priv_key) = self.p2p_priv_key {
            cmd.push("--p2p.priv.raw".to_string());
            cmd.push(p2p_priv_key);
        }

        // P2P listen IP
        cmd.push("--p2p.listen.ip".to_string());
        cmd.push(self.p2p_ip.clone());

        cmd.push("--p2p.advertise.ip".to_string());
        cmd.push(self.p2p_ip);

        // P2P port
        cmd.push("--p2p.listen.tcp".to_string());
        cmd.push(self.p2p_port.to_string());

        cmd.push("--p2p.listen.udp".to_string());
        cmd.push(self.p2p_port.to_string());

        // Conductor configuration (for sequencer high-availability)
        if let Some(conductor_rpc) = self.conductor_rpc {
            cmd.push("--conductor.rpc".to_string());
            cmd.push(conductor_rpc);
        }

        // Start sequencer in stopped state (for conductor control)
        if self.sequencer_stopped {
            cmd.push("--sequencer.stopped".to_string());
        }

        // RPC
        cmd.push("--rpc.port".to_string());
        cmd.push(self.rpc_port.to_string());

        // L1 chain config file (for custom/local L1 chains)
        if let Some(l1_config_file) = self.l1_config_file {
            cmd.push("--l1-config-file".to_string());
            cmd.push(l1_config_file);
        }

        // Flashblocks configuration
        if self.flashblocks_enabled {
            cmd.push("--flashblocks".to_string());
            // kona-node defaults --flashblocks-host to "localhost" which is not a valid
            // IpAddr and causes a parse error. Always provide 0.0.0.0 as a safe default.
            let host = self
                .flashblocks_host
                .unwrap_or_else(|| "0.0.0.0".to_string());
            cmd.push("--flashblocks-host".to_string());
            cmd.push(host);
        }
        if let Some(builder_url) = self.flashblocks_builder_url {
            cmd.push("--flashblocks-builder-url".to_string());
            cmd.push(builder_url);
        }
        if let Some(port) = self.flashblocks_port {
            cmd.push("--flashblocks-port".to_string());
            cmd.push(port.to_string());
        }

        cmd.push("-vvvv".to_string());
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
            "0.0.0.0",
            "/data/rollup.json",
            "/data/jwt.hex",
        )
        .rpc_port(7545)
        .metrics(true, 7300)
        .build();

        assert!(cmd.contains(&"node".to_string()));
        assert!(cmd.contains(&"--mode".to_string()));
        assert!(cmd.contains(&"validator".to_string()));
    }

    #[test]
    fn test_flashblocks_flags() {
        let cmd = KonaNodeCmdBuilder::new(
            "http://localhost:8545",
            "http://localhost:9551",
            "0.0.0.0",
            "/data/rollup.json",
            "/data/jwt.hex",
        )
        .flashblocks(true)
        .flashblocks_builder_url("ws://op-rbuilder:1111")
        .flashblocks_relay("0.0.0.0", 1112)
        .build();

        assert!(cmd.contains(&"--flashblocks".to_string()));
        let builder_pos = cmd.iter().position(|s| s == "--flashblocks-builder-url");
        assert!(builder_pos.is_some());
        assert_eq!(cmd[builder_pos.unwrap() + 1], "ws://op-rbuilder:1111");
        let host_pos = cmd.iter().position(|s| s == "--flashblocks-host");
        assert!(host_pos.is_some());
        assert_eq!(cmd[host_pos.unwrap() + 1], "0.0.0.0");
        let port_pos = cmd.iter().position(|s| s == "--flashblocks-port");
        assert!(port_pos.is_some());
        assert_eq!(cmd[port_pos.unwrap() + 1], "1112");
    }

    #[test]
    fn test_flashblocks_absent_by_default() {
        let cmd = KonaNodeCmdBuilder::new(
            "http://localhost:8545",
            "http://localhost:9551",
            "0.0.0.0",
            "/data/rollup.json",
            "/data/jwt.hex",
        )
        .build();
        assert!(
            !cmd.contains(&"--flashblocks".to_string()),
            "Should not contain flashblocks flags when not enabled"
        );
    }
}
