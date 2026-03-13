//! L1 genesis extraction from op-deployer state.
//!
//! After op-deployer runs with `--deployment-target genesis`, the state.json
//! contains an `l1StateDump` field with gzipped, base64-encoded account allocations.
//! This module extracts those allocations and constructs an L1 genesis.json
//! suitable for Anvil's `--init` flag.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use flate2::read::GzDecoder;

/// Default L1 genesis gas limit (matches op-deployer's `defaultGasLimit`).
const DEFAULT_GAS_LIMIT: u64 = 30_000_000;

/// Timeout (seconds) for waiting for Anvil to serve genesis block during rollup.json patching.
const ANVIL_GENESIS_READY_TIMEOUT_SECS: u64 = 15;

/// Extract the L1 genesis from op-deployer's state.json and write it to a file.
///
/// The state.json produced by `op-deployer apply --deployment-target genesis` contains
/// an `l1StateDump` field: a base64-encoded, gzipped JSON object of account allocations.
///
/// This function:
/// 1. Reads state.json and extracts the `l1StateDump` field
/// 2. Base64-decodes and gzip-decompresses the allocations
/// 3. Constructs a Geth-compatible genesis.json with chain config matching
///    op-deployer's `NewL1GenesisMinimal` (all forks through Cancun active at genesis)
/// 4. Writes the result to `{output_dir}/l1-genesis.json`
pub fn extract_l1_genesis(
    state_json_path: &Path,
    l1_chain_id: u64,
    timestamp: u64,
    output_dir: &Path,
) -> Result<std::path::PathBuf> {
    #[derive(serde::Deserialize)]
    struct StateDump {
        #[serde(rename = "l1StateDump")]
        l1_state_dump: String,
    }

    let state_content = std::fs::read_to_string(state_json_path).with_context(|| {
        format!(
            "Failed to read state.json from {}",
            state_json_path.display()
        )
    })?;

    let state: StateDump = serde_json::from_str(&state_content)
        .context("Failed to parse state.json (missing 'l1StateDump' field?)")?;

    let l1_state_dump_b64 = &state.l1_state_dump;

    // Base64 decode
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(l1_state_dump_b64)
        .context("Failed to base64-decode l1StateDump")?;

    // Gzip decompress
    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut decompressed = String::new();
    decoder
        .read_to_string(&mut decompressed)
        .context("Failed to gzip-decompress l1StateDump")?;

    // Parse as JSON to extract the accounts map
    let allocs: serde_json::Value = serde_json::from_str(&decompressed)
        .context("Failed to parse decompressed l1StateDump as JSON")?;

    // The allocs may be wrapped in an "accounts" key (ForgeAllocs format)
    // or may be a flat map of address -> account. Support both formats.
    let accounts = allocs.get("accounts").unwrap_or(&allocs);

    // Construct the L1 genesis.json matching op-deployer's NewL1GenesisMinimal config.
    // All forks through Cancun are active at genesis (block 0 / time 0).
    let zero = "0x0";
    let genesis = serde_json::json!({
        "config": {
            "chainId": l1_chain_id,
            "homesteadBlock": 0,
            "eip150Block": 0,
            "eip155Block": 0,
            "eip158Block": 0,
            "byzantiumBlock": 0,
            "constantinopleBlock": 0,
            "petersburgBlock": 0,
            "istanbulBlock": 0,
            "muirGlacierBlock": 0,
            "berlinBlock": 0,
            "londonBlock": 0,
            "arrowGlacierBlock": 0,
            "grayGlacierBlock": 0,
            "mergeNetsplitBlock": 0,
            "terminalTotalDifficulty": 0,
            "terminalTotalDifficultyPassed": true,
            "shanghaiTime": 0,
            "cancunTime": 0,
            "blobSchedule": {
                "cancun": {
                    "target": 3,
                    "max": 6,
                    "baseFeeUpdateFraction": 3338477
                },
                "prague": {
                    "target": 6,
                    "max": 9,
                    "baseFeeUpdateFraction": 5007716
                }
            }
        },
        "timestamp": format!("0x{:x}", timestamp),
        "gasLimit": format!("0x{:x}", DEFAULT_GAS_LIMIT),
        "difficulty": zero,
        "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
        "coinbase": "0x0000000000000000000000000000000000000000",
        "nonce": "0x0000000000000000",
        "extraData": "0x",
        "alloc": accounts,
        "number": zero,
        "gasUsed": zero,
        "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
        "baseFeePerGas": "0x3b9aca00",
        "excessBlobGas": zero,
        "blobGasUsed": zero
    });

    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory {}", output_dir.display()))?;

    let output_path = output_dir.join("l1-genesis.json");
    // Use compact JSON — this file is only consumed by Anvil, not humans,
    // and the alloc section can be large.
    let genesis_str =
        serde_json::to_string(&genesis).context("Failed to serialize L1 genesis JSON")?;

    std::fs::write(&output_path, genesis_str)
        .with_context(|| format!("Failed to write L1 genesis to {}", output_path.display()))?;

    tracing::info!(
        path = %output_path.display(),
        l1_chain_id,
        timestamp,
        "L1 genesis file created from op-deployer state dump"
    );

    Ok(output_path)
}

/// Patch rollup.json with the actual L1 genesis block hash from Anvil.
///
/// Anvil's `--init` flag has a known bug where the genesis block hash is computed
/// from an empty state root instead of the real state root after applying the alloc.
/// This causes a mismatch between the L1 genesis hash in rollup.json (computed by
/// op-deployer using go-ethereum's `ToBlock()`) and what Anvil actually serves.
///
/// This function queries Anvil's block 0, extracts the actual hash, and patches
/// `genesis.l1.hash` in rollup.json to match.
pub async fn patch_rollup_l1_genesis_hash(
    rollup_json_path: &Path,
    anvil_url: &url::Url,
) -> Result<()> {
    let client = crate::rpc::create_client()?;
    let url = anvil_url.as_str().to_string();

    // Wait for Anvil to be ready (it may not have opened its RPC port yet,
    // especially when reusing existing deployment artifacts on restart).
    crate::rpc::wait_until_ready(
        "Anvil genesis block",
        ANVIL_GENESIS_READY_TIMEOUT_SECS,
        || {
            let client = client.clone();
            let url = url.clone();
            async move {
                crate::rpc::json_rpc_call::<serde_json::Value>(
                    &client,
                    &url,
                    "eth_getBlockByNumber",
                    vec![serde_json::json!("0x0"), serde_json::json!(false)],
                )
                .await
                .map(|_| ())
            }
        },
    )
    .await
    .context(format!(
        "Anvil not ready after {}s",
        ANVIL_GENESIS_READY_TIMEOUT_SECS
    ))?;

    // Now fetch the actual genesis block hash
    let block: serde_json::Value = crate::rpc::json_rpc_call(
        &client,
        anvil_url.as_str(),
        "eth_getBlockByNumber",
        vec![serde_json::json!("0x0"), serde_json::json!(false)],
    )
    .await
    .context("Failed to query Anvil for genesis block")?;

    let actual_hash = block
        .get("hash")
        .and_then(|h| h.as_str())
        .context("Anvil response missing hash for block 0")?;

    // Read and patch rollup.json
    let content = std::fs::read_to_string(rollup_json_path)
        .with_context(|| format!("Failed to read {}", rollup_json_path.display()))?;

    let mut rollup: serde_json::Value =
        serde_json::from_str(&content).context("Failed to parse rollup.json")?;

    let original_hash = rollup
        .pointer("/genesis/l1/hash")
        .and_then(|h| h.as_str())
        .unwrap_or("unknown")
        .to_string();

    if let Some(l1_hash) = rollup.pointer_mut("/genesis/l1/hash") {
        *l1_hash = serde_json::Value::String(actual_hash.to_string());
    } else {
        anyhow::bail!("rollup.json missing genesis.l1.hash field");
    }

    let patched =
        serde_json::to_string_pretty(&rollup).context("Failed to serialize patched rollup.json")?;

    std::fs::write(rollup_json_path, patched)
        .with_context(|| format!("Failed to write patched {}", rollup_json_path.display()))?;

    tracing::info!(
        original = %original_hash,
        patched = %actual_hash,
        "Patched rollup.json L1 genesis hash (Anvil --init bug workaround)"
    );

    Ok(())
}
