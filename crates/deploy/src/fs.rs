//! File system utils.

use std::{path::Path, path::PathBuf, time::Duration};

use anyhow::Context;
use notify::{Event, RecursiveMode, Watcher};
use tokio::sync::watch;

pub struct FsHandler;

impl FsHandler {
    /// Recursively copy a directory tree from `src` to `dst`.
    ///
    /// Creates `dst` and all intermediate directories. Preserves the directory
    /// structure under `src`.
    pub async fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(dst).await.context(format!(
            "Failed to create destination directory: {}",
            dst.display()
        ))?;

        let mut entries = tokio::fs::read_dir(src).await.context(format!(
            "Failed to read source directory: {}",
            src.display()
        ))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .context("Failed to read directory entry")?
        {
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let file_type = entry.file_type().await.context("Failed to get file type")?;

            if file_type.is_dir() {
                Box::pin(Self::copy_dir_recursive(&src_path, &dst_path)).await?;
            } else {
                tokio::fs::copy(&src_path, &dst_path)
                    .await
                    .context(format!(
                        "Failed to copy {} -> {}",
                        src_path.display(),
                        dst_path.display()
                    ))?;
            }
        }

        Ok(())
    }

    pub fn set_writable(path: &Path) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)
                .context("Failed to get metadata for file")?
                .permissions();
            perms.set_mode(0o777);
            std::fs::set_permissions(path, perms).context("Failed to set permissions on file")?;
        }
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
