//! Dynamic shell completions for the Kupcake CLI.
//!
//! Provides [`ValueCandidates`] implementations that read the devnet registry
//! at completion time, so tab-completion reflects the current set of deployed networks.

use clap_complete::engine::{CompletionCandidate, ValueCandidates};
use kupcake_deploy::{DevnetEntry, DevnetRegistry, DevnetState};

/// Build completion candidates from the registry, keeping only entries that pass `filter`.
fn devnet_candidates(filter: impl Fn(&DevnetEntry) -> bool) -> Vec<CompletionCandidate> {
    let Ok(registry) = DevnetRegistry::new() else {
        return Vec::new();
    };
    let Ok(entries) = registry.list() else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|e| filter(e))
        .map(|e| {
            CompletionCandidate::new(e.name).help(Some(
                format!("{} — {}", e.state, e.datadir.display()).into(),
            ))
        })
        .collect()
}

/// Completer that suggests only **Running** devnet names.
///
/// Used for commands that operate on a live network: `inspect`, `faucet`, `spam`, `node`.
#[derive(Clone)]
pub struct RunningDevnetCompleter;

impl ValueCandidates for RunningDevnetCompleter {
    fn candidates(&self) -> Vec<CompletionCandidate> {
        devnet_candidates(|e| e.state == DevnetState::Running)
    }
}

/// Completer that suggests **all** devnet names (Running and Stopped).
///
/// Used for `cleanup`, which can operate on stopped networks too.
#[derive(Clone)]
pub struct AllDevnetCompleter;

impl ValueCandidates for AllDevnetCompleter {
    fn candidates(&self) -> Vec<CompletionCandidate> {
        devnet_candidates(|_| true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kupcake_deploy::DevnetRegistry;
    use std::path::Path;
    use tempdir::TempDir;

    fn test_registry() -> (TempDir, DevnetRegistry) {
        let dir = TempDir::new("kupcake-completions-test").unwrap();
        let registry = DevnetRegistry::with_base_path(dir.path().to_path_buf()).unwrap();
        (dir, registry)
    }

    #[test]
    fn test_running_filter_excludes_stopped() {
        let (_dir, registry) = test_registry();
        registry
            .register("running-net", Path::new("/tmp/data-running"))
            .unwrap();
        registry
            .register("stopped-net", Path::new("/tmp/data-stopped"))
            .unwrap();
        registry.mark_stopped("stopped-net").unwrap();

        let entries = registry.list().unwrap();
        let running: Vec<_> = entries
            .iter()
            .filter(|e| e.state == DevnetState::Running)
            .collect();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].name, "running-net");
    }

    #[test]
    fn test_all_filter_returns_all() {
        let (_dir, registry) = test_registry();
        registry
            .register("running-net", Path::new("/tmp/data-running"))
            .unwrap();
        registry
            .register("stopped-net", Path::new("/tmp/data-stopped"))
            .unwrap();
        registry.mark_stopped("stopped-net").unwrap();

        let entries = registry.list().unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_empty_registry_returns_empty() {
        let (_dir, registry) = test_registry();
        let entries = registry.list().unwrap();
        assert!(entries.is_empty());
    }
}
