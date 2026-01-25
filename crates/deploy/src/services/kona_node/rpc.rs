//! RPC client helpers for kona-node.

use anyhow::Context;
use serde::Deserialize;

use crate::rpc;

use super::KonaNodeHandler;

/// Sync status response from kona-node optimism_syncStatus RPC.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncStatus {
    /// Unsafe L2 head (latest block, may be reorged).
    pub unsafe_l2: BlockRef,
    /// Safe L2 head (derived from L1, unlikely to reorg).
    pub safe_l2: BlockRef,
    /// Finalized L2 head (finalized on L1, will not reorg).
    pub finalized_l2: BlockRef,
}

/// Block reference with number and hash.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockRef {
    /// Block number.
    pub number: u64,
    /// Block hash.
    pub hash: String,
}

impl KonaNodeHandler {
    /// Query the sync status from this kona-node using the optimism_syncStatus RPC method.
    ///
    /// Returns the sync status if the node is accessible and responding, or an error if
    /// the RPC URL is not published or the request fails.
    ///
    /// # Errors
    /// - Returns an error if the RPC URL is not published to the host
    /// - Returns an error if the HTTP request fails
    /// - Returns an error if the response contains an RPC error
    /// - Returns an error if the response cannot be parsed
    pub async fn sync_status(&self) -> Result<SyncStatus, anyhow::Error> {
        let rpc_url = self
            .rpc_host_url
            .as_ref()
            .context("RPC URL not published to host")?;

        let client = rpc::create_client()?;

        rpc::json_rpc_call(&client, rpc_url.as_str(), "optimism_syncStatus", vec![]).await
    }

    /// Wait for this kona-node to be ready by polling the RPC endpoint.
    ///
    /// Polls the node's RPC endpoint until it responds successfully or the timeout is reached.
    ///
    /// # Arguments
    /// * `timeout_secs` - Maximum time to wait in seconds
    ///
    /// # Errors
    /// Returns an error if the node doesn't become ready within the timeout period.
    pub async fn wait_until_ready(&self, timeout_secs: u64) -> Result<(), anyhow::Error> {
        rpc::wait_until_ready(&self.container_name, timeout_secs, || async {
            self.sync_status().await.map(|_| ())
        })
        .await
    }
}
