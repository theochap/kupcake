//! Command builder for Anvil.

use std::path::Path;

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
            "--block-time".to_string(),
            "12".to_string(),
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
