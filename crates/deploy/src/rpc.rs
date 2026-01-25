//! Shared RPC utilities for interacting with Ethereum JSON-RPC endpoints.

use std::time::Duration;

use anyhow::Context;
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

/// Wait for a service to be ready by repeatedly calling a check function.
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
    let start = std::time::Instant::now();
    let max_duration = Duration::from_secs(timeout_secs);

    loop {
        if start.elapsed() > max_duration {
            anyhow::bail!("Timeout waiting for {} to be ready", name);
        }

        match check_fn().await {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::trace!(error = %e, service = %name, "Readiness check failed, retrying...");
            }
        }

        tokio::time::sleep(DEFAULT_POLL_INTERVAL).await;
    }
}
