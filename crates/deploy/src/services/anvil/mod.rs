//! Anvil service for local L1 chain.

mod cmd;

use std::path::{Path, PathBuf};

use anyhow::Context;
use backon::{ConstantBuilder, Retryable};
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::{AnvilCmdBuilder, AnvilInitMode};

use crate::{
    AccountInfo,
    docker::{DockerImage, ExposedPort, KupDocker, PortMapping, ServiceConfig},
    fs::FsHandler,
    service::{self, KupcakeService},
};

/// Input parameters for deploying Anvil.
pub struct AnvilInput {
    pub chain_id: u64,
    pub init_mode: Option<AnvilInitMode>,
    pub accounts: AnvilAccounts,
}

/// Named accounts from Anvil matching the OP Stack participant roles.
///
/// These accounts map to the roles used by the op-deployer for L1 contract deployment:
/// - Index 0: deployer (also base_fee_vault_recipient)
/// - Index 1: l1_fee_vault_recipient
/// - Index 2: sequencer_fee_vault_recipient
/// - Index 3: l1_proxy_admin_owner
/// - Index 4: l2_proxy_admin_owner
/// - Index 5: system_config_owner
/// - Index 6: unsafe_block_signer
/// - Index 7: batcher
/// - Index 8: proposer
/// - Index 9: challenger
/// - Index 10+: extra_accounts
#[derive(Debug, Clone)]
pub struct AnvilAccounts {
    /// The deployer account (index 0). Also used as base_fee_vault_recipient.
    pub deployer: AccountInfo,
    /// The L1 fee vault recipient account (index 1).
    pub l1_fee_vault_recipient: AccountInfo,
    /// The sequencer fee vault recipient account (index 2).
    pub sequencer_fee_vault_recipient: AccountInfo,
    /// The L1 proxy admin owner account (index 3).
    pub l1_proxy_admin_owner: AccountInfo,
    /// The L2 proxy admin owner account (index 4).
    pub l2_proxy_admin_owner: AccountInfo,
    /// The system config owner account (index 5).
    pub system_config_owner: AccountInfo,
    /// The unsafe block signer account (index 6).
    pub unsafe_block_signer: AccountInfo,
    /// The batcher account (index 7).
    pub batcher: AccountInfo,
    /// The proposer account (index 8).
    pub proposer: AccountInfo,
    /// The challenger account (index 9).
    pub challenger: AccountInfo,
    /// Additional accounts beyond the named roles (index 10+).
    pub extra_accounts: Vec<AccountInfo>,
}

impl AnvilAccounts {
    /// The minimum number of accounts required for OP Stack deployment.
    pub const MIN_REQUIRED_ACCOUNTS: usize = 10;

    /// Returns all accounts as a slice, in order (named accounts first, then extra).
    pub fn all_accounts(&self) -> Vec<&AccountInfo> {
        let mut accounts = vec![
            &self.deployer,
            &self.l1_fee_vault_recipient,
            &self.sequencer_fee_vault_recipient,
            &self.l1_proxy_admin_owner,
            &self.l2_proxy_admin_owner,
            &self.system_config_owner,
            &self.unsafe_block_signer,
            &self.batcher,
            &self.proposer,
            &self.challenger,
        ];
        accounts.extend(self.extra_accounts.iter());
        accounts
    }

    /// Write accounts to `anvil.json` in the format expected by faucet/spam commands.
    ///
    /// This produces the same JSON structure that Anvil's `--config-out` flag writes,
    /// with `available_accounts` and `private_keys` arrays. We write it ourselves
    /// because `--config-out` is incompatible with `--init` (genesis mode).
    pub fn write_anvil_json(&self, outdata: &std::path::Path) -> Result<(), anyhow::Error> {
        let anvil_dir = outdata.join("anvil");
        if !anvil_dir.exists() {
            std::fs::create_dir_all(&anvil_dir).context("Failed to create anvil data directory")?;
        }

        let accounts = self.all_accounts();
        let available_accounts: Vec<String> = accounts
            .iter()
            .map(|a| format!("0x{}", hex::encode(&a.address)))
            .collect();
        let private_keys: Vec<String> = accounts
            .iter()
            .map(|a| format!("0x{}", hex::encode(&a.private_key)))
            .collect();

        let data = serde_json::json!({
            "available_accounts": available_accounts,
            "private_keys": private_keys,
        });

        let path = anvil_dir.join("anvil.json");
        std::fs::write(&path, serde_json::to_string_pretty(&data)?)
            .with_context(|| format!("Failed to write {}", path.display()))?;

        tracing::debug!(path = %path.display(), "Wrote anvil.json with {} accounts", accounts.len());
        Ok(())
    }
}

/// Default port for Anvil.
pub const DEFAULT_PORT: u16 = 8545;

/// Number of accounts Anvil generates from its HD mnemonic.
/// Must match the count passed to `derive_accounts_from_mnemonic` in genesis mode.
pub const DEFAULT_ACCOUNT_COUNT: usize = 30;

/// Default Docker image for Anvil (Foundry).
pub const DEFAULT_DOCKER_IMAGE: &str = "ghcr.io/foundry-rs/foundry";
/// Default Docker tag for Anvil (Foundry).
pub const DEFAULT_DOCKER_TAG: &str = "latest";

/// Configuration for Anvil.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnvilConfig {
    /// Docker image configuration for Anvil.
    pub docker_image: DockerImage,
    /// Host address for Anvil.
    pub host: String,
    /// Port for Anvil RPC (container port).
    pub port: u16,
    /// Host port for Anvil RPC. If None, not published to host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_port: Option<u16>,
    /// Block time in seconds.
    pub block_time: u64,
    /// URL to fork from (optional, if not provided Anvil runs without forking).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_url: Option<String>,
    /// Container name for Anvil.
    pub container_name: String,
    /// Genesis timestamp.
    pub timestamp: Option<u64>,
    /// Fork block number.
    pub fork_block_number: Option<u64>,
    /// Extra arguments to pass to Anvil.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

impl Default for AnvilConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-anvil".to_string(),
            host: "0.0.0.0".to_string(),
            port: DEFAULT_PORT,
            host_port: Some(0), // Let OS pick an available port
            block_time: 12,
            fork_url: None,
            timestamp: None,
            fork_block_number: None,
            extra_args: Vec::new(),
        }
    }
}

/// Handler for a running Anvil instance.
pub struct AnvilHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// The RPC URL for the L1 chain behind Anvil (internal Docker network).
    pub l1_rpc_url: Url,
    /// The RPC URL accessible from host (if published). None if not published.
    pub l1_host_url: Option<Url>,
    /// Named accounts from Anvil matching the OP Stack participant roles.
    pub accounts: AnvilAccounts,
}

/// Anvil listens on port 8545 inside the container.
const ANVIL_INTERNAL_PORT: u16 = 8545;

impl AnvilConfig {
    /// Build the Docker command arguments for Anvil.
    pub fn build_cmd(
        &self,
        _host_config_path: &Path,
        input: &AnvilInput,
    ) -> Result<Vec<String>, anyhow::Error> {
        let mut cmd_builder = AnvilCmdBuilder::new(input.chain_id)
            .host("0.0.0.0")
            .port(ANVIL_INTERNAL_PORT)
            .block_time(self.block_time)
            .timestamp(self.timestamp)
            .fork_block_number(self.fork_block_number)
            .extra_args(self.extra_args.clone());

        if let Some(ref mode) = input.init_mode {
            cmd_builder = cmd_builder.init_mode(mode.clone());
        }

        if let Some(ref fork_url) = self.fork_url {
            cmd_builder = cmd_builder.fork_url(fork_url);
        }

        Ok(cmd_builder.build())
    }
}

impl KupcakeService for AnvilConfig {
    type Input = AnvilInput;
    type Output = AnvilHandler;

    fn container_name(&self) -> &str {
        &self.container_name
    }

    fn docker_image(&self) -> &DockerImage {
        &self.docker_image
    }

    async fn deploy<'a>(
        &'a self,
        docker: &'a mut KupDocker,
        host_config_path: &'a Path,
        input: AnvilInput,
    ) -> Result<AnvilHandler, anyhow::Error> {
        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path.to_path_buf())?;
        }

        let container_config_path = PathBuf::from("/data");

        let cmd = self.build_cmd(host_config_path, &input)?;

        // Build port mappings only for ports that should be published to host
        let port_mappings: Vec<PortMapping> =
            PortMapping::tcp_optional(ANVIL_INTERNAL_PORT, self.host_port)
                .into_iter()
                .collect();

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .entrypoint(vec!["anvil".to_string()])
            .cmd(cmd)
            .expose(ExposedPort::tcp(ANVIL_INTERNAL_PORT))
            .ports(port_mappings)
            .bind(host_config_path, &container_config_path, "rw");

        let mut handler = service::deploy_container(
            docker,
            &self.docker_image,
            &self.container_name,
            service_config,
        )
        .await
        .context("Failed to start Anvil container")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            "Anvil container started"
        );

        // Wait for Anvil to bind its ports (confirms container is ready)
        let container_id = handler.container_id.clone();
        handler.bound_ports = (|| async {
            let ports = docker.get_container_bound_ports(&container_id).await?;

            if ports.is_empty() {
                anyhow::bail!("no port bindings yet");
            }

            Ok(ports)
        })
        .retry(
            ConstantBuilder::default()
                .with_delay(std::time::Duration::from_millis(500))
                .with_max_times(30),
        )
        .await
        .context("Anvil port bindings not available after 15s — container may have crashed")?;

        let l1_rpc_url = KupDocker::build_http_url(&handler.container_name, ANVIL_INTERNAL_PORT)?;

        // Build host-accessible URL from bound port
        let l1_host_url = handler.build_host_url(ANVIL_INTERNAL_PORT, "http")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            ?l1_host_url,
            "Anvil container started"
        );

        Ok(AnvilHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            accounts: input.accounts,
            l1_rpc_url,
            l1_host_url,
        })
    }
}
