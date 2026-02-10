//! Faucet module for bridging ETH from L1 (Anvil) to L2 via OptimismPortal deposit.

use std::path::Path;

use anyhow::{Context, Result};
use bollard::Docker;
use serde_json::Value;

use crate::{Deployer, health::build_host_rpc_url, rpc};

/// Result of a faucet deposit operation.
#[derive(Debug)]
pub struct FaucetResult {
    /// L1 transaction hash.
    pub l1_tx_hash: String,
    /// L2 balance after deposit (if `--wait` was used).
    pub l2_balance: Option<String>,
}

/// Execute a faucet deposit: bridge ETH from L1 to L2 via OptimismPortal.
///
/// Sends `amount_eth` from the Anvil deployer account (index 0) to the
/// `OptimismPortalProxy` contract, which creates a deposit transaction on L2
/// that mints the corresponding ETH to `to_address`.
pub async fn faucet_deposit(
    deployer: &Deployer,
    to_address: &str,
    amount_eth: f64,
    wait: bool,
) -> Result<FaucetResult> {
    validate_address(to_address)?;

    let docker = Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker daemon")?;
    let client = rpc::create_client()?;

    let deployer_address = load_deployer_address(&deployer.outdata)?;
    let portal_address = load_optimism_portal_address(&deployer.outdata)?;

    let l1_url = build_host_rpc_url(&docker, &deployer.anvil.container_name, deployer.anvil.port)
        .await
        .context("Failed to build L1 RPC URL - is Anvil running?")?;

    let amount_wei = eth_to_wei(amount_eth);
    let calldata = encode_deposit_transaction(to_address, amount_wei, 100_000);
    let value_hex = format!("0x{:x}", amount_wei);

    let tx_hash: String = rpc::json_rpc_call(
        &client,
        &l1_url,
        "eth_sendTransaction",
        vec![serde_json::json!({
            "from": deployer_address,
            "to": portal_address,
            "value": value_hex,
            "data": calldata,
            "gas": "0x100000"
        })],
    )
    .await
    .context("Failed to send deposit transaction")?;

    tracing::info!(tx_hash = %tx_hash, "Deposit transaction sent on L1");

    let l2_balance = if wait {
        Some(wait_for_l2_deposit(&docker, &client, deployer, to_address, 120).await?)
    } else {
        None
    };

    Ok(FaucetResult {
        l1_tx_hash: tx_hash,
        l2_balance,
    })
}

/// Wait for an L2 deposit by polling `eth_getBalance` until the balance changes.
async fn wait_for_l2_deposit(
    docker: &Docker,
    client: &reqwest::Client,
    deployer: &Deployer,
    to_address: &str,
    timeout_secs: u64,
) -> Result<String> {
    let seq = &deployer.l2_stack.sequencers[0];
    let l2_url = build_host_rpc_url(
        docker,
        &seq.op_reth.container_name,
        seq.op_reth.http_port,
    )
    .await
    .context("Failed to build L2 RPC URL - is the sequencer running?")?;

    let initial_balance: String = rpc::json_rpc_call(
        client,
        &l2_url,
        "eth_getBalance",
        vec![
            serde_json::json!(to_address),
            serde_json::json!("latest"),
        ],
    )
    .await
    .context("Failed to get initial L2 balance")?;

    tracing::info!(initial_balance = %initial_balance, "Waiting for L2 deposit...");

    let client_ref = client.clone();
    let l2_url_ref = l2_url.clone();
    let initial_ref = initial_balance.clone();
    let to_ref = to_address.to_string();

    rpc::wait_until_ready("L2 deposit", timeout_secs, || {
        let client = client_ref.clone();
        let l2_url = l2_url_ref.clone();
        let initial = initial_ref.clone();
        let to = to_ref.clone();
        async move {
            let balance: String = rpc::json_rpc_call(
                &client,
                &l2_url,
                "eth_getBalance",
                vec![serde_json::json!(to), serde_json::json!("latest")],
            )
            .await?;

            if balance != initial {
                Ok(())
            } else {
                anyhow::bail!("Balance unchanged: {}", balance)
            }
        }
    })
    .await?;

    let final_balance: String = rpc::json_rpc_call(
        client,
        &l2_url,
        "eth_getBalance",
        vec![
            serde_json::json!(to_address),
            serde_json::json!("latest"),
        ],
    )
    .await
    .context("Failed to get final L2 balance")?;

    Ok(final_balance)
}

/// Load the deployer address (account index 0) from `anvil.json`.
fn load_deployer_address(outdata: &Path) -> Result<String> {
    let anvil_path = outdata.join("anvil/anvil.json");
    let content = std::fs::read_to_string(&anvil_path)
        .with_context(|| format!("Failed to read {}", anvil_path.display()))?;
    let data: Value =
        serde_json::from_str(&content).context("Failed to parse anvil.json")?;

    data["available_accounts"][0]
        .as_str()
        .context("Deployer address (account 0) not found in anvil.json")
        .map(String::from)
}

/// Load the `OptimismPortalProxy` address from `state.json`.
fn load_optimism_portal_address(outdata: &Path) -> Result<String> {
    let state_path = outdata.join("l2-stack/state.json");
    let content = std::fs::read_to_string(&state_path)
        .with_context(|| format!("Failed to read {}", state_path.display()))?;
    let data: Value =
        serde_json::from_str(&content).context("Failed to parse state.json")?;

    data["opChainDeployments"][0]["OptimismPortalProxy"]
        .as_str()
        .context("OptimismPortalProxy address not found in state.json")
        .map(String::from)
}

/// Validate an Ethereum address format (0x-prefixed, 40 hex chars).
fn validate_address(addr: &str) -> Result<()> {
    if !addr.starts_with("0x") || addr.len() != 42 {
        anyhow::bail!(
            "Invalid address format: expected 0x-prefixed 40 hex chars, got '{}'",
            addr
        );
    }

    if !addr[2..].chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!(
            "Invalid address: contains non-hex characters: '{}'",
            addr
        );
    }

    Ok(())
}

/// Convert ETH amount (f64) to wei.
///
/// Rounds to gwei precision (9 decimal places) to avoid floating-point noise,
/// then scales to wei. Gwei precision is more than sufficient for a dev faucet.
fn eth_to_wei(eth: f64) -> u128 {
    let gwei = (eth * 1e9).round() as u128;
    gwei * 1_000_000_000u128
}

/// ABI-encode a `depositTransaction` call.
///
/// Function: `depositTransaction(address,uint256,uint64,bool,bytes)`
/// Selector: `0xe9e05c42`
fn encode_deposit_transaction(to: &str, value: u128, gas_limit: u64) -> String {
    let selector = "e9e05c42";

    let addr = to.trim_start_matches("0x").to_lowercase();
    let addr_padded = format!("{:0>64}", addr);

    let value_hex = format!("{:064x}", value);
    let gas_limit_hex = format!("{:064x}", gas_limit);
    let is_creation_hex = format!("{:064x}", 0u64);
    // Offset to the `bytes` data: 5 head words * 32 bytes = 160 = 0xa0
    // (4 static params + 1 offset pointer for the dynamic `bytes` param)
    let data_offset_hex = format!("{:064x}", 160u64);
    // Empty bytes: length = 0
    let data_length_hex = format!("{:064x}", 0u64);

    format!(
        "0x{}{}{}{}{}{}{}",
        selector,
        addr_padded,
        value_hex,
        gas_limit_hex,
        is_creation_hex,
        data_offset_hex,
        data_length_hex,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_address_valid() {
        assert!(validate_address("0x70997970C51812dc3A010C7d01b50e0d17dc79C8").is_ok());
        assert!(validate_address("0x0000000000000000000000000000000000000000").is_ok());
        assert!(validate_address("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef").is_ok());
    }

    #[test]
    fn test_validate_address_invalid() {
        assert!(validate_address("0x1234").is_err());
        assert!(validate_address("1234567890abcdef1234567890abcdef12345678").is_err());
        assert!(validate_address("0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG").is_err());
        assert!(validate_address("").is_err());
    }

    #[test]
    fn test_eth_to_wei() {
        assert_eq!(eth_to_wei(1.0), 1_000_000_000_000_000_000);
        assert_eq!(eth_to_wei(0.1), 100_000_000_000_000_000);
        assert_eq!(eth_to_wei(10.0), 10_000_000_000_000_000_000);
        // Verify precision: 0.7 ETH = 700000000000000000 wei
        assert_eq!(eth_to_wei(0.7), 700_000_000_000_000_000);
    }

    #[test]
    fn test_encode_deposit_transaction() {
        let calldata = encode_deposit_transaction(
            "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
            0,
            100_000,
        );

        // Should start with selector
        assert!(calldata.starts_with("0xe9e05c42"));

        // Total length: "0x" + 8 (selector) + 6 * 64 (6 words of 32 bytes) = 394
        assert_eq!(calldata.len(), 394);

        // Address should be lowercase and left-padded in the first word after selector
        assert!(calldata[10..74]
            .eq("00000000000000000000000070997970c51812dc3a010c7d01b50e0d17dc79c8"));
    }

    #[test]
    fn test_encode_deposit_transaction_with_value() {
        let calldata = encode_deposit_transaction(
            "0x0000000000000000000000000000000000000001",
            1_000_000_000_000_000_000, // 1 ETH in wei
            21_000,
        );

        assert!(calldata.starts_with("0xe9e05c42"));
        assert_eq!(calldata.len(), 394);

        // Check value word (second 32-byte word after selector)
        let value_word = &calldata[74..138];
        assert_eq!(
            value_word,
            "0000000000000000000000000000000000000000000000000de0b6b3a7640000"
        );
    }
}
