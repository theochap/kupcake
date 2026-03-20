//! Devnet registry for tracking deployed devnets.
//!
//! Maintains a TOML file at `~/.kupcake/devnets.toml` that tracks all devnets
//! by name, state, and datadir path. Uses file locking for safe concurrent access.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

/// State of a tracked devnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DevnetState {
    Running,
    Stopped,
}

impl std::fmt::Display for DevnetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DevnetState::Running => write!(f, "running"),
            DevnetState::Stopped => write!(f, "stopped"),
        }
    }
}

/// A single devnet entry in the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevnetEntry {
    pub name: String,
    pub state: DevnetState,
    pub datadir: PathBuf,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<String>,
}

/// The registry file format.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DevnetRegistryFile {
    #[serde(default)]
    devnets: Vec<DevnetEntry>,
}

/// Devnet registry backed by `~/.kupcake/devnets.toml`.
pub struct DevnetRegistry {
    base_path: PathBuf,
}

fn now_iso8601() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

impl DevnetRegistry {
    /// Create a registry using the default path (`~/.kupcake`).
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let base_path = home.join(".kupcake");
        Self::with_base_path(base_path)
    }

    /// Create a registry with a custom base path (useful for testing).
    pub fn with_base_path(base_path: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&base_path)
            .with_context(|| format!("Failed to create registry dir: {}", base_path.display()))?;
        Ok(Self { base_path })
    }

    /// Register a devnet as Running. If it already exists, update state to Running.
    pub fn register(&self, name: &str, datadir: &Path) -> Result<()> {
        self.with_lock(|file| {
            if let Some(entry) = file.devnets.iter_mut().find(|e| e.name == name) {
                entry.state = DevnetState::Running;
                entry.stopped_at = None;
                entry.datadir = datadir.to_path_buf();
            } else {
                file.devnets.push(DevnetEntry {
                    name: name.to_string(),
                    state: DevnetState::Running,
                    datadir: datadir.to_path_buf(),
                    created_at: now_iso8601(),
                    stopped_at: None,
                });
            }
            Ok(())
        })
    }

    /// Mark a devnet as Stopped. No-op if name not found.
    pub fn mark_stopped(&self, name: &str) -> Result<()> {
        self.with_lock(|file| {
            if let Some(entry) = file.devnets.iter_mut().find(|e| e.name == name) {
                entry.state = DevnetState::Stopped;
                entry.stopped_at = Some(now_iso8601());
            }
            Ok(())
        })
    }

    /// Remove a devnet entry entirely.
    pub fn remove(&self, name: &str) -> Result<()> {
        self.with_lock(|file| {
            file.devnets.retain(|e| e.name != name);
            Ok(())
        })
    }

    /// List all devnet entries.
    pub fn list(&self) -> Result<Vec<DevnetEntry>> {
        self.with_lock(|file| Ok(file.devnets.clone()))
    }

    /// Remove all Stopped entries, delete their datadirs, return removed entries.
    pub fn prune(&self) -> Result<Vec<DevnetEntry>> {
        self.with_lock(|file| {
            let (stopped, running): (Vec<_>, Vec<_>) = file
                .devnets
                .drain(..)
                .partition(|e| e.state == DevnetState::Stopped);

            file.devnets = running;

            for entry in &stopped {
                if entry.datadir.exists()
                    && let Err(e) = std::fs::remove_dir_all(&entry.datadir)
                {
                    tracing::warn!(
                        datadir = %entry.datadir.display(),
                        error = %e,
                        "Failed to remove datadir for stopped devnet"
                    );
                }
            }

            Ok(stopped)
        })
    }

    fn registry_path(&self) -> PathBuf {
        self.base_path.join("devnets.toml")
    }

    fn lock_path(&self) -> PathBuf {
        self.base_path.join("devnets.lock")
    }

    /// Acquire exclusive lock, read registry, apply mutation, write back, release lock.
    fn with_lock<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut DevnetRegistryFile) -> Result<R>,
    {
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(self.lock_path())
            .context("Failed to open registry lock file")?;

        lock_file
            .lock_exclusive()
            .context("Failed to acquire registry lock")?;

        let _guard = LockGuard(&lock_file);

        let registry_path = self.registry_path();
        let mut data = if registry_path.exists() {
            let content = std::fs::read_to_string(&registry_path)
                .context("Failed to read devnet registry")?;
            toml::from_str(&content).context("Failed to parse devnet registry")?
        } else {
            DevnetRegistryFile::default()
        };

        let result = f(&mut data)?;

        let content =
            toml::to_string_pretty(&data).context("Failed to serialize devnet registry")?;
        std::fs::write(&registry_path, content).context("Failed to write devnet registry")?;

        Ok(result)
    }
}

/// RAII guard that unlocks on drop, logging warnings instead of panicking.
struct LockGuard<'a>(&'a std::fs::File);

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.0.unlock() {
            tracing::warn!(error = %e, "Failed to release registry lock");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    fn test_registry() -> (TempDir, DevnetRegistry) {
        let dir = TempDir::new("kupcake-registry-test").unwrap();
        let registry = DevnetRegistry::with_base_path(dir.path().to_path_buf()).unwrap();
        (dir, registry)
    }

    #[test]
    fn test_creates_dir_and_file() {
        let dir = TempDir::new("kupcake-registry-test").unwrap();
        let nested = dir.path().join("nested").join("path");
        let registry = DevnetRegistry::with_base_path(nested.clone()).unwrap();
        assert!(nested.exists());

        // After a list call, the registry file should exist
        let _ = registry.list().unwrap();
        assert!(nested.join("devnets.toml").exists());
    }

    #[test]
    fn test_register_devnet() {
        let (_dir, registry) = test_registry();
        registry
            .register("test-net", Path::new("/tmp/data-test-net"))
            .unwrap();

        let entries = registry.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test-net");
        assert_eq!(entries[0].state, DevnetState::Running);
        assert!(!entries[0].created_at.is_empty());
    }

    #[test]
    fn test_register_duplicate_is_idempotent() {
        let (_dir, registry) = test_registry();
        registry
            .register("test-net", Path::new("/tmp/data-test-net"))
            .unwrap();
        registry.mark_stopped("test-net").unwrap();
        registry
            .register("test-net", Path::new("/tmp/data-test-net"))
            .unwrap();

        let entries = registry.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, DevnetState::Running);
        assert!(entries[0].stopped_at.is_none());
    }

    #[test]
    fn test_mark_stopped() {
        let (_dir, registry) = test_registry();
        registry
            .register("test-net", Path::new("/tmp/data-test-net"))
            .unwrap();
        registry.mark_stopped("test-net").unwrap();

        let entries = registry.list().unwrap();
        assert_eq!(entries[0].state, DevnetState::Stopped);
        assert!(entries[0].stopped_at.is_some());
    }

    #[test]
    fn test_mark_stopped_missing_is_noop() {
        let (_dir, registry) = test_registry();
        // Should not error
        registry.mark_stopped("nonexistent").unwrap();
        let entries = registry.list().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_remove_devnet() {
        let (_dir, registry) = test_registry();
        registry
            .register("test-net", Path::new("/tmp/data-test-net"))
            .unwrap();
        registry.remove("test-net").unwrap();

        let entries = registry.list().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_list() {
        let (_dir, registry) = test_registry();
        registry
            .register("net-1", Path::new("/tmp/data-net-1"))
            .unwrap();
        registry
            .register("net-2", Path::new("/tmp/data-net-2"))
            .unwrap();

        let entries = registry.list().unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_prune_removes_stopped() {
        let (dir, registry) = test_registry();

        // Create temp datadirs that prune should delete
        let datadir = dir.path().join("data-stopped");
        std::fs::create_dir_all(&datadir).unwrap();

        registry.register("stopped-net", &datadir).unwrap();
        registry.mark_stopped("stopped-net").unwrap();

        let removed = registry.prune().unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].name, "stopped-net");
        assert!(!datadir.exists());

        let entries = registry.list().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_prune_skips_running() {
        let (dir, registry) = test_registry();

        let stopped_dir = dir.path().join("data-stopped");
        std::fs::create_dir_all(&stopped_dir).unwrap();

        registry
            .register("running-net", Path::new("/tmp/data-running"))
            .unwrap();
        registry.register("stopped-net", &stopped_dir).unwrap();
        registry.mark_stopped("stopped-net").unwrap();

        let removed = registry.prune().unwrap();
        assert_eq!(removed.len(), 1);

        let entries = registry.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "running-net");
    }

    #[test]
    fn test_roundtrip_serialization() {
        let (_dir, registry) = test_registry();

        registry
            .register("test-net", Path::new("/tmp/data-test"))
            .unwrap();
        registry.mark_stopped("test-net").unwrap();

        let entries = registry.list().unwrap();
        let entry = &entries[0];

        assert_eq!(entry.name, "test-net");
        assert_eq!(entry.state, DevnetState::Stopped);
        assert_eq!(entry.datadir, PathBuf::from("/tmp/data-test"));
        assert!(entry.created_at.contains('T'));
        assert!(entry.stopped_at.is_some());
    }
}
