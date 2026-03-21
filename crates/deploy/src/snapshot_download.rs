//! Download and validate remote snapshot archives from GCS.
//!
//! Supports three URL formats:
//! - Full HTTPS: `https://storage.googleapis.com/oplabs-snapshots/kupcake/...`
//! - GCS URI: `gs://bucket/path`
//! - Shorthand: `op-sepolia/latest` (expands to the oplabs GCS bucket)

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use backon::{ExponentialBuilder, Retryable};
use futures::StreamExt;

/// Default GCS bucket for oplabs snapshots.
const GCS_BUCKET: &str = "oplabs-snapshots";

/// Default path prefix within the bucket.
const GCS_PATH_PREFIX: &str = "kupcake";

/// Base URL for GCS public downloads.
const GCS_BASE_URL: &str = "https://storage.googleapis.com";

/// Contents of a validated snapshot directory.
#[derive(Debug)]
pub struct SnapshotContents {
    /// Path to rollup.json.
    pub rollup_json: PathBuf,
    /// Path to the single reth database subdirectory.
    pub reth_db_dir: PathBuf,
    /// Path to intent.toml, if present.
    pub intent_toml: Option<PathBuf>,
}

/// Returns `true` if the snapshot string looks like a remote URL rather than a local path.
pub fn is_remote_url(input: &str) -> bool {
    input.starts_with("https://")
        || input.starts_with("http://")
        || input.starts_with("gs://")
        || is_shorthand(input)
}

/// Resolve a snapshot input string to a full HTTPS download URL.
///
/// - `gs://bucket/path` → `https://storage.googleapis.com/bucket/path`
/// - Shorthand like `op-sepolia/latest` → full GCS URL with `.tar.gz` suffix
/// - Full HTTPS URL → pass through unchanged
pub fn resolve_snapshot_url(input: &str) -> String {
    if input.starts_with("https://") || input.starts_with("http://") {
        return input.to_string();
    }

    if let Some(gcs_path) = input.strip_prefix("gs://") {
        return format!("{GCS_BASE_URL}/{gcs_path}");
    }

    // Shorthand: "op-sepolia/latest" → full GCS URL
    let name = input.strip_suffix(".tar.gz").unwrap_or(input);
    format!("{GCS_BASE_URL}/{GCS_BUCKET}/{GCS_PATH_PREFIX}/{name}.tar.gz")
}

/// Download a `.tar.gz` snapshot archive from `url` and extract it into `dest_dir`.
///
/// The HTTP response is buffered into memory and then extracted synchronously
/// via `tar` + `flate2`. Retries transient failures (5xx, timeouts) with
/// exponential backoff.
///
/// NOTE: The entire archive is buffered in memory. For very large snapshots
/// (>4 GB), consider switching to streaming extraction via a tempfile.
///
/// Returns the path to `dest_dir` after successful extraction.
pub async fn download_and_extract_snapshot(url: &str, dest_dir: &Path) -> Result<PathBuf> {
    let dest = dest_dir.to_path_buf();

    tracing::info!(url = %url, dest = %dest.display(), "Downloading snapshot archive");

    let bytes = download_with_retry(url).await?;

    tracing::info!(
        size_mb = bytes.len() / (1024 * 1024),
        "Download complete, extracting archive"
    );

    // Extract in a blocking task since tar/flate2 are synchronous.
    let extract_dest = dest.clone();
    tokio::task::spawn_blocking(move || extract_tar_gz(&bytes, &extract_dest))
        .await
        .context("Extraction task panicked")?
        .context("Failed to extract snapshot archive")?;

    tracing::info!(dest = %dest.display(), "Snapshot extracted successfully");

    Ok(dest)
}

/// Validate that a directory has the expected snapshot structure.
///
/// Expects:
/// - `rollup.json` (required)
/// - Exactly one subdirectory (the reth database)
/// - Optionally `intent.toml`
pub fn validate_snapshot_dir(path: &Path) -> Result<SnapshotContents> {
    let rollup_json = path.join("rollup.json");
    if !rollup_json.exists() {
        anyhow::bail!(
            "Snapshot directory is missing rollup.json: {}",
            path.display()
        );
    }

    let subdirs: Vec<_> = std::fs::read_dir(path)
        .context("Failed to read snapshot directory")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to read directory entry")?
        .into_iter()
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .collect();

    let reth_db_dir = match subdirs.len() {
        0 => anyhow::bail!(
            "Snapshot directory has no subdirectory (expected reth database): {}",
            path.display()
        ),
        1 => subdirs
            .into_iter()
            .next()
            .map(|e| e.path())
            .context("unreachable")?,
        n => anyhow::bail!(
            "Snapshot directory has {} subdirectories, expected exactly one reth database directory. Found: {}",
            n,
            subdirs
                .iter()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };

    let intent_path = path.join("intent.toml");
    let intent_toml = intent_path.exists().then_some(intent_path);

    Ok(SnapshotContents {
        rollup_json,
        reth_db_dir,
        intent_toml,
    })
}

/// Download bytes from a URL with exponential backoff retry on transient errors.
async fn download_with_retry(url: &str) -> Result<Vec<u8>> {
    let url = url.to_string();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .context("Failed to build HTTP client")?;

    let download = || async {
        let response = client
            .get(&url)
            .send()
            .await
            .context("HTTP request failed")?;

        let status = response.status();
        if status.is_server_error() {
            anyhow::bail!("Server error: {status}");
        }
        if !status.is_success() {
            anyhow::bail!("Download failed with status {status} for URL: {url}");
        }

        let content_length = response.content_length();
        if let Some(len) = content_length {
            tracing::info!(size_mb = len / (1024 * 1024), "Snapshot size");
        }

        let capacity = content_length
            .and_then(|len| usize::try_from(len).ok())
            .unwrap_or(0);
        let mut bytes = Vec::with_capacity(capacity);
        let mut stream = response.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut last_log: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Error reading response stream")?;
            downloaded += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);

            // Log progress every 50 MB
            if downloaded - last_log >= 50 * 1024 * 1024 {
                tracing::info!(downloaded_mb = downloaded / (1024 * 1024), "Downloading...");
                last_log = downloaded;
            }
        }

        Ok(bytes)
    };

    let backoff = ExponentialBuilder::default()
        .with_min_delay(std::time::Duration::from_secs(1))
        .with_max_delay(std::time::Duration::from_secs(30))
        .with_max_times(3);

    download
        .retry(backoff)
        .when(|e| {
            let msg = e.to_string();
            if msg.starts_with("Server error") {
                return true;
            }
            // Check for reqwest transport errors (timeout, connection reset)
            e.chain().any(|cause| {
                cause
                    .downcast_ref::<reqwest::Error>()
                    .map(|re| re.is_timeout() || re.is_connect())
                    .unwrap_or(false)
            })
        })
        .await
        .context("Failed to download snapshot after retries")
}

/// Extract a gzipped tar archive into `dest_dir`.
///
/// If all entries share a common top-level directory prefix (e.g., from
/// `tar czf archive.tar.gz dir/`), the archive is first extracted to a
/// temporary location and then the contents are moved up to `dest_dir`.
fn extract_tar_gz(data: &[u8], dest_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("Failed to create directory: {}", dest_dir.display()))?;

    // Use tar's built-in unpack which handles all entry types (files, dirs, symlinks).
    let decoder = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(dest_dir)
        .context("Failed to unpack tar archive")?;

    // Check if all extracted entries share a single top-level directory.
    // If so, move its contents up to dest_dir to strip the prefix.
    let entries: Vec<_> = std::fs::read_dir(dest_dir)
        .context("Failed to read extracted directory")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to read directory entry")?;

    // If there's exactly one entry and it's a directory, move its contents up.
    if entries.len() == 1 {
        let entry = &entries[0];
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            let prefix_dir = entry.path();
            let inner_entries: Vec<_> = std::fs::read_dir(&prefix_dir)
                .context("Failed to read prefix directory")?
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to read prefix directory entry")?;

            for inner in inner_entries {
                let src = inner.path();
                let dst = dest_dir.join(inner.file_name());
                std::fs::rename(&src, &dst).with_context(|| {
                    format!(
                        "Failed to move {} to {}",
                        src.display(),
                        dst.display()
                    )
                })?;
            }

            // Remove the now-empty prefix directory
            std::fs::remove_dir(&prefix_dir).with_context(|| {
                format!(
                    "Failed to remove prefix directory: {}",
                    prefix_dir.display()
                )
            })?;
        }
    }

    Ok(())
}

/// Check if an input string looks like a shorthand snapshot name (e.g., "op-sepolia/latest").
///
/// A shorthand contains a `/`, does not start with `/` or `.`, and is not a URL scheme.
fn is_shorthand(input: &str) -> bool {
    input.contains('/')
        && !input.starts_with('/')
        && !input.starts_with('.')
        && !input.starts_with("https://")
        && !input.starts_with("http://")
        && !input.starts_with("gs://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_remote_url_local_paths() {
        assert!(!is_remote_url("/home/user/snapshot"));
        assert!(!is_remote_url("./my-snapshot"));
        assert!(!is_remote_url("../snapshots/data"));
        assert!(!is_remote_url("snapshot-dir"));
    }

    #[test]
    fn test_is_remote_url_urls() {
        assert!(is_remote_url("https://storage.googleapis.com/bucket/path"));
        assert!(is_remote_url("http://localhost:8080/snapshot.tar.gz"));
        assert!(is_remote_url("gs://oplabs-snapshots/kupcake/op-sepolia/latest.tar.gz"));
    }

    #[test]
    fn test_is_remote_url_shorthand() {
        assert!(is_remote_url("op-sepolia/latest"));
        assert!(is_remote_url("op-mainnet/v1.0"));
    }

    #[test]
    fn test_resolve_snapshot_url_https_passthrough() {
        let url = "https://example.com/snapshot.tar.gz";
        assert_eq!(resolve_snapshot_url(url), url);
    }

    #[test]
    fn test_resolve_snapshot_url_gs() {
        assert_eq!(
            resolve_snapshot_url("gs://my-bucket/path/to/snapshot.tar.gz"),
            "https://storage.googleapis.com/my-bucket/path/to/snapshot.tar.gz"
        );
    }

    #[test]
    fn test_resolve_snapshot_url_shorthand() {
        assert_eq!(
            resolve_snapshot_url("op-sepolia/latest"),
            "https://storage.googleapis.com/oplabs-snapshots/kupcake/op-sepolia/latest.tar.gz"
        );
    }

    #[test]
    fn test_resolve_snapshot_url_shorthand_with_extension() {
        assert_eq!(
            resolve_snapshot_url("op-sepolia/latest.tar.gz"),
            "https://storage.googleapis.com/oplabs-snapshots/kupcake/op-sepolia/latest.tar.gz"
        );
    }

    #[test]
    fn test_validate_snapshot_dir_valid() {
        let dir = tempdir::TempDir::new("snapshot-test").unwrap();
        std::fs::write(dir.path().join("rollup.json"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("reth-data")).unwrap();

        let contents = validate_snapshot_dir(dir.path()).unwrap();
        assert_eq!(contents.rollup_json, dir.path().join("rollup.json"));
        assert_eq!(contents.reth_db_dir, dir.path().join("reth-data"));
        assert!(contents.intent_toml.is_none());
    }

    #[test]
    fn test_validate_snapshot_dir_with_intent() {
        let dir = tempdir::TempDir::new("snapshot-test").unwrap();
        std::fs::write(dir.path().join("rollup.json"), "{}").unwrap();
        std::fs::write(dir.path().join("intent.toml"), "").unwrap();
        std::fs::create_dir(dir.path().join("reth-data")).unwrap();

        let contents = validate_snapshot_dir(dir.path()).unwrap();
        assert!(contents.intent_toml.is_some());
    }

    #[test]
    fn test_validate_snapshot_dir_missing_rollup() {
        let dir = tempdir::TempDir::new("snapshot-test").unwrap();
        std::fs::create_dir(dir.path().join("reth-data")).unwrap();

        let err = validate_snapshot_dir(dir.path()).unwrap_err();
        assert!(err.to_string().contains("missing rollup.json"));
    }

    #[test]
    fn test_validate_snapshot_dir_no_subdir() {
        let dir = tempdir::TempDir::new("snapshot-test").unwrap();
        std::fs::write(dir.path().join("rollup.json"), "{}").unwrap();

        let err = validate_snapshot_dir(dir.path()).unwrap_err();
        assert!(err.to_string().contains("no subdirectory"));
    }

    #[test]
    fn test_validate_snapshot_dir_multiple_subdirs() {
        let dir = tempdir::TempDir::new("snapshot-test").unwrap();
        std::fs::write(dir.path().join("rollup.json"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("reth-data-1")).unwrap();
        std::fs::create_dir(dir.path().join("reth-data-2")).unwrap();

        let err = validate_snapshot_dir(dir.path()).unwrap_err();
        assert!(err.to_string().contains("2 subdirectories"));
    }
}
