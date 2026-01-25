//! RPC client helpers for op-reth.

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;

use crate::rpc;

use super::OpRethHandler;

/// Sync status response from op-reth.
#[derive(Debug, Clone, Deserialize)]
pub struct OpRethStatus {
    /// Whether the node is currently syncing.
    pub is_syncing: bool,
    /// Current block number.
    pub block_number: u64,
    /// Sync progress if syncing.
    pub sync_progress: Option<EthSyncProgress>,
}

/// Ethereum sync progress from eth_syncing.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EthSyncProgress {
    /// Starting block number.
    pub starting_block: u64,
    /// Current block number being synced.
    pub current_block: u64,
    /// Highest block number known.
    pub highest_block: u64,
}

impl OpRethHandler {
    /// Query the sync status from this op-reth node.
    ///
    /// Uses eth_syncing and eth_blockNumber to determine the node's sync state.
    pub async fn sync_status(&self) -> Result<OpRethStatus, anyhow::Error> {
        let rpc_url = self
            .http_host_url
            .as_ref()
            .context("HTTP RPC URL not published to host")?;

        let client = rpc::create_client()?;

        // Get eth_syncing status - returns false or sync progress object
        let syncing_result: Value =
            rpc::json_rpc_call(&client, rpc_url.as_str(), "eth_syncing", vec![]).await?;

        // eth_syncing returns:
        // - `false` (boolean) when not syncing
        // - an object with sync progress when syncing
        let is_syncing = match &syncing_result {
            Value::Bool(false) => false,
            _ => true, // Either object (syncing) or unexpected format
        };

        let sync_progress = if is_syncing {
            serde_json::from_value(syncing_result.clone()).ok()
        } else {
            None
        };

        // Get current block number
        let block_hex: String =
            rpc::json_rpc_call(&client, rpc_url.as_str(), "eth_blockNumber", vec![]).await?;

        let block_number = u64::from_str_radix(block_hex.trim_start_matches("0x"), 16)
            .context("Failed to parse block number")?;

        Ok(OpRethStatus {
            is_syncing,
            block_number,
            sync_progress,
        })
    }

    /// Wait for this op-reth node to be ready by polling the RPC endpoint.
    ///
    /// Returns Ok(()) when the node responds successfully, or an error after timeout.
    pub async fn wait_until_ready(&self, timeout_secs: u64) -> Result<(), anyhow::Error> {
        rpc::wait_until_ready(&self.container_name, timeout_secs, || async {
            self.sync_status().await.map(|_| ())
        })
        .await
    }
}
