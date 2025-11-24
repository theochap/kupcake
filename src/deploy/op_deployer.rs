use std::path::PathBuf;

use anyhow::Context;
use bollard::{
    container::{Config, StartContainerOptions},
    secret::HostConfig,
};
use serde::{Deserialize, Serialize};

use crate::deploy::{AccountInfo, KupDocker, anvil::AnvilHandler, fs::FsHandler};

/// The minimum number of accounts required for the intent file. Those are:
/// [`ChainConfig::base_fee_vault_recipient`], [`ChainConfig::l1_fee_vault_recipient`], [`ChainConfig::sequencer_fee_vault_recipient`], [`ChainRoles::l1_proxy_admin_owner`],
/// [`ChainRoles::l2_proxy_admin_owner`], [`ChainRoles::system_config_owner`], [`ChainRoles::unsafe_block_signer`], [`ChainRoles::batcher`], [`ChainRoles::proposer`], [`ChainRoles::challenger`].
const MIN_ACCOUNTS_FOR_INTENT: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntentFile {
    config_type: String,
    #[serde(rename = "l1ChainID")]
    l1_chain_id: u64,
    opcm_address: String,
    fund_dev_accounts: bool,
    l1_contracts_locator: String,
    l2_contracts_locator: String,
    chains: Vec<ChainConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChainConfig {
    id: String,
    base_fee_vault_recipient: String,
    l1_fee_vault_recipient: String,
    sequencer_fee_vault_recipient: String,
    eip1559_denominator_canyon: u64,
    eip1559_denominator: u64,
    eip1559_elasticity: u64,
    gas_limit: u64,
    operator_fee_scalar: u64,
    operator_fee_constant: u64,
    min_base_fee: u64,
    da_footprint_gas_scalar: u64,
    roles: ChainRoles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChainRoles {
    l1_proxy_admin_owner: String,
    l2_proxy_admin_owner: String,
    system_config_owner: String,
    unsafe_block_signer: String,
    batcher: String,
    proposer: String,
    challenger: String,
}

pub struct OpDeployerConfig {
    pub container_name: String,
}

pub struct OpDeployerHandler {
    pub container_id: String,
}

impl OpDeployerConfig {
    async fn generate_intent_file(
        &self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        container_config_path: PathBuf,
        l1_chain_id: u64,
        l2_chain_id: u64,
    ) -> Result<PathBuf, anyhow::Error> {
        let cmd = vec![
            "op-deployer".to_string(),
            "--cache-dir".to_string(),
            container_config_path.join(".cache").display().to_string(),
            "init".to_string(),
            "--l1-chain-id".to_string(),
            l1_chain_id.to_string(),
            "--l2-chain-ids".to_string(),
            l2_chain_id.to_string(),
            "--workdir".to_string(),
            container_config_path.display().to_string(),
            "--intent-type".to_string(),
            "standard-overrides".to_string(),
        ];

        // Get current user UID and GID to run container as non-root
        // This ensures files created by the container have the correct and can be rewritten by this process.
        #[cfg(unix)]
        let user = {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(&host_config_path)
                .context("Failed to get metadata for host config path")?;
            Some(format!("{}:{}", metadata.uid(), metadata.gid()))
        };

        #[cfg(not(unix))]
        let user: Option<String> = None;

        // Bind mount: host_path:container_path
        // This maps the host file to the container file so data persists on the host
        let host_config = HostConfig {
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            auto_remove: Some(true),
            ..Default::default()
        };

        // Create container configuration
        let config = Config {
            image: Some(format!(
                "{}:{}",
                docker.config.op_deployer_docker_image, docker.config.op_deployer_docker_tag
            )),
            cmd: Some(cmd),
            host_config: Some(host_config),
            user,
            ..Default::default()
        };

        // Start the container
        docker
            .create_and_start_container(
                &format!("{}-init", self.container_name),
                config,
                None::<StartContainerOptions<String>>,
            )
            .await
            .context("Failed to start Op Deployer container")?;

        // Wait for the intent file to be created
        let config_file_path = host_config_path.join("intent.toml");
        FsHandler::wait_for_file(&config_file_path, std::time::Duration::from_secs(30))
            .await
            .context("Op Deployer config file was not created in time")?;

        tracing::info!(
            "✓ Op Deployer intent file created at: {}",
            config_file_path.display()
        );

        Ok(config_file_path)
    }

    async fn apply_contract_deployments(
        &self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        container_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
    ) -> Result<OpDeployerHandler, anyhow::Error> {
        let cmd = vec![
            "op-deployer".to_string(),
            "--cache-dir".to_string(),
            container_config_path.join(".cache").display().to_string(),
            "apply".to_string(),
            "--workdir".to_string(),
            container_config_path.display().to_string(),
            "--l1-rpc-url".to_string(),
            anvil_handler.l1_rpc_url.to_string(),
            "--private-key".to_string(),
            anvil_handler.account_infos[0].private_key.to_string(),
        ];

        // Get the network mode - use the Docker network if available
        let network_mode = docker.network_id.as_ref().map(|id| id.clone());

        // Bind mount: host_path:container_path
        // This maps the host file to the container file so data persists on the host
        let host_config = HostConfig {
            binds: Some(vec![format!(
                "{}:{}:rw",
                host_config_path.display(),
                container_config_path.to_string_lossy()
            )]),
            auto_remove: Some(true),
            network_mode,
            ..Default::default()
        };

        // Create container configuration
        let config = Config {
            image: Some(format!(
                "{}:{}",
                docker.config.op_deployer_docker_image, docker.config.op_deployer_docker_tag
            )),
            cmd: Some(cmd),
            host_config: Some(host_config),
            ..Default::default()
        };

        // Start the container
        let container_id = docker
            .create_and_start_container(
                &format!("{}-apply", self.container_name),
                config,
                None::<StartContainerOptions<String>>,
            )
            .await
            .context("Failed to start Op Deployer apply container")?;

        tracing::info!("✓ Op Deployer apply container started: {}", container_id);

        // Wait for the container to complete
        docker
            .wait_for_container(&container_id)
            .await
            .context("Op Deployer apply container failed")?;

        tracing::info!("✓ Op Deployer apply container completed successfully");

        Ok(OpDeployerHandler { container_id })
    }

    pub async fn start(
        self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
        l1_chain_id: u64,
        l2_chain_id: u64,
    ) -> Result<OpDeployerHandler, anyhow::Error> {
        tracing::info!("Starting Op Deployer container '{}'", self.container_name);

        // Build the command
        // Container path where anvil will write the config
        let container_config_path = PathBuf::from("/data");

        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        let config_file_path = self
            .generate_intent_file(
                docker,
                host_config_path.clone(),
                container_config_path.clone(),
                l1_chain_id,
                l2_chain_id,
            )
            .await
            .context("Failed to generate intent file")?;

        // Parse the intent file and update with the account addresses generated by anvil.
        Self::update_intent_with_accounts(&config_file_path, &anvil_handler.account_infos)
            .await
            .context("Failed to update intent file with account addresses")?;

        tracing::info!("✓ Intent file updated with account addresses from Anvil");

        // Now apply the contract deployments.
        let op_deployer_handler = self
            .apply_contract_deployments(
                docker,
                host_config_path,
                container_config_path,
                anvil_handler,
            )
            .await
            .context("Failed to apply contract deployments")?;

        Ok(op_deployer_handler)
    }

    /// Updates the intent.toml file with account addresses from Anvil.
    ///
    /// This function replaces the placeholder addresses in the intent file with
    /// actual addresses from the accounts generated by Anvil at startup.
    async fn update_intent_with_accounts(
        intent_path: &PathBuf,
        accounts: &[AccountInfo],
    ) -> Result<(), anyhow::Error> {
        // Read the intent file
        let content = tokio::fs::read_to_string(intent_path)
            .await
            .context("Failed to read intent file")?;

        // Parse the TOML
        let mut intent: IntentFile =
            toml::from_str(&content).context("Failed to parse intent file as TOML")?;

        // Ensure we have enough accounts
        if accounts.len() < MIN_ACCOUNTS_FOR_INTENT {
            anyhow::bail!(
                "Not enough accounts provided. Need at least {}, got {}",
                MIN_ACCOUNTS_FOR_INTENT,
                accounts.len()
            );
        }

        // Helper function to format address as lowercase hex string with 0x prefix
        let format_address =
            |bytes: &[u8]| -> String { format!("0x{}", hex::encode(bytes).to_lowercase()) };

        // Update the roles with account addresses
        // Map accounts to roles based on their index
        for chain in &mut intent.chains {
            chain.base_fee_vault_recipient = format_address(&accounts[0].address);
            chain.l1_fee_vault_recipient = format_address(&accounts[1].address);
            chain.sequencer_fee_vault_recipient = format_address(&accounts[2].address);

            chain.roles.l1_proxy_admin_owner = format_address(&accounts[3].address);
            chain.roles.l2_proxy_admin_owner = format_address(&accounts[4].address);
            chain.roles.system_config_owner = format_address(&accounts[5].address);
            chain.roles.unsafe_block_signer = format_address(&accounts[6].address);

            chain.roles.batcher = format_address(&accounts[7].address);
            chain.roles.proposer = format_address(&accounts[8].address);
            chain.roles.challenger = format_address(&accounts[9].address);

            tracing::debug!(
                chain_id = chain.id,
                base_fee_vault_recipient = chain.base_fee_vault_recipient,
                l1_fee_vault_recipient = chain.l1_fee_vault_recipient,
                sequencer_fee_vault_recipient = chain.sequencer_fee_vault_recipient,
                l1_proxy_admin_owner = chain.roles.l1_proxy_admin_owner,
                l2_proxy_admin_owner = chain.roles.l2_proxy_admin_owner,
                system_config_owner = chain.roles.system_config_owner,
                unsafe_block_signer = chain.roles.unsafe_block_signer,
                batcher = chain.roles.batcher,
                proposer = chain.roles.proposer,
                challenger = chain.roles.challenger,
                "Updated chain roles with Anvil account addresses"
            );
        }

        // Serialize back to TOML
        let updated_content =
            toml::to_string_pretty(&intent).context("Failed to serialize intent file to TOML")?;

        // Write back to file
        tokio::fs::write(intent_path, updated_content)
            .await
            .context("Failed to write updated intent file")?;

        Ok(())
    }
}
