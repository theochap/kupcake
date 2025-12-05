use std::{collections::HashMap, path::PathBuf};

use alloy_core::primitives::Bytes;
use anyhow::Context;
use bollard::{
    container::Config,
    secret::{HostConfig, PortBinding},
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::deploy::{AccountInfo, cmd_builders::AnvilCmdBuilder, docker::KupDocker, fs::FsHandler};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnvilConfig {
    pub host: String,
    pub port: u16,
    pub fork_url: String,

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
        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Build the command using the builder
        // Container path where anvil will write the config
        let container_config_path = PathBuf::from("/data");

        let cmd = AnvilCmdBuilder::new(chain_id)
            .host("0.0.0.0")
            .port(8545) // Internal port, mapped to self.port on host
            .fork_url(&self.fork_url)
            .config_out(container_config_path.join("anvil.json"))
            .extra_args(self.extra_args.clone())
            .build();

        // Configure port binding
        let port_bindings = HashMap::from([(
            "8545/tcp".to_string(),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(self.port.to_string()),
            }]),
        )]);

        // Bind mount: host_path:container_path
        // This maps the host file to the container file so data persists on the host
        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            network_mode: Some(docker.network_id.clone()),
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
            .create_and_start_container(&self.container_name, config, Default::default())
            .await
            .context("Failed to start Anvil container")?;

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

        // When using Docker network, containers can communicate using container names
        let l1_rpc_url = Url::parse(&format!("http://{}:8545", self.container_name))
            .context("Failed to parse Anvil RPC URL")?;

        Ok(AnvilHandler {
            container_id,
            container_name: self.container_name.clone(),
            account_infos,
            l1_rpc_url,
        })
    }
}
