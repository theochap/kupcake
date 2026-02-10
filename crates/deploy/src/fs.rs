//! File system utils.

use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use notify::{Event, RecursiveMode, Watcher};
use tokio::sync::watch;

pub struct FsHandler;

impl FsHandler {
    pub fn set_writable(path: &PathBuf) -> anyhow::Result<()> {
        let metadata = std::fs::metadata(path).context("Failed to get metadata for file")?;

        let mut perms = metadata.permissions();

        perms.set_readonly(false);

        std::fs::set_permissions(path, perms)
            .context("Failed to set permissions on intent file")?;

        Ok(())
    }

    // Create a docker host data directory if it doesn't exist
    // This ensures the bind mount has proper permissions
    pub fn create_host_config_directory(host_config_path: &PathBuf) -> anyhow::Result<()> {
        std::fs::create_dir_all(host_config_path)
            .context("Failed to create docker host data directory")?;
        tracing::debug!(
            "Created docker host data directory: {}",
            host_config_path.display()
        );

        Self::set_writable(host_config_path)
            .context("Failed to set permissions on docker host data directory")?;

        Ok(())
    }

    /// Wait for a file to be created with a timeout.
    ///
    /// This function uses file system watching (via notify crate) to efficiently
    /// wait for a file to appear.
    ///
    /// # Arguments
    /// * `path` - The path to wait for
    /// * `timeout` - Maximum duration to wait
    ///
    /// # Returns
    /// Ok(()) if the file was created/exists, Err if timeout or other error
    pub async fn wait_for_file(path: &PathBuf, timeout: Duration) -> anyhow::Result<()> {
        // If file already exists, return immediately
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            tracing::debug!("File already exists: {}", path.display());
            return Ok(());
        }

        tracing::debug!("Waiting for file: {}", path.display());

        // Watch the parent directory
        let parent = path
            .parent()
            .context("File path must have a parent directory")?;

        let (tx, mut rx) = watch::channel(None);

        // Create watcher
        // Clone the path to avoid borrowing issues
        let path_watcher = path.clone();
        let mut watcher =
            notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
                Ok(event) if (event.kind.is_create() || event.kind.is_modify()) && event.paths.contains(&path_watcher) => {
                    tracing::debug!(event = ?event, path = ?path_watcher.display(), "File detected");

                    if let Err(e) = tx.send(Some(event.clone())) {
                        tracing::error!(err = ?e, event = ?event, path = ?path_watcher.display(), "Failed to send event to channel");
                    }
                }
                Ok(event) => {
                    // Ignore other events
                    tracing::trace!(event = ?event, path = ?path_watcher.display(), "Ignored event emitted by file watcher");
                }
                Err(e) => {
                    tracing::error!(err = ?e, path = ?path_watcher.display(), "Failed to watch directory");
                }
            })
            .context("Failed to create file watcher")?;

        watcher
            .watch(parent, RecursiveMode::NonRecursive)
            .context("Failed to watch directory")?;

        // Wait for the file with timeout
        tokio::time::timeout(timeout, async {
            rx.changed()
                .await
                .map_err(|e| anyhow::anyhow!("File watcher channel closed: {}", e))
        })
        .await
        .context(format!("Timeout waiting for file: {}", path.display()))??;

        // Small delay to allow the file content to be fully flushed.
        // File watcher may trigger on file creation before the writer has finished.
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(())
    }
}
