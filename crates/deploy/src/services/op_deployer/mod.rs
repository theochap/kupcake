//! OP Deployer service for deploying L1 contracts.

use std::path::PathBuf;

use anyhow::Context;
use bollard::{container::Config, secret::HostConfig};
use serde::{Deserialize, Serialize};

use crate::{
    AccountInfo,
    docker::{DockerImage, DockerImageBuilder, KupDocker},
    fs::FsHandler,
};

use super::anvil::AnvilHandler;

/// Default Docker image for op-deployer.
pub const DEFAULT_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-deployer";
/// Default Docker tag for op-deployer.
pub const DEFAULT_DOCKER_TAG: &str = "v0.5.0-rc.2";

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

/// Configuration for the OP Deployer service.
pub struct OpDeployerConfig {
    /// Docker image configuration for op-deployer.
    pub docker_image: DockerImageBuilder,
    /// Container name for op-deployer.
    pub container_name: String,
}

impl Default for OpDeployerConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImageBuilder::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-op-deployer".to_string(),
        }
    }
}

impl OpDeployerConfig {
    pub async fn run_docker_container(
        &self,
        docker: &mut KupDocker,
        image: &DockerImage,
        container_name: &str,
        host_config_path: &PathBuf,
        container_config_path: &PathBuf,
        cmd: Vec<String>,
    ) -> Result<(), anyhow::Error> {
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
            auto_remove: Some(false),
            network_mode: Some(docker.network_id.clone()),
            ..Default::default()
        };

        // Create container configuration
        let config = Config {
            image: Some(image.full_name()),
            cmd: Some(cmd),
            host_config: Some(host_config),
            user,
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        use crate::docker::CreateAndStartContainerOptions;

        // Start the container
        let container_id = docker
            .create_and_start_container(
                container_name,
                config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    wait_for_container: true,
                    start_options: None,
                },
            )
            .await
            .context(format!(
                "Failed to start Op Deployer container: {}",
                container_name
            ))?;

        tracing::debug!(
            container_id,
            container_name,
            "Op Deployer container completed successfully",
        );

        Ok(())
    }

    async fn generate_intent_file(
        &self,
        docker: &mut KupDocker,
        image: &DockerImage,
        host_config_path: &PathBuf,
        container_config_path: &PathBuf,
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

        tracing::debug!(l2_chain_id, "Deploying contracts");

        self.run_docker_container(
            docker,
            image,
            &format!("{}-init", self.container_name),
            host_config_path,
            container_config_path,
            cmd,
        )
        .await?;

        // Wait for the intent file to be created
        let config_file_path = host_config_path.join("intent.toml");
        FsHandler::wait_for_file(&config_file_path, std::time::Duration::from_secs(30))
            .await
            .context("Op Deployer config file was not created in time")?;

        tracing::debug!(?config_file_path, "Op Deployer intent file created");

        Ok(config_file_path)
    }

    async fn apply_contract_deployments(
        &self,
        docker: &mut KupDocker,
        image: &DockerImage,
        host_config_path: &PathBuf,
        container_config_path: &PathBuf,
        anvil_handler: &AnvilHandler,
    ) -> Result<(), anyhow::Error> {
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

        self.run_docker_container(
            docker,
            image,
            &format!("{}-apply", self.container_name),
            host_config_path,
            container_config_path,
            cmd,
        )
        .await?;

        Ok(())
    }

    async fn generate_config_files(
        &self,
        docker: &mut KupDocker,
        image: &DockerImage,
        host_config_path: &PathBuf,
        container_config_path: &PathBuf,
        l2_chain_id: u64,
    ) -> Result<(), anyhow::Error> {
        let container_config_path_str = container_config_path.display().to_string();
        let config_cmd = |config_type: &str| -> Vec<String> {
            vec![
                "sh".to_string(),
                "-c".to_string(),
                format!(
                    "op-deployer --cache-dir {container_config_path_str}/.cache inspect {config_type} --workdir {container_config_path_str} {l2_chain_id} > {container_config_path_str}/{config_type}.json",
                ),
            ]
        };

        self.run_docker_container(
            docker,
            image,
            &format!("{}-inspect-genesis", self.container_name),
            host_config_path,
            container_config_path,
            config_cmd("genesis"),
        )
        .await?;

        self.run_docker_container(
            docker,
            image,
            &format!("{}-inspect-rollup", self.container_name),
            host_config_path,
            container_config_path,
            config_cmd("rollup"),
        )
        .await?;

        // Wait for the rollup config file to be created
        let genesis_file_path = host_config_path.join("genesis.json");
        let rollup_file_path = host_config_path.join("rollup.json");

        let (genesis_result, rollup_result) = tokio::join!(
            FsHandler::wait_for_file(&genesis_file_path, std::time::Duration::from_secs(30)),
            FsHandler::wait_for_file(&rollup_file_path, std::time::Duration::from_secs(30)),
        );

        genesis_result.context("Op Deployer genesis config file was not created in time")?;
        rollup_result.context("Op Deployer rollup config file was not created in time")?;

        tracing::debug!(
            ?genesis_file_path,
            ?rollup_file_path,
            "Op Deployer config files created",
        );

        Ok(())
    }

    pub async fn deploy_contracts(
        self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        anvil_handler: &AnvilHandler,
        l1_chain_id: u64,
        l2_chain_id: u64,
    ) -> Result<(), anyhow::Error> {
        // Build the command
        // Container path where anvil will write the config
        let container_config_path = PathBuf::from("/data");

        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Pull the op-deployer image once and reuse it
        let image = self.docker_image.build(docker).await?;

        let config_file_path = self
            .generate_intent_file(
                docker,
                &image,
                &host_config_path,
                &container_config_path,
                l1_chain_id,
                l2_chain_id,
            )
            .await
            .context("Failed to generate intent file")?;

        // Parse the intent file and update with the account addresses generated by anvil.
        Self::update_intent_with_accounts(&config_file_path, &anvil_handler.account_infos)
            .await
            .context("Failed to update intent file with account addresses")?;

        tracing::debug!("Intent file updated with account addresses from Anvil");

        // Now apply the contract deployments.
        self.apply_contract_deployments(
            docker,
            &image,
            &host_config_path,
            &container_config_path,
            anvil_handler,
        )
        .await
        .context("Failed to apply contract deployments")?;

        // Now generate the config files to be used by the L2 nodes.
        self.generate_config_files(
            docker,
            &image,
            &host_config_path,
            &container_config_path,
            l2_chain_id,
        )
        .await
        .context("Failed to generate config files")?;

        Ok(())
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
