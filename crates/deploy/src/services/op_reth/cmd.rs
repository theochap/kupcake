//! Command builder for op-reth execution client.

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
    discovery_port: u16,
    listen_port: u16,
    sequencer_http: Option<String>,
    bootnodes: Vec<String>,
    nat_dns: Option<String>,
    net_if: Option<String>,
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
            http_api: "eth,net,web3,debug,trace,txpool,admin".to_string(),
            ws_addr: "0.0.0.0".to_string(),
            ws_port: 8546,
            ws_api: "eth,net,web3,debug,trace,txpool,admin".to_string(),
            authrpc_addr: "0.0.0.0".to_string(),
            authrpc_port: 8551,
            authrpc_jwtsecret: String::new(),
            metrics: None,
            discovery_disabled: false,
            discovery_port: 30303,
            listen_port: 30303,
            sequencer_http: None,
            bootnodes: Vec::new(),
            nat_dns: None,
            net_if: None,
            log_format: "terminal".to_string(),
            extra_args: Vec::new(),
        }
    }

    /// Set the HTTP RPC address.
    pub fn http_addr(mut self, addr: impl Into<String>) -> Self {
        self.http_addr = addr.into();
        self
    }

    /// Set the NAT DNS.
    pub fn nat_dns(mut self, dns: impl Into<String>) -> Self {
        self.nat_dns = Some(dns.into());
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

    /// Set the listen port.
    pub fn listen_port(mut self, port: u16) -> Self {
        self.listen_port = port;
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

    /// Set the P2P discovery port.
    pub fn discovery_port(mut self, port: u16) -> Self {
        self.discovery_port = port;
        self
    }

    /// Set the sequencer HTTP URL.
    pub fn sequencer_http(mut self, url: impl Into<String>) -> Self {
        self.sequencer_http = Some(url.into());
        self
    }

    /// Set the bootnodes (enode URLs) for P2P peer discovery.
    pub fn bootnodes(mut self, bootnodes: Vec<String>) -> Self {
        self.bootnodes = bootnodes;
        self
    }

    pub fn net_if(mut self, net_if: Option<String>) -> Self {
        self.net_if = net_if;
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
            "--port".to_string(),
            self.listen_port.to_string(),
        ];

        if let Some(nat_dns) = self.nat_dns {
            cmd.push(format!("--nat=extaddr:{}", nat_dns));
        }

        if let Some(net_if) = self.net_if {
            cmd.push("--net-if.experimental".to_string());
            cmd.push(net_if);
        }

        if let Some(metrics) = self.metrics {
            cmd.push("--metrics".to_string());
            cmd.push(metrics);
        }

        if self.discovery_disabled {
            cmd.push("--disable-discovery".to_string());
        } else {
            cmd.push("--discovery.port".to_string());
            cmd.push(self.discovery_port.to_string());
            cmd.push("--enable-discv5-discovery".to_string());
            cmd.push("--disable-discv4-discovery".to_string());
        }

        if !self.bootnodes.is_empty() {
            cmd.push("--trusted-peers".to_string());
            cmd.push(self.bootnodes.join(","));
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
}
