use std::{collections::HashMap, path::PathBuf};

use alloy_core::primitives::Bytes;
use anyhow::Context;
use bollard::{
    container::{Config, StartContainerOptions},
    secret::{HostConfig, PortBinding},
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::deploy::{AccountInfo, docker::KupDocker, fs::FsHandler};

pub struct AnvilConfig {
    pub host: String,
    pub port: u16,
    pub fork_url: Option<String>,

    pub extra_args: Vec<String>,

    pub container_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct L1AnvilData {
    available_accounts: Vec<Bytes>,
    private_keys: Vec<Bytes>,
}

pub struct AnvilHandler {
    pub container_id: String,
    pub container_name: String,

    /// The RPC URL for the L1 chain behind Anvil.
    pub l1_rpc_url: Url,

    pub account_infos: Vec<AccountInfo>,
}

impl AnvilConfig {
    /// Start an Anvil container.
    ///
    /// # Arguments
    /// * `container_name` - Name for the container
    /// * `port` - Host port to bind to (default: 8545)
    /// * `chain_id` - Chain ID for Anvil
    /// * `extra_args` - Additional arguments to pass to Anvil
    ///
    /// # Returns
    /// The container ID
    pub async fn start(
        self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        chain_id: u64,
    ) -> Result<AnvilHandler, anyhow::Error> {
        tracing::info!(
            "Starting Anvil container '{}' on port {} with chain ID {}",
            self.container_name,
            self.port,
            chain_id
        );

        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Build the command
        // Container path where anvil will write the config
        let container_config_path = PathBuf::from("/data");

        let mut cmd = vec![
            "--host".to_string(),
            "0.0.0.0".to_string(),
            "--chain-id".to_string(),
            chain_id.to_string(),
            "--config-out".to_string(),
            container_config_path
                .join("anvil.json")
                .display()
                .to_string(),
        ];

        // Add fork URL if provided
        if let Some(ref fork_url) = self.fork_url {
            cmd.push("--fork-url".to_string());
            cmd.push(fork_url.clone());
        }

        cmd.extend(self.extra_args);

        // Configure port binding
        let port_bindings = HashMap::from([(
            "8545/tcp".to_string(),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(self.port.to_string()),
            }]),
        )]);

        // Get the network mode - use the Docker network if available
        let network_mode = docker.network_id.as_ref().map(|id| id.clone());

        // Bind mount: host_path:container_path
        // This maps the host file to the container file so data persists on the host
        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            network_mode,
            ..Default::default()
        };

        // Create container configuration
        let config = Config {
            entrypoint: Some(vec!["anvil".to_string()]),
            image: Some(format!(
                "{}:{}",
                docker.config.foundry_docker_image, docker.config.foundry_docker_tag
            )),
            cmd: Some(cmd),
            host_config: Some(host_config),
            ..Default::default()
        };

        // Start the container
        let container_id = docker
            .create_and_start_container(
                &self.container_name,
                config,
                None::<StartContainerOptions<String>>,
            )
            .await
            .context("Failed to start Anvil container")?;

        tracing::info!("✓ Anvil container started: {}", container_id);
        tracing::info!("  Data persisted to: {}", host_config_path.display());

        // Wait for the Anvil config file to be created
        let config_file_path = host_config_path.join("anvil.json");
        tracing::info!("Waiting for Anvil config file...");

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

        tracing::info!(
            l1_config = ?l1_config,
            "✓ Anvil config parsed successfully",
        );

        // Get the account infos from the Anvil config
        let account_infos = l1_config
            .available_accounts
            .iter()
            .zip(l1_config.private_keys)
            .map(|(address, private_key)| AccountInfo {
                address: address.clone(),
                private_key: private_key.clone(),
            })
            .collect();

        // Determine the RPC URL based on whether we're using a Docker network
        let l1_rpc_url = if docker.network_id.is_some() {
            // When using Docker network, containers can communicate using container names
            Url::parse(&format!("http://{}:8545", self.container_name))
                .context("Failed to parse Anvil RPC URL")?
        } else {
            // When not using Docker network, use host and port
            Url::parse(&format!("http://{}:{}", self.host, self.port))
                .context("Failed to parse Anvil RPC URL")?
        };

        Ok(AnvilHandler {
            container_id,
            container_name: self.container_name.clone(),
            account_infos,
            l1_rpc_url,
        })
    }
}
