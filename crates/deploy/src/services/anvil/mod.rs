//! Anvil service for local L1 chain.

mod cmd;

use std::path::PathBuf;

use alloy_core::primitives::Bytes;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;

pub use cmd::AnvilCmdBuilder;

use crate::{
    AccountInfo,
    docker::{CreateAndStartContainerOptions, DockerImage, ExposedPort, KupDocker, PortMapping, ServiceConfig},
    fs::FsHandler,
};

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

    /// Create named accounts from a vector of account infos.
    ///
    /// Returns an error if fewer than 10 accounts are provided.
    pub fn from_account_infos(accounts: Vec<AccountInfo>) -> Result<Self, anyhow::Error> {
        if accounts.len() < Self::MIN_REQUIRED_ACCOUNTS {
            anyhow::bail!(
                "Not enough accounts provided. Need at least {}, got {}",
                Self::MIN_REQUIRED_ACCOUNTS,
                accounts.len()
            );
        }

        let mut accounts = accounts.into_iter();

        Ok(Self {
            deployer: accounts.next().unwrap(),
            l1_fee_vault_recipient: accounts.next().unwrap(),
            sequencer_fee_vault_recipient: accounts.next().unwrap(),
            l1_proxy_admin_owner: accounts.next().unwrap(),
            l2_proxy_admin_owner: accounts.next().unwrap(),
            system_config_owner: accounts.next().unwrap(),
            unsafe_block_signer: accounts.next().unwrap(),
            batcher: accounts.next().unwrap(),
            proposer: accounts.next().unwrap(),
            challenger: accounts.next().unwrap(),
            extra_accounts: accounts.collect(),
        })
    }

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
}

/// Default port for Anvil.
pub const DEFAULT_PORT: u16 = 8545;

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
    #[serde(
        default = "default_anvil_host_port",
        skip_serializing_if = "Option::is_none"
    )]
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

fn default_anvil_host_port() -> Option<u16> {
    Some(0) // Let OS pick an available port
}

/// Parsed Anvil configuration data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct L1AnvilData {
    available_accounts: Vec<Bytes>,
    private_keys: Vec<Bytes>,
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

impl AnvilConfig {
    /// Start an Anvil container.
    ///
    /// # Arguments
    /// * `docker` - Docker client
    /// * `host_config_path` - Path on host to store Anvil data
    /// * `chain_id` - Chain ID for Anvil
    ///
    /// # Returns
    /// An `AnvilHandler` with the running container information.
    pub async fn start(
        self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        chain_id: u64,
    ) -> Result<AnvilHandler, anyhow::Error> {
        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Container path where anvil will write the config
        let container_config_path = PathBuf::from("/data");

        // Anvil listens on port 8545 inside the container
        const ANVIL_INTERNAL_PORT: u16 = 8545;

        let mut cmd_builder = AnvilCmdBuilder::new(chain_id)
            .host("0.0.0.0")
            .port(ANVIL_INTERNAL_PORT)
            .block_time(self.block_time)
            .timestamp(self.timestamp)
            .fork_block_number(self.fork_block_number)
            .config_out(container_config_path.join("anvil.json"))
            .state_path(container_config_path.clone())
            .extra_args(self.extra_args.clone());

        if let Some(ref fork_url) = self.fork_url {
            cmd_builder = cmd_builder.fork_url(fork_url);
        }

        let cmd = cmd_builder.build();

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
            .bind(&host_config_path, &container_config_path, "rw");

        let handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions::default(),
            )
            .await
            .context("Failed to start Anvil container")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            "Anvil container started"
        );

        // Wait for the Anvil config file to be created
        let config_file_path = host_config_path.join("anvil.json");

        FsHandler::wait_for_file(&config_file_path, std::time::Duration::from_secs(30))
            .await
            .context("Anvil config file was not created in time")?;

        // Parse the Anvil config
        let l1_config = serde_json::from_str::<L1AnvilData>(
            &tokio::fs::read_to_string(&config_file_path)
                .await
                .context(format!(
                    "Failed to read Anvil config from {}",
                    config_file_path.display()
                ))?,
        )
        .context("Failed to parse Anvil config")?;

        let account_infos: Vec<AccountInfo> = l1_config
            .available_accounts
            .iter()
            .zip(l1_config.private_keys)
            .map(|(address, private_key)| AccountInfo {
                address: address.clone(),
                private_key: private_key.clone(),
            })
            .collect();

        let accounts = AnvilAccounts::from_account_infos(account_infos)
            .context("Failed to create named accounts from Anvil")?;

        let l1_rpc_url = KupDocker::build_http_url(&handler.container_name, ANVIL_INTERNAL_PORT)?;

        // Build host-accessible URL from bound port
        let l1_host_url = handler
            .get_tcp_host_port(ANVIL_INTERNAL_PORT)
            .map(|port| Url::parse(&format!("http://localhost:{}/", port)))
            .transpose()
            .context("Failed to build Anvil host URL")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            ?l1_host_url,
            "Anvil container started"
        );

        Ok(AnvilHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            accounts,
            l1_rpc_url,
            l1_host_url,
        })
    }
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
