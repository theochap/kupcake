//! Command builder for Anvil.

use std::path::Path;

/// Specifies how Anvil should load initial state.
#[derive(Debug, Clone)]
pub enum AnvilInitMode {
    /// Load genesis state from a JSON file (`--init <path>`).
    /// Used in genesis deployment mode where contracts are embedded in the L1 genesis.
    Init(String),
    /// Load persisted state from a state dump (`--load-state <path>`).
    /// Used to restore previously dumped state or load external state via `--override-state`.
    LoadState(String),
}

/// Builder for Anvil commands.
#[derive(Debug, Clone)]
pub struct AnvilCmdBuilder {
    host: String,
    port: u16,
    chain_id: u64,
    block_time: u64,
    fork_url: Option<String>,
    init_mode: Option<AnvilInitMode>,
    config_out: Option<String>,
    timestamp: Option<u64>,
    fork_block_number: Option<u64>,
    extra_args: Vec<String>,
}

impl AnvilCmdBuilder {
    /// Create a new Anvil command builder.
    pub fn new(chain_id: u64) -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8545,
            chain_id,
            block_time: 12,
            fork_url: None,
            init_mode: None,
            config_out: None,
            timestamp: None,
            fork_block_number: None,
            extra_args: Vec::new(),
        }
    }

    /// Set the block time in seconds.
    pub fn block_time(mut self, block_time: u64) -> Self {
        self.block_time = block_time;
        self
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

    /// Set the init mode (how Anvil loads initial state).
    pub fn init_mode(mut self, mode: AnvilInitMode) -> Self {
        self.init_mode = Some(mode);
        self
    }

    /// Set the config output path.
    pub fn config_out(mut self, path: impl AsRef<Path>) -> Self {
        self.config_out = Some(path.as_ref().display().to_string());
        self
    }

    /// Set the genesis timestamp.
    pub fn timestamp(mut self, timestamp: Option<u64>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Set the fork block number.
    pub fn fork_block_number(mut self, block_number: Option<u64>) -> Self {
        self.fork_block_number = block_number;
        self
    }

    /// Add extra arguments.
    pub fn extra_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.extra_args.extend(args.into_iter().map(|s| s.into()));
        self
    }

    /// Build the command as a vector of strings.
    ///
    /// NOTE: `--init` / `--load-state` MUST appear before `--host` in the argument
    /// list. Anvil's nightly build has a bug where `--init` resets the host binding
    /// to 127.0.0.1 if it appears after `--host`.
    pub fn build(self) -> Vec<String> {
        let mut cmd = Vec::new();

        // Init mode must come first (before --host) due to Anvil arg ordering bug
        match self.init_mode {
            Some(AnvilInitMode::Init(path)) => {
                cmd.push("--init".to_string());
                cmd.push(path);
            }
            Some(AnvilInitMode::LoadState(path)) => {
                cmd.push("--load-state".to_string());
                cmd.push(path);
            }
            None => {
                tracing::trace!("No init data for anvil. Starting an empty chain.")
            }
        }

        cmd.extend([
            "--host".to_string(),
            self.host,
            "--port".to_string(),
            self.port.to_string(),
            "--chain-id".to_string(),
            self.chain_id.to_string(),
            "--block-time".to_string(),
            self.block_time.to_string(),
            "--accounts".to_string(),
            super::DEFAULT_ACCOUNT_COUNT.to_string(),
            "-j".to_string(),
            "0".to_string(),
        ]);

        if let Some(timestamp) = self.timestamp {
            cmd.push("--timestamp".to_string());
            cmd.push(timestamp.to_string());
        }

        if let Some(fork_block_number) = self.fork_block_number {
            cmd.push("--fork-block-number".to_string());
            cmd.push(fork_block_number.to_string());
        }

        if let Some(fork_url) = self.fork_url {
            cmd.push("--fork-url".to_string());
            cmd.push(fork_url);
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
    fn test_anvil_cmd_builder_load_state() {
        let cmd = AnvilCmdBuilder::new(11155111)
            .port(8545)
            .init_mode(AnvilInitMode::LoadState("/data/state.json".to_string()))
            .build();

        assert!(cmd.contains(&"--load-state".to_string()));
        assert!(cmd.contains(&"/data/state.json".to_string()));
        assert!(!cmd.contains(&"--init".to_string()));
    }

    #[test]
    fn test_anvil_cmd_builder_init() {
        let cmd = AnvilCmdBuilder::new(900)
            .init_mode(AnvilInitMode::Init("/data/l1-genesis.json".to_string()))
            .config_out("/data/anvil.json")
            .build();

        assert!(cmd.contains(&"--init".to_string()));
        assert!(cmd.contains(&"/data/l1-genesis.json".to_string()));
        assert!(cmd.contains(&"--config-out".to_string()));
        assert!(cmd.contains(&"/data/anvil.json".to_string()));
        assert!(!cmd.contains(&"--load-state".to_string()));
    }
}
