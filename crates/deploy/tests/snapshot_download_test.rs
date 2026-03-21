//! Tests for snapshot download and extraction.
//!
//! Unit tests cover URL parsing, resolution, and directory validation.
//! The download+extract test uses an in-process HTTP server serving a `.tar.gz` fixture.
//! The full integration test (Docker required) is `#[ignore]`.
//!
//! Run with: cargo test --test snapshot_download_test

mod common;

use anyhow::Result;
use kupcake_deploy::snapshot_download;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use common::*;

/// Build a minimal `.tar.gz` snapshot archive in memory.
///
/// The archive contains:
/// - `rollup.json` (minimal valid JSON)
/// - `reth-data/` directory with a placeholder file
fn build_test_snapshot_archive() -> Vec<u8> {
    let buf = Vec::new();
    let encoder = flate2::write::GzEncoder::new(buf, flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);

    // Add rollup.json
    let rollup_content = b"{}";
    let mut header = tar::Header::new_gnu();
    header.set_size(rollup_content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "rollup.json", &rollup_content[..])
        .unwrap();

    // Add reth-data directory
    let mut dir_header = tar::Header::new_gnu();
    dir_header.set_entry_type(tar::EntryType::Directory);
    dir_header.set_size(0);
    dir_header.set_mode(0o755);
    dir_header.set_cksum();
    builder
        .append_data(&mut dir_header, "reth-data/", &[] as &[u8])
        .unwrap();

    // Add a placeholder file inside reth-data
    let placeholder = b"placeholder";
    let mut file_header = tar::Header::new_gnu();
    file_header.set_size(placeholder.len() as u64);
    file_header.set_mode(0o644);
    file_header.set_cksum();
    builder
        .append_data(&mut file_header, "reth-data/IDENTITY", &placeholder[..])
        .unwrap();

    let encoder = builder.into_inner().unwrap();
    encoder.finish().unwrap()
}

/// Build a `.tar.gz` archive with a top-level directory prefix (as `tar czf dir/` produces).
fn build_test_snapshot_archive_with_prefix(prefix: &str) -> Vec<u8> {
    let buf = Vec::new();
    let encoder = flate2::write::GzEncoder::new(buf, flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);

    // Add prefix directory
    let mut dir_header = tar::Header::new_gnu();
    dir_header.set_entry_type(tar::EntryType::Directory);
    dir_header.set_size(0);
    dir_header.set_mode(0o755);
    dir_header.set_cksum();
    builder
        .append_data(&mut dir_header, format!("{prefix}/"), &[] as &[u8])
        .unwrap();

    // Add rollup.json under prefix
    let rollup_content = b"{}";
    let mut header = tar::Header::new_gnu();
    header.set_size(rollup_content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, format!("{prefix}/rollup.json"), &rollup_content[..])
        .unwrap();

    // Add reth-data directory under prefix
    let mut reth_dir_header = tar::Header::new_gnu();
    reth_dir_header.set_entry_type(tar::EntryType::Directory);
    reth_dir_header.set_size(0);
    reth_dir_header.set_mode(0o755);
    reth_dir_header.set_cksum();
    builder
        .append_data(
            &mut reth_dir_header,
            format!("{prefix}/reth-data/"),
            &[] as &[u8],
        )
        .unwrap();

    // Add placeholder inside reth-data
    let placeholder = b"placeholder";
    let mut file_header = tar::Header::new_gnu();
    file_header.set_size(placeholder.len() as u64);
    file_header.set_mode(0o644);
    file_header.set_cksum();
    builder
        .append_data(
            &mut file_header,
            format!("{prefix}/reth-data/IDENTITY"),
            &placeholder[..],
        )
        .unwrap();

    let encoder = builder.into_inner().unwrap();
    encoder.finish().unwrap()
}

/// Start a simple HTTP server that serves `data` on any GET request.
/// Returns the base URL (e.g., "http://127.0.0.1:PORT").
async fn start_mock_http_server(data: Vec<u8>) -> Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}", addr.port());

    let handle = tokio::spawn(async move {
        // Serve a single request then keep listening for more
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let data = data.clone();
            tokio::spawn(async move {
                // Read the request (we don't care about the content)
                let mut buf = [0u8; 4096];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;

                // Send HTTP response
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/gzip\r\n\r\n",
                    data.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.write_all(&data).await;
                let _ = stream.flush().await;
            });
        }
    });

    Ok((url, handle))
}

// =============================================================================
// Unit tests: URL parsing and resolution
// =============================================================================

#[test]
fn test_is_remote_url_local_paths() {
    assert!(!snapshot_download::is_remote_url("/home/user/snapshot"));
    assert!(!snapshot_download::is_remote_url("./my-snapshot"));
    assert!(!snapshot_download::is_remote_url("../snapshots/data"));
    assert!(!snapshot_download::is_remote_url("snapshot-dir"));
}

#[test]
fn test_is_remote_url_https() {
    assert!(snapshot_download::is_remote_url(
        "https://storage.googleapis.com/bucket/path"
    ));
    assert!(snapshot_download::is_remote_url(
        "http://localhost:8080/snapshot.tar.gz"
    ));
}

#[test]
fn test_is_remote_url_gs() {
    assert!(snapshot_download::is_remote_url(
        "gs://oplabs-snapshots/kupcake/op-sepolia/latest.tar.gz"
    ));
}

#[test]
fn test_is_remote_url_shorthand() {
    assert!(snapshot_download::is_remote_url("op-sepolia/latest"));
    assert!(snapshot_download::is_remote_url("op-mainnet/v1.0"));
}

#[test]
fn test_resolve_snapshot_url_https_passthrough() {
    let url = "https://example.com/snapshot.tar.gz";
    assert_eq!(snapshot_download::resolve_snapshot_url(url), url);
}

#[test]
fn test_resolve_snapshot_url_gs() {
    assert_eq!(
        snapshot_download::resolve_snapshot_url("gs://my-bucket/path/to/snapshot.tar.gz"),
        "https://storage.googleapis.com/my-bucket/path/to/snapshot.tar.gz"
    );
}

#[test]
fn test_resolve_snapshot_url_shorthand() {
    assert_eq!(
        snapshot_download::resolve_snapshot_url("op-sepolia/latest"),
        "https://storage.googleapis.com/oplabs-snapshots/kupcake/op-sepolia/latest.tar.gz"
    );
}

#[test]
fn test_resolve_snapshot_url_shorthand_with_extension() {
    assert_eq!(
        snapshot_download::resolve_snapshot_url("op-sepolia/latest.tar.gz"),
        "https://storage.googleapis.com/oplabs-snapshots/kupcake/op-sepolia/latest.tar.gz"
    );
}

// =============================================================================
// Unit tests: snapshot directory validation
// =============================================================================

#[test]
fn test_validate_snapshot_dir_valid() {
    let dir = tempdir::TempDir::new("snapshot-test").unwrap();
    std::fs::write(dir.path().join("rollup.json"), "{}").unwrap();
    std::fs::create_dir(dir.path().join("reth-data")).unwrap();

    let contents = snapshot_download::validate_snapshot_dir(dir.path()).unwrap();
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

    let contents = snapshot_download::validate_snapshot_dir(dir.path()).unwrap();
    assert!(contents.intent_toml.is_some());
}

#[test]
fn test_validate_snapshot_dir_missing_rollup() {
    let dir = tempdir::TempDir::new("snapshot-test").unwrap();
    std::fs::create_dir(dir.path().join("reth-data")).unwrap();

    let err = snapshot_download::validate_snapshot_dir(dir.path()).unwrap_err();
    assert!(err.to_string().contains("missing rollup.json"));
}

#[test]
fn test_validate_snapshot_dir_no_subdir() {
    let dir = tempdir::TempDir::new("snapshot-test").unwrap();
    std::fs::write(dir.path().join("rollup.json"), "{}").unwrap();

    let err = snapshot_download::validate_snapshot_dir(dir.path()).unwrap_err();
    assert!(err.to_string().contains("no subdirectory"));
}

#[test]
fn test_validate_snapshot_dir_multiple_subdirs() {
    let dir = tempdir::TempDir::new("snapshot-test").unwrap();
    std::fs::write(dir.path().join("rollup.json"), "{}").unwrap();
    std::fs::create_dir(dir.path().join("reth-data-1")).unwrap();
    std::fs::create_dir(dir.path().join("reth-data-2")).unwrap();

    let err = snapshot_download::validate_snapshot_dir(dir.path()).unwrap_err();
    assert!(err.to_string().contains("2 subdirectories"));
}

// =============================================================================
// Download + extract tests (use mock HTTP server)
// =============================================================================

#[tokio::test]
async fn test_download_and_extract_snapshot() {
    let archive = build_test_snapshot_archive();
    let (url, server) = start_mock_http_server(archive).await.unwrap();

    let dest = tempdir::TempDir::new("snapshot-download-test").unwrap();
    let snapshot_url = format!("{url}/snapshot.tar.gz");

    let result =
        snapshot_download::download_and_extract_snapshot(&snapshot_url, dest.path()).await;

    assert!(result.is_ok(), "download_and_extract failed: {result:?}");

    let extracted = result.unwrap();
    let contents = snapshot_download::validate_snapshot_dir(&extracted).unwrap();
    assert!(contents.rollup_json.exists());
    assert!(contents.reth_db_dir.exists());
    assert!(contents.reth_db_dir.join("IDENTITY").exists());

    server.abort();
}

#[tokio::test]
async fn test_download_and_extract_snapshot_with_prefix() {
    let archive = build_test_snapshot_archive_with_prefix("my-snapshot-v1");
    let (url, server) = start_mock_http_server(archive).await.unwrap();

    let dest = tempdir::TempDir::new("snapshot-download-prefix-test").unwrap();
    let snapshot_url = format!("{url}/snapshot.tar.gz");

    let result =
        snapshot_download::download_and_extract_snapshot(&snapshot_url, dest.path()).await;

    assert!(result.is_ok(), "download_and_extract failed: {result:?}");

    // Should strip the top-level prefix and extract directly into dest
    let extracted = result.unwrap();
    let contents = snapshot_download::validate_snapshot_dir(&extracted).unwrap();
    assert!(contents.rollup_json.exists());
    assert!(contents.reth_db_dir.exists());

    server.abort();
}

#[tokio::test]
async fn test_download_snapshot_404_fails() {
    // Start a server that returns 404
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://127.0.0.1:{}/missing.tar.gz", addr.port());

    let server = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
    });

    let dest = tempdir::TempDir::new("snapshot-404-test").unwrap();
    let result = snapshot_download::download_and_extract_snapshot(&url, dest.path()).await;

    assert!(result.is_err());
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("404") || err.contains("failed") || err.contains("Failed"),
        "Expected download failure error, got: {err}"
    );

    server.abort();
}

// =============================================================================
// Full integration test (requires Docker)
// =============================================================================

#[tokio::test]
#[ignore] // Requires Docker and network access
async fn test_snapshot_download_and_deploy() {
    let _permit = TEST_SEMAPHORE.acquire().await.unwrap();

    // Build a test archive and serve it
    let archive = build_test_snapshot_archive();
    let (url, server) = start_mock_http_server(archive).await.unwrap();

    let ctx = TestContext::new("snap-dl");

    // Download and extract
    let download_dir = ctx.outdata_path.join("snapshot-download");
    let snapshot_url = format!("{url}/snapshot.tar.gz");
    let snapshot_path = snapshot_download::download_and_extract_snapshot(
        &snapshot_url,
        &download_dir,
    )
    .await
    .expect("Failed to download snapshot");

    // Validate extraction
    let contents = snapshot_download::validate_snapshot_dir(&snapshot_path)
        .expect("Invalid snapshot structure");
    assert!(contents.rollup_json.exists());
    assert!(contents.reth_db_dir.exists());

    // Build deployer with snapshot (uses live mode since snapshot requires --l1)
    let deployer = kupcake_deploy::DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(kupcake_deploy::OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(1) // Only sequencer, no validators for speed
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .snapshot(snapshot_path)
        .copy_snapshot(true)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(false)
        .build()
        .await
        .expect("Failed to build deployer");

    deployer.save_config().expect("Failed to save config");

    // Deploy and check for block progression
    let (_docker, deployment) = ctx.deploy(deployer).await.expect("Deployment failed");

    wait_for_all_nodes(&deployment).await;

    // Reload deployer from config for health check helpers
    let reloaded = kupcake_deploy::Deployer::load_from_file(
        &ctx.outdata_path.join("Kupcake.toml"),
    )
    .expect("Failed to reload deployer");

    let result = wait_for_all_nodes_advancing(
        &reloaded,
        120, // 2 min timeout for block progression
    )
    .await;

    // Cleanup
    let _ = kupcake_deploy::cleanup_by_prefix(&ctx.network_name).await;
    server.abort();

    result.expect("Blocks did not advance");
}
