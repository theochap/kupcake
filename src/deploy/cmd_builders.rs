//! Command builders for Docker service commands.
//!
//! Each builder generates the `cmd` argument for a Docker container configuration.

use std::path::Path;

/// Builder for op-reth execution client commands.
#[derive(Debug, Clone)]
pub struct OpRethCmdBuilder {
    chain_path: String,
    datadir: String,
    http_addr: String,
    http_port: u16,
    http_api: String,
    ws_addr: String,
    ws_port: u16,
    ws_api: String,
    authrpc_addr: String,
    authrpc_port: u16,
    authrpc_jwtsecret: String,
    metrics: Option<String>,
    discovery_disabled: bool,
    sequencer_http: Option<String>,
    log_format: String,
    extra_args: Vec<String>,
}

impl OpRethCmdBuilder {
    /// Create a new op-reth command builder with required paths.
    pub fn new(chain_path: impl AsRef<Path>, datadir: impl AsRef<Path>) -> Self {
        Self {
            chain_path: chain_path.as_ref().display().to_string(),
            datadir: datadir.as_ref().display().to_string(),
            http_addr: "0.0.0.0".to_string(),
            http_port: 8545,
            http_api: "eth,net,web3,debug,trace,txpool".to_string(),
            ws_addr: "0.0.0.0".to_string(),
            ws_port: 8546,
            ws_api: "eth,net,web3,debug,trace,txpool".to_string(),
            authrpc_addr: "0.0.0.0".to_string(),
            authrpc_port: 8551,
            authrpc_jwtsecret: String::new(),
            metrics: None,
            discovery_disabled: true,
            sequencer_http: None,
            log_format: "terminal".to_string(),
            extra_args: Vec::new(),
        }
    }

    /// Set the HTTP RPC address.
    pub fn http_addr(mut self, addr: impl Into<String>) -> Self {
        self.http_addr = addr.into();
        self
    }

    /// Set the HTTP RPC port.
    pub fn http_port(mut self, port: u16) -> Self {
        self.http_port = port;
        self
    }

    /// Set the HTTP API methods.
    pub fn http_api(mut self, api: impl Into<String>) -> Self {
        self.http_api = api.into();
        self
    }

    /// Set the WebSocket RPC address.
    pub fn ws_addr(mut self, addr: impl Into<String>) -> Self {
        self.ws_addr = addr.into();
        self
    }

    /// Set the WebSocket RPC port.
    pub fn ws_port(mut self, port: u16) -> Self {
        self.ws_port = port;
        self
    }

    /// Set the WebSocket API methods.
    pub fn ws_api(mut self, api: impl Into<String>) -> Self {
        self.ws_api = api.into();
        self
    }

    /// Set the Auth RPC address.
    pub fn authrpc_addr(mut self, addr: impl Into<String>) -> Self {
        self.authrpc_addr = addr.into();
        self
    }

    /// Set the Auth RPC port.
    pub fn authrpc_port(mut self, port: u16) -> Self {
        self.authrpc_port = port;
        self
    }

    /// Set the JWT secret path.
    pub fn authrpc_jwtsecret(mut self, path: impl AsRef<Path>) -> Self {
        self.authrpc_jwtsecret = path.as_ref().display().to_string();
        self
    }

    /// Enable metrics on the specified address and port.
    pub fn metrics(mut self, addr: impl Into<String>, port: u16) -> Self {
        self.metrics = Some(format!("{}:{}", addr.into(), port));
        self
    }

    /// Enable or disable P2P discovery.
    pub fn discovery(mut self, enabled: bool) -> Self {
        self.discovery_disabled = !enabled;
        self
    }

    /// Set the sequencer HTTP URL.
    pub fn sequencer_http(mut self, url: impl Into<String>) -> Self {
        self.sequencer_http = Some(url.into());
        self
    }

    /// Set the log format.
    pub fn log_format(mut self, format: impl Into<String>) -> Self {
        self.log_format = format.into();
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
            "node".to_string(),
            "--chain".to_string(),
            self.chain_path,
            "--datadir".to_string(),
            self.datadir,
            // HTTP RPC
            "--http".to_string(),
            "--http.addr".to_string(),
            self.http_addr,
            "--http.port".to_string(),
            self.http_port.to_string(),
            "--http.api".to_string(),
            self.http_api,
            // WebSocket RPC
            "--ws".to_string(),
            "--ws.addr".to_string(),
            self.ws_addr,
            "--ws.port".to_string(),
            self.ws_port.to_string(),
            "--ws.api".to_string(),
            self.ws_api,
            // Auth RPC
            "--authrpc.addr".to_string(),
            self.authrpc_addr,
            "--authrpc.port".to_string(),
            self.authrpc_port.to_string(),
            "--authrpc.jwtsecret".to_string(),
            self.authrpc_jwtsecret,
        ];

        if let Some(metrics) = self.metrics {
            cmd.push("--metrics".to_string());
            cmd.push(metrics);
        }

        if self.discovery_disabled {
            cmd.push("--disable-discovery".to_string());
        }

        if let Some(sequencer_http) = self.sequencer_http {
            cmd.push("--rollup.sequencer-http".to_string());
            cmd.push(sequencer_http);
        }

        cmd.push("--log.stdout.format".to_string());
        cmd.push(self.log_format);

        cmd.extend(self.extra_args);

        cmd
    }
}

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
            mode: "sequencer".to_string(),
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
            extra_args: Vec::new(),
        }
    }

    /// Set the operating mode (sequencer, follower, etc.).
    pub fn mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = mode.into();
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
    rpc_addr: String,
    rpc_port: u16,
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
            rpc_addr: "0.0.0.0".to_string(),
            rpc_port: 8561,
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

        // RPC
        cmd.push("--rpc.addr".to_string());
        cmd.push(self.rpc_addr);
        cmd.push("--rpc.port".to_string());
        cmd.push(self.rpc_port.to_string());

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

/// Builder for Anvil commands.
#[derive(Debug, Clone)]
pub struct AnvilCmdBuilder {
    host: String,
    port: u16,
    chain_id: u64,
    fork_url: Option<String>,
    state_path: Option<String>,
    config_out: Option<String>,
    extra_args: Vec<String>,
}

impl AnvilCmdBuilder {
    /// Create a new Anvil command builder.
    pub fn new(chain_id: u64) -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8545,
            chain_id,
            fork_url: None,
            state_path: None,
            config_out: None,
            extra_args: Vec::new(),
        }
    }

    /// Set the host address.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Set the port.
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the fork URL.
    pub fn fork_url(mut self, url: impl Into<String>) -> Self {
        self.fork_url = Some(url.into());
        self
    }

    /// Set the state persistence path.
    pub fn state_path(mut self, path: impl AsRef<Path>) -> Self {
        self.state_path = Some(path.as_ref().display().to_string());
        self
    }

    /// Set the config output path.
    pub fn config_out(mut self, path: impl AsRef<Path>) -> Self {
        self.config_out = Some(path.as_ref().display().to_string());
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
            "--host".to_string(),
            self.host,
            "--port".to_string(),
            self.port.to_string(),
            "--chain-id".to_string(),
            self.chain_id.to_string(),
        ];

        if let Some(fork_url) = self.fork_url {
            cmd.push("--fork-url".to_string());
            cmd.push(fork_url);
        }

        if let Some(state_path) = self.state_path {
            cmd.push("--state".to_string());
            cmd.push(state_path);
        }

        if let Some(config_out) = self.config_out {
            cmd.push("--config-out".to_string());
            cmd.push(config_out);
        }

        cmd.extend(self.extra_args);

        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_reth_cmd_builder() {
        let cmd = OpRethCmdBuilder::new("/data/genesis.json", "/data/reth-data")
            .http_port(9545)
            .ws_port(9546)
            .authrpc_port(9551)
            .authrpc_jwtsecret("/data/jwt.hex")
            .metrics("0.0.0.0", 9001)
            .sequencer_http("http://localhost:9545")
            .build();

        assert!(cmd.contains(&"node".to_string()));
        assert!(cmd.contains(&"--chain".to_string()));
        assert!(cmd.contains(&"/data/genesis.json".to_string()));
        assert!(cmd.contains(&"--http.port".to_string()));
        assert!(cmd.contains(&"9545".to_string()));
    }

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

    #[test]
    fn test_anvil_cmd_builder() {
        let cmd = AnvilCmdBuilder::new(11155111)
            .port(8545)
            .fork_url("https://ethereum-sepolia-rpc.publicnode.com")
            .state_path("/data/anvil-state.json")
            .build();

        assert!(cmd.contains(&"--chain-id".to_string()));
        assert!(cmd.contains(&"11155111".to_string()));
        assert!(cmd.contains(&"--fork-url".to_string()));
    }
}
