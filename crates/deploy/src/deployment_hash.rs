use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::Deployer;

/// Configuration parameters that affect contract deployment.
///
/// This struct contains only the deployment-relevant parameters that, when changed,
/// require redeploying the L1 contracts. Runtime-only parameters (like block_time,
/// Docker images, port mappings) are explicitly excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentConfigHash {
    /// L1 chain ID - determines which OPCM contracts are used
    pub l1_chain_id: u64,
    /// L2 chain ID - embedded in all deployed contracts and genesis
    pub l2_chain_id: u64,
    /// L1 fork URL - changes which L1 state is forked
    pub fork_url: Option<String>,
    /// L1 fork block number - changes L1 fork point
    pub fork_block_number: Option<u64>,
    /// Genesis timestamp - affects genesis timestamp alignment
    pub timestamp: Option<u64>,

    // EIP-1559 parameters (currently hardcoded in op-deployer intent generation)
    // These are included for future extensibility when they become configurable
    pub eip1559_denominator: u64,
    pub eip1559_denominator_canyon: u64,
    pub eip1559_elasticity: u64,
}

impl DeploymentConfigHash {
    /// Extract deployment-relevant configuration from a Deployer instance.
    pub fn from_deployer(deployer: &Deployer) -> Self {
        Self {
            l1_chain_id: deployer.l1_chain_id,
            l2_chain_id: deployer.l2_chain_id,
            fork_url: deployer.anvil.fork_url.clone(),
            fork_block_number: deployer.anvil.fork_block_number,
            timestamp: deployer.anvil.timestamp,
            // Hardcoded values from op-deployer intent generation
            // See: crates/deploy/src/services/op_deployer/mod.rs:231-233
            eip1559_denominator: 50,
            eip1559_denominator_canyon: 250,
            eip1559_elasticity: 6,
        }
    }

    /// Compute a SHA-256 hash of this configuration.
    ///
    /// The hash is deterministic - the same configuration always produces the same hash.
    /// The configuration is serialized to JSON (with sorted keys) before hashing to ensure
    /// consistent ordering.
    pub fn compute_hash(&self) -> String {
        // Serialize to JSON with sorted keys for consistent hashing
        let json = serde_json::to_string(self)
            .expect("DeploymentConfigHash serialization should never fail");

        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        let result = hasher.finalize();

        // Return hex-encoded hash
        hex::encode(result)
    }
}

/// Deployment version metadata stored alongside deployment artifacts.
///
/// This file is saved to `{outdata}/l2-stack/.deployment-version.json` after successful
/// contract deployment and used to detect when configuration changes require redeployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentVersion {
    /// SHA-256 hash of the deployment configuration
    pub config_hash: String,
    /// Unix timestamp when this deployment was created
    pub deployed_at: u64,
    /// Kupcake version that created this deployment
    pub kupcake_version: String,
}

impl DeploymentVersion {
    /// Create a new DeploymentVersion with the given config hash.
    ///
    /// The timestamp is set to the current system time, and the kupcake_version is
    /// set from the CARGO_PKG_VERSION environment variable.
    pub fn new(config_hash: String) -> Self {
        Self {
            config_hash,
            deployed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("System time should be after Unix epoch")
                .as_secs(),
            kupcake_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Save this version metadata to a file.
    ///
    /// The file is written as formatted JSON for human readability.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize deployment version")?;

        std::fs::write(path, json).context(format!(
            "Failed to write deployment version to {}",
            path.display()
        ))?;

        Ok(())
    }

    /// Load version metadata from a file.
    ///
    /// Returns an error if the file doesn't exist, is malformed, or cannot be read.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!("Deployment version file does not exist: {}", path.display());
        }

        let content = std::fs::read_to_string(path).context(format!(
            "Failed to read deployment version from {}",
            path.display()
        ))?;

        let version: Self =
            serde_json::from_str(&content).context("Failed to parse deployment version JSON")?;

        Ok(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    #[test]
    fn test_hash_determinism() {
        let config = DeploymentConfigHash {
            l1_chain_id: 11155111,
            l2_chain_id: 42069,
            fork_url: Some("https://ethereum-sepolia-rpc.publicnode.com".to_string()),
            fork_block_number: Some(1000000),
            timestamp: Some(1737316800),
            eip1559_denominator: 50,
            eip1559_denominator_canyon: 250,
            eip1559_elasticity: 6,
        };

        let hash1 = config.compute_hash();
        let hash2 = config.compute_hash();

        assert_eq!(hash1, hash2, "Hash should be deterministic");
        assert_eq!(hash1.len(), 64, "SHA-256 hash should be 64 hex characters");
    }

    #[test]
    fn test_hash_changes_with_l1_chain_id() {
        let config1 = DeploymentConfigHash {
            l1_chain_id: 11155111,
            l2_chain_id: 42069,
            fork_url: Some("https://ethereum-sepolia-rpc.publicnode.com".to_string()),
            fork_block_number: Some(1000000),
            timestamp: Some(1737316800),
            eip1559_denominator: 50,
            eip1559_denominator_canyon: 250,
            eip1559_elasticity: 6,
        };

        let mut config2 = config1.clone();
        config2.l1_chain_id = 1; // mainnet

        assert_ne!(
            config1.compute_hash(),
            config2.compute_hash(),
            "Hash should change when l1_chain_id changes"
        );
    }

    #[test]
    fn test_hash_changes_with_l2_chain_id() {
        let config1 = DeploymentConfigHash {
            l1_chain_id: 11155111,
            l2_chain_id: 42069,
            fork_url: Some("https://ethereum-sepolia-rpc.publicnode.com".to_string()),
            fork_block_number: Some(1000000),
            timestamp: Some(1737316800),
            eip1559_denominator: 50,
            eip1559_denominator_canyon: 250,
            eip1559_elasticity: 6,
        };

        let mut config2 = config1.clone();
        config2.l2_chain_id = 12345;

        assert_ne!(
            config1.compute_hash(),
            config2.compute_hash(),
            "Hash should change when l2_chain_id changes"
        );
    }

    #[test]
    fn test_hash_changes_with_fork_url() {
        let config1 = DeploymentConfigHash {
            l1_chain_id: 11155111,
            l2_chain_id: 42069,
            fork_url: Some("https://ethereum-sepolia-rpc.publicnode.com".to_string()),
            fork_block_number: Some(1000000),
            timestamp: Some(1737316800),
            eip1559_denominator: 50,
            eip1559_denominator_canyon: 250,
            eip1559_elasticity: 6,
        };

        let mut config2 = config1.clone();
        config2.fork_url = Some("https://eth-mainnet.g.alchemy.com/v2/demo".to_string());

        assert_ne!(
            config1.compute_hash(),
            config2.compute_hash(),
            "Hash should change when fork_url changes"
        );
    }

    #[test]
    fn test_hash_changes_with_fork_block_number() {
        let config1 = DeploymentConfigHash {
            l1_chain_id: 11155111,
            l2_chain_id: 42069,
            fork_url: Some("https://ethereum-sepolia-rpc.publicnode.com".to_string()),
            fork_block_number: Some(1000000),
            timestamp: Some(1737316800),
            eip1559_denominator: 50,
            eip1559_denominator_canyon: 250,
            eip1559_elasticity: 6,
        };

        let mut config2 = config1.clone();
        config2.fork_block_number = Some(2000000);

        assert_ne!(
            config1.compute_hash(),
            config2.compute_hash(),
            "Hash should change when fork_block_number changes"
        );
    }

    #[test]
    fn test_hash_changes_with_timestamp() {
        let config1 = DeploymentConfigHash {
            l1_chain_id: 11155111,
            l2_chain_id: 42069,
            fork_url: Some("https://ethereum-sepolia-rpc.publicnode.com".to_string()),
            fork_block_number: Some(1000000),
            timestamp: Some(1737316800),
            eip1559_denominator: 50,
            eip1559_denominator_canyon: 250,
            eip1559_elasticity: 6,
        };

        let mut config2 = config1.clone();
        config2.timestamp = Some(1737320000);

        assert_ne!(
            config1.compute_hash(),
            config2.compute_hash(),
            "Hash should change when timestamp changes"
        );
    }

    #[test]
    fn test_hash_changes_with_eip1559_params() {
        let config1 = DeploymentConfigHash {
            l1_chain_id: 11155111,
            l2_chain_id: 42069,
            fork_url: Some("https://ethereum-sepolia-rpc.publicnode.com".to_string()),
            fork_block_number: Some(1000000),
            timestamp: Some(1737316800),
            eip1559_denominator: 50,
            eip1559_denominator_canyon: 250,
            eip1559_elasticity: 6,
        };

        let mut config2 = config1.clone();
        config2.eip1559_denominator = 100;

        assert_ne!(
            config1.compute_hash(),
            config2.compute_hash(),
            "Hash should change when EIP-1559 parameters change"
        );
    }

    #[test]
    fn test_version_save_and_load() {
        let temp_dir = TempDir::new("kupcake-test").expect("Failed to create temp dir");
        let version_path = temp_dir.path().join(".deployment-version.json");

        let original_version = DeploymentVersion {
            config_hash: "a7f3c2b1d8e5f4a9b2c3d4e5f6a7b8c9".to_string(),
            deployed_at: 1737316800,
            kupcake_version: "0.1.0".to_string(),
        };

        // Save
        original_version
            .save_to_file(&version_path)
            .expect("Failed to save version");

        // Load
        let loaded_version =
            DeploymentVersion::load_from_file(&version_path).expect("Failed to load version");

        assert_eq!(
            original_version, loaded_version,
            "Loaded version should match original"
        );
    }

    #[test]
    fn test_version_load_missing_file() {
        let temp_dir = TempDir::new("kupcake-test").expect("Failed to create temp dir");
        let version_path = temp_dir.path().join("nonexistent.json");

        let result = DeploymentVersion::load_from_file(&version_path);
        assert!(result.is_err(), "Loading missing file should return error");
    }

    #[test]
    fn test_version_load_corrupted_file() {
        let temp_dir = TempDir::new("kupcake-test").expect("Failed to create temp dir");
        let version_path = temp_dir.path().join(".deployment-version.json");

        // Write corrupted JSON
        std::fs::write(&version_path, "{ invalid json }").expect("Failed to write corrupted file");

        let result = DeploymentVersion::load_from_file(&version_path);
        assert!(
            result.is_err(),
            "Loading corrupted file should return error"
        );
    }

    #[test]
    fn test_from_deployer() {
        use crate::{
            AnvilConfig, KupDockerConfig, L2StackBuilder, MonitoringConfig, OpDeployerConfig,
        };
        use std::path::PathBuf;

        let deployer = Deployer {
            l1_chain_id: 11155111,
            l2_chain_id: 42069,
            outdata: PathBuf::from("/tmp/test"),
            anvil: AnvilConfig {
                fork_url: Some("https://ethereum-sepolia-rpc.publicnode.com".to_string()),
                fork_block_number: Some(1000000),
                timestamp: Some(1737316800),
                ..Default::default()
            },
            op_deployer: OpDeployerConfig::default(),
            docker: KupDockerConfig {
                net_name: "test-net".to_string(),
                no_cleanup: false,
                publish_all_ports: false,
            },
            l2_stack: L2StackBuilder::default(),
            monitoring: MonitoringConfig::default(),
            dashboards_path: None,
            detach: false,
            snapshot: None,
            copy_snapshot: false,
        };

        let config_hash = DeploymentConfigHash::from_deployer(&deployer);

        assert_eq!(config_hash.l1_chain_id, 11155111);
        assert_eq!(config_hash.l2_chain_id, 42069);
        assert_eq!(
            config_hash.fork_url,
            Some("https://ethereum-sepolia-rpc.publicnode.com".to_string())
        );
        assert_eq!(config_hash.fork_block_number, Some(1000000));
        assert_eq!(config_hash.timestamp, Some(1737316800));
        assert_eq!(config_hash.eip1559_denominator, 50);
        assert_eq!(config_hash.eip1559_denominator_canyon, 250);
        assert_eq!(config_hash.eip1559_elasticity, 6);
    }
}
