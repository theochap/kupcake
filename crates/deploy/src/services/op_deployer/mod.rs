//! OP Deployer service for deploying L1 contracts.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    docker::{CreateAndStartContainerOptions, DockerImage, KupDocker, ServiceConfig},
    fs::FsHandler,
};

use crate::AccountInfo;

use super::{
    anvil::{AnvilAccounts, AnvilHandler},
    kona_node::is_known_l1_chain,
};

/// Create named accounts from a vector of account infos.
///
/// Maps account infos to OP Stack participant roles by index.
/// Returns an error if fewer than 10 accounts are provided.
pub fn anvil_accounts_from_infos(
    accounts: Vec<AccountInfo>,
) -> Result<AnvilAccounts, anyhow::Error> {
    if accounts.len() < AnvilAccounts::MIN_REQUIRED_ACCOUNTS {
        anyhow::bail!(
            "Not enough accounts provided. Need at least {}, got {}",
            AnvilAccounts::MIN_REQUIRED_ACCOUNTS,
            accounts.len()
        );
    }

    let mut iter = accounts.into_iter();

    // Length was validated above, so these nexts are guaranteed to succeed.
    let result = AnvilAccounts {
        deployer: iter.next().context("missing deployer account")?,
        l1_fee_vault_recipient: iter
            .next()
            .context("missing l1_fee_vault_recipient account")?,
        sequencer_fee_vault_recipient: iter
            .next()
            .context("missing sequencer_fee_vault_recipient account")?,
        l1_proxy_admin_owner: iter
            .next()
            .context("missing l1_proxy_admin_owner account")?,
        l2_proxy_admin_owner: iter
            .next()
            .context("missing l2_proxy_admin_owner account")?,
        system_config_owner: iter.next().context("missing system_config_owner account")?,
        unsafe_block_signer: iter.next().context("missing unsafe_block_signer account")?,
        batcher: iter.next().context("missing batcher account")?,
        proposer: iter.next().context("missing proposer account")?,
        challenger: iter.next().context("missing challenger account")?,
        extra_accounts: iter.collect(),
    };

    Ok(result)
}

/// Default Docker image for op-deployer.
pub const DEFAULT_DOCKER_IMAGE: &str =
    "us-docker.pkg.dev/oplabs-tools-artifacts/images/op-deployer";
/// Default Docker tag for op-deployer.
pub const DEFAULT_DOCKER_TAG: &str = "v0.5.0-rc.2";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntentFile {
    config_type: String,
    // op_deployer_version: String,
    #[serde(rename = "l1ChainID")]
    l1_chain_id: u64,
    /// OPCM address - only present in "standard-overrides" mode
    #[serde(skip_serializing_if = "Option::is_none")]
    opcm_address: Option<String>,
    fund_dev_accounts: bool,
    l1_contracts_locator: String,
    l2_contracts_locator: String,
    /// Superchain roles - only present in "custom" mode
    #[serde(skip_serializing_if = "Option::is_none")]
    superchain_roles: Option<SuperchainRoles>,
    chains: Vec<ChainConfig>,
}

/// Superchain-level roles for custom intent type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SuperchainRoles {
    superchain_proxy_admin_owner: String,
    superchain_guardian: String,
    protocol_versions_owner: String,
    challenger: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChainConfig {
    id: String,
    base_fee_vault_recipient: String,
    l1_fee_vault_recipient: String,
    sequencer_fee_vault_recipient: String,
    // operator_fee_vault_recipient: String,
    // chain_fees_recipient: String,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpDeployerConfig {
    /// Docker image configuration for op-deployer.
    pub docker_image: DockerImage,
    /// Container name for op-deployer.
    pub container_name: String,
}

impl Default for OpDeployerConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
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
        host_config_path: &Path,
        container_config_path: &Path,
        cmd: Vec<String>,
    ) -> Result<(), anyhow::Error> {
        let mut service_config = ServiceConfig::new(image.clone()).cmd(cmd).bind(
            host_config_path,
            container_config_path,
            "rw",
        );

        // Get current user UID and GID to run container as non-root
        // This ensures files created by the container have the correct permissions
        // and can be rewritten by this process.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(host_config_path)
                .context("Failed to get metadata for host config path")?;
            service_config = service_config.user(format!("{}:{}", metadata.uid(), metadata.gid()));
        }

        let handler = docker
            .start_service(
                container_name,
                service_config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    wait_for_container: true,
                    start_options: None,
                    collect_logs: false,
                },
            )
            .await
            .context(format!(
                "Failed to start Op Deployer container: {}",
                container_name
            ))?;

        tracing::debug!(
            container_id = handler.container_id,
            container_name = handler.container_name,
            "Op Deployer container completed successfully",
        );

        Ok(())
    }

    async fn generate_intent_file(
        &self,
        docker: &mut KupDocker,
        _image: &DockerImage,
        host_config_path: &Path,
        _container_config_path: &Path,
        l1_chain_id: u64,
        l2_chain_id: u64,
    ) -> Result<PathBuf, anyhow::Error> {
        // Use "standard-overrides" for known chains with pre-deployed OPCM (Sepolia, Mainnet)
        // Use "custom" intent type for local/custom chains (no pre-deployed OPCM)
        let intent_type = if is_known_l1_chain(l1_chain_id) {
            "standard-overrides"
        } else {
            "custom"
        };

        self.generate_intent_file_with_type(
            docker,
            host_config_path,
            l1_chain_id,
            l2_chain_id,
            intent_type,
        )
        .await
    }

    /// Apply contract deployments to a live L1.
    ///
    /// SAFETY: Private key in command is from the well-known Anvil test mnemonic.
    async fn apply_contract_deployments(
        &self,
        docker: &mut KupDocker,
        image: &DockerImage,
        host_config_path: &Path,
        container_config_path: &Path,
        anvil_handler: &AnvilHandler,
    ) -> Result<(), anyhow::Error> {
        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "cat {container_config_path_str}/intent.toml && op-deployer --cache-dir {container_config_path_str}/.cache apply --workdir {container_config_path_str} --l1-rpc-url {l1_rpc_url} --private-key {private_key}",
                container_config_path_str = container_config_path.display().to_string(),
                l1_rpc_url = anvil_handler.l1_rpc_url.to_string(),
                private_key = anvil_handler.accounts.deployer.private_key.to_string(),
            ),
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

    /// Run `op-deployer inspect <config_type>` to generate a single config file.
    pub async fn inspect_config(
        &self,
        docker: &mut KupDocker,
        host_config_path: &Path,
        l2_chain_id: u64,
        config_type: &str,
    ) -> Result<PathBuf, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");
        let container_config_path_str = container_config_path.display().to_string();

        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "op-deployer --cache-dir {container_config_path_str}/.cache inspect {config_type} --workdir {container_config_path_str} {l2_chain_id} > {container_config_path_str}/{config_type}.json",
            ),
        ];

        self.run_docker_container(
            docker,
            &self.docker_image,
            &format!("{}-inspect-{}", self.container_name, config_type),
            host_config_path,
            &container_config_path,
            cmd,
        )
        .await?;

        let file_path = host_config_path.join(format!("{}.json", config_type));
        FsHandler::wait_for_file(&file_path, std::time::Duration::from_secs(120))
            .await
            .with_context(|| format!("{}.json was not created in time", config_type))?;

        tracing::debug!(?file_path, "{}.json generated", config_type);
        Ok(file_path)
    }

    async fn generate_config_files(
        &self,
        docker: &mut KupDocker,
        image: &DockerImage,
        host_config_path: &Path,
        container_config_path: &Path,
        l2_chain_id: u64,
    ) -> Result<(), anyhow::Error> {
        // Note: we can't use inspect_config here because this method takes
        // explicit image/container_config_path params from deploy_contracts.
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

        let genesis_file_path = host_config_path.join("genesis.json");
        let rollup_file_path = host_config_path.join("rollup.json");

        let (genesis_result, rollup_result) = tokio::join!(
            FsHandler::wait_for_file(&genesis_file_path, std::time::Duration::from_secs(120)),
            FsHandler::wait_for_file(&rollup_file_path, std::time::Duration::from_secs(120)),
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

    /// Deploy contracts to a live L1 (standard mode).
    ///
    /// Runs op-deployer init → update intent → apply → inspect to deploy contracts
    /// on a running Anvil instance and generate L2 config files.
    pub async fn deploy_contracts(
        &self,
        docker: &mut KupDocker,
        host_config_path: &Path,
        anvil_handler: &AnvilHandler,
        l1_chain_id: u64,
        l2_chain_id: u64,
    ) -> Result<(), anyhow::Error> {
        FsHandler::create_host_config_directory(&host_config_path.to_path_buf())?;

        self.generate_and_update_intent(
            docker,
            host_config_path,
            &anvil_handler.accounts,
            l1_chain_id,
            l2_chain_id,
        )
        .await?;

        // Apply the contract deployments to the live L1.
        let container_config_path = PathBuf::from("/data");
        self.apply_contract_deployments(
            docker,
            &self.docker_image,
            host_config_path,
            &container_config_path,
            anvil_handler,
        )
        .await
        .context("Failed to apply contract deployments")?;

        // Generate the L2 config files (genesis.json + rollup.json).
        self.generate_l2_config_files(docker, host_config_path, l2_chain_id)
            .await
            .context("Failed to generate L2 config files")?;

        Ok(())
    }

    /// Deploy contracts at genesis (genesis mode).
    ///
    /// Runs op-deployer with `--deployment-target genesis` which deploys contracts
    /// into an in-memory L1 state instead of a live chain. The resulting state dump
    /// in state.json can be used to construct an L1 genesis for Anvil.
    ///
    /// Does NOT generate L2 config files — those should be generated after Anvil
    /// starts by calling `generate_l2_config_files()`.
    pub async fn deploy_contracts_at_genesis(
        &self,
        docker: &mut KupDocker,
        host_config_path: &Path,
        accounts: &AnvilAccounts,
        l1_chain_id: u64,
        l2_chain_id: u64,
        timestamp: u64,
    ) -> Result<(), anyhow::Error> {
        FsHandler::create_host_config_directory(&host_config_path.to_path_buf())?;

        let config_file_path = self
            .generate_and_update_intent(
                docker,
                host_config_path,
                accounts,
                l1_chain_id,
                l2_chain_id,
            )
            .await?;

        // Add l1DevGenesisParams to the intent for genesis mode
        Self::add_l1_dev_genesis_params(&config_file_path, timestamp)
            .await
            .context("Failed to add l1DevGenesisParams to intent")?;

        // Apply with --deployment-target genesis (no L1 RPC needed).
        // SAFETY: Private key in command is from the well-known Anvil test mnemonic.
        let container_config_path = PathBuf::from("/data");
        let container_config_path_str = container_config_path.display().to_string();

        let cmd = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "cat {container_config_path_str}/intent.toml && op-deployer --cache-dir {container_config_path_str}/.cache apply --workdir {container_config_path_str} --deployment-target genesis --private-key {private_key}",
                private_key = format!("0x{}", hex::encode(&accounts.deployer.private_key)),
            ),
        ];

        self.run_docker_container(
            docker,
            &self.docker_image,
            &format!("{}-apply", self.container_name),
            host_config_path,
            &container_config_path,
            cmd,
        )
        .await
        .context("Failed to apply genesis contract deployments")?;

        Ok(())
    }

    /// Generate L2 config files (genesis.json + rollup.json) using op-deployer inspect.
    ///
    /// This can be called after either live or genesis deployment, as long as
    /// Anvil is running and state.json exists.
    pub async fn generate_l2_config_files(
        &self,
        docker: &mut KupDocker,
        host_config_path: &Path,
        l2_chain_id: u64,
    ) -> Result<(), anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

        self.generate_config_files(
            docker,
            &self.docker_image,
            host_config_path,
            &container_config_path,
            l2_chain_id,
        )
        .await
    }

    /// Generate an intent.toml using op-deployer init with a specific intent type.
    ///
    /// Used in snapshot mode to generate a standard-overrides intent file.
    pub async fn generate_intent_file_with_type(
        &self,
        docker: &mut KupDocker,
        host_config_path: &Path,
        l1_chain_id: u64,
        l2_chain_id: u64,
        intent_type: &str,
    ) -> Result<PathBuf, anyhow::Error> {
        let container_config_path = PathBuf::from("/data");

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
            intent_type.to_string(),
        ];

        self.run_docker_container(
            docker,
            &self.docker_image,
            &format!("{}-init", self.container_name),
            host_config_path,
            &container_config_path,
            cmd,
        )
        .await?;

        let config_file_path = host_config_path.join("intent.toml");
        FsHandler::wait_for_file(&config_file_path, std::time::Duration::from_secs(120))
            .await
            .context("intent.toml was not created in time")?;

        tracing::debug!(?config_file_path, "intent.toml generated");

        Ok(config_file_path)
    }

    /// Generate an intent file and update it with account addresses.
    /// Shared between live and genesis deployment modes.
    async fn generate_and_update_intent(
        &self,
        docker: &mut KupDocker,
        host_config_path: &Path,
        accounts: &AnvilAccounts,
        l1_chain_id: u64,
        l2_chain_id: u64,
    ) -> Result<PathBuf, anyhow::Error> {
        let config_file_path = self
            .generate_intent_file(
                docker,
                &self.docker_image,
                host_config_path,
                &PathBuf::from("/data"),
                l1_chain_id,
                l2_chain_id,
            )
            .await
            .context("Failed to generate intent file")?;

        Self::update_intent_with_accounts(&config_file_path, accounts)
            .await
            .context("Failed to update intent file with account addresses")?;

        tracing::debug!("Intent file updated with account addresses");
        Ok(config_file_path)
    }

    /// Add l1DevGenesisParams section to an intent.toml file.
    ///
    /// This is required for genesis deployment mode. It sets the timestamp and
    /// gas limit for the L1 genesis block, and pre-funds the role accounts.
    async fn add_l1_dev_genesis_params(
        intent_path: &Path,
        timestamp: u64,
    ) -> Result<(), anyhow::Error> {
        let content = tokio::fs::read_to_string(intent_path)
            .await
            .context("Failed to read intent file")?;

        let mut doc: toml::Value =
            toml::from_str(&content).context("Failed to parse intent file")?;

        // Add l1DevGenesisParams with genesis block parameters
        let genesis_params = toml::Value::Table({
            let mut table = toml::map::Map::new();
            let mut block_params = toml::map::Map::new();
            block_params.insert(
                "timestamp".to_string(),
                toml::Value::Integer(i64::try_from(timestamp).context("timestamp overflows i64")?),
            );
            block_params.insert("gasLimit".to_string(), toml::Value::Integer(30_000_000));
            table.insert("blockParams".to_string(), toml::Value::Table(block_params));
            table
        });

        if let Some(root) = doc.as_table_mut() {
            root.insert("l1DevGenesisParams".to_string(), genesis_params);
        }

        let updated_content = toml::to_string_pretty(&doc)
            .context("Failed to serialize intent file with l1DevGenesisParams")?;

        tokio::fs::write(intent_path, updated_content)
            .await
            .context("Failed to write updated intent file")?;

        tracing::debug!(timestamp, "Added l1DevGenesisParams to intent.toml");
        Ok(())
    }

    /// Updates the intent.toml file with account addresses from Anvil.
    ///
    /// This function replaces the placeholder addresses in the intent file with
    /// actual addresses from the accounts generated by Anvil at startup.
    async fn update_intent_with_accounts(
        intent_path: &PathBuf,
        accounts: &AnvilAccounts,
    ) -> Result<(), anyhow::Error> {
        // Read the intent file
        let content = tokio::fs::read_to_string(intent_path)
            .await
            .context("Failed to read intent file")?;

        // Parse the TOML
        let mut intent: IntentFile =
            toml::from_str(&content).context("Failed to parse intent file as TOML")?;

        // Helper function to format address as lowercase hex string with 0x prefix
        let format_address =
            |bytes: &[u8]| -> String { format!("0x{}", hex::encode(bytes).to_lowercase()) };

        // Update the roles with account addresses using named fields
        for chain in &mut intent.chains {
            // The deployer account is also used as base_fee_vault_recipient
            chain.base_fee_vault_recipient = format_address(&accounts.deployer.address);
            chain.l1_fee_vault_recipient = format_address(&accounts.l1_fee_vault_recipient.address);
            chain.sequencer_fee_vault_recipient =
                format_address(&accounts.sequencer_fee_vault_recipient.address);

            chain.roles.l1_proxy_admin_owner =
                format_address(&accounts.l1_proxy_admin_owner.address);
            chain.roles.l2_proxy_admin_owner =
                format_address(&accounts.l2_proxy_admin_owner.address);
            chain.roles.system_config_owner = format_address(&accounts.system_config_owner.address);
            chain.roles.unsafe_block_signer = format_address(&accounts.unsafe_block_signer.address);

            chain.roles.batcher = format_address(&accounts.batcher.address);
            chain.roles.proposer = format_address(&accounts.proposer.address);
            chain.roles.challenger = format_address(&accounts.challenger.address);

            // Set EIP-1559 parameters if not already set (custom intent type has zeros)
            if chain.eip1559_denominator == 0 {
                chain.eip1559_denominator = 50; // OP Mainnet default
            }
            if chain.eip1559_denominator_canyon == 0 {
                chain.eip1559_denominator_canyon = 250; // Canyon upgrade default
            }
            if chain.eip1559_elasticity == 0 {
                chain.eip1559_elasticity = 6; // Standard elasticity
            }

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

        // Update superchain roles if present (custom intent type)
        if let Some(ref mut superchain_roles) = intent.superchain_roles {
            superchain_roles.superchain_proxy_admin_owner =
                format_address(&accounts.deployer.address);
            superchain_roles.superchain_guardian = format_address(&accounts.deployer.address);
            superchain_roles.protocol_versions_owner = format_address(&accounts.deployer.address);
            superchain_roles.challenger = format_address(&accounts.challenger.address);

            tracing::debug!(
                superchain_proxy_admin_owner = superchain_roles.superchain_proxy_admin_owner,
                superchain_guardian = superchain_roles.superchain_guardian,
                protocol_versions_owner = superchain_roles.protocol_versions_owner,
                challenger = superchain_roles.challenger,
                "Updated superchain roles with Anvil account addresses"
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
