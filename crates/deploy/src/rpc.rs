//! Shared RPC utilities for interacting with Ethereum JSON-RPC endpoints.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use backon::{ConstantBuilder, Retryable};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Default timeout for RPC requests.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default interval between polling attempts when waiting for readiness.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Create an HTTP client configured for JSON-RPC requests.
pub fn create_client() -> Result<reqwest::Client, anyhow::Error> {
    reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .context("Failed to create HTTP client")
}

/// Make a JSON-RPC call and deserialize the result.
///
/// # Arguments
/// * `client` - The HTTP client to use
/// * `url` - The RPC endpoint URL
/// * `method` - The RPC method name
/// * `params` - The method parameters
///
/// # Returns
/// The deserialized result, or an error if the request failed or returned an error response.
pub async fn json_rpc_call<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Vec<Value>,
) -> Result<T, anyhow::Error> {
    let response = client
        .post(url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        }))
        .send()
        .await
        .with_context(|| format!("Failed to send {} request", method))?;

    let result: Value = response
        .json()
        .await
        .with_context(|| format!("Failed to parse {} response", method))?;

    if let Some(error) = result.get("error") {
        anyhow::bail!(
            "RPC error: {}",
            error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
        );
    }

    let result_value = result
        .get("result")
        .context("No result in response")?
        .clone();

    serde_json::from_value(result_value)
        .with_context(|| format!("Failed to deserialize {} result", method))
}

/// Dump Anvil state via `anvil_dumpState` RPC and write to disk.
///
/// Called before cleanup to persist Anvil L1 state via RPC. The returned hex
/// string is decoded and written as the state file that `--load-state` expects
/// on subsequent boots.
pub async fn anvil_dump_state(rpc_url: &str, output_path: &Path) -> Result<(), anyhow::Error> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let client = create_client()?;
    let hex_state: String = json_rpc_call(&client, rpc_url, "anvil_dumpState", vec![])
        .await
        .context("anvil_dumpState RPC failed")?;

    let hex_str = hex_state.strip_prefix("0x").unwrap_or(&hex_state);
    let compressed =
        hex::decode(hex_str).context("Failed to hex-decode anvil_dumpState response")?;

    // anvil_dumpState returns gzip-compressed JSON; --load-state expects plain JSON.
    let bytes = if compressed.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress gzipped anvil state")?;
        decompressed
    } else {
        compressed
    };

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    std::fs::write(output_path, &bytes)
        .with_context(|| format!("Failed to write state to {}", output_path.display()))?;

    tracing::info!(
        path = %output_path.display(),
        bytes = bytes.len(),
        "Dumped Anvil state via RPC"
    );
    Ok(())
}

/// Wait for a service to be ready by repeatedly calling a check function.
///
/// Uses `backon` for retries with a constant interval and a maximum duration.
///
/// # Arguments
/// * `name` - Name of the service (for error messages)
/// * `timeout_secs` - Maximum time to wait in seconds
/// * `check_fn` - Function that returns Ok(()) when the service is ready
///
/// # Returns
/// Ok(()) when the service is ready, or an error after timeout.
pub async fn wait_until_ready<F, Fut>(
    name: &str,
    timeout_secs: u64,
    check_fn: F,
) -> Result<(), anyhow::Error>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<(), anyhow::Error>>,
{
    let backoff = ConstantBuilder::default()
        .with_delay(DEFAULT_POLL_INTERVAL)
        .with_max_times(
            (timeout_secs as usize * 1000) / DEFAULT_POLL_INTERVAL.as_millis() as usize,
        );

    (|| async {
        let result = check_fn().await;
        if let Err(ref e) = result {
            tracing::trace!(error = %e, service = %name, "Readiness check failed, retrying...");
        }
        result
    })
    .retry(backoff)
    .await
    .with_context(|| format!("Timeout waiting for {} to be ready", name))
}
