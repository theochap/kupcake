//! TOML `include` directive resolution.
//!
//! Recursively walks a [`toml::Value`] tree and inlines any tables that contain
//! an `include` key pointing to an external TOML file. This enables splitting
//! large `Kupcake.toml` files into smaller, focused config files.
//!
//! # Example
//!
//! ```toml
//! [l2_stack.op_batcher]
//! include = "./configs/batcher.toml"
//!
//! [[l2_stack.sequencers]]
//! include = "./configs/seq-0.toml"
//! ```
//!
//! The referenced file's contents replace the table entirely.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Recursively resolve `include` directives in a TOML value tree.
///
/// When a table contains an `include` key with a string value, the referenced
/// file is read, parsed as TOML, and its contents replace the table.
///
/// Paths are resolved relative to `base_dir` (typically the parent directory
/// of the config file being parsed).
///
/// Circular includes are detected and return an error.
pub fn resolve_includes(value: &mut toml::Value, base_dir: &Path) -> Result<()> {
    let mut visited = HashSet::new();
    resolve_recursive(value, base_dir, &mut visited)
}

fn resolve_recursive(
    value: &mut toml::Value,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<()> {
    match value {
        toml::Value::Table(table) => {
            // Check if this table has an `include` directive
            if let Some(toml::Value::String(include_path)) = table.get("include") {
                let resolved_path = base_dir.join(include_path);
                let canonical = resolved_path.canonicalize().with_context(|| {
                    format!(
                        "Failed to resolve include path '{}' (relative to '{}')",
                        include_path,
                        base_dir.display()
                    )
                })?;

                if !visited.insert(canonical.clone()) {
                    anyhow::bail!(
                        "Circular include detected: '{}' has already been included",
                        canonical.display()
                    );
                }

                let content = std::fs::read_to_string(&canonical).with_context(|| {
                    format!("Failed to read included file '{}'", canonical.display())
                })?;

                let mut included: toml::Value = toml::from_str(&content).with_context(|| {
                    format!(
                        "Failed to parse included file '{}' as TOML",
                        canonical.display()
                    )
                })?;

                // Recursively resolve includes in the included file
                let included_base = canonical.parent().unwrap_or(base_dir);
                resolve_recursive(&mut included, included_base, visited)?;

                // Backtrack: allow the same file to be included from different paths
                // (diamond includes). Only circular chains within a single path are blocked.
                visited.remove(&canonical);

                // Replace the current table with the included content
                *value = included;
                return Ok(());
            }

            // No include directive — recurse into child values
            for (_, child) in table.iter_mut() {
                resolve_recursive(child, base_dir, visited)?;
            }
        }
        toml::Value::Array(arr) => {
            for item in arr.iter_mut() {
                resolve_recursive(item, base_dir, visited)?;
            }
        }
        // Scalar values: nothing to resolve
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_no_includes_passthrough() {
        let mut value: toml::Value = toml::from_str(
            r#"
            [deployer]
            name = "test"
            block_time = 4
            "#,
        )
        .unwrap();

        let original = value.clone();
        resolve_includes(&mut value, Path::new("/tmp")).unwrap();
        assert_eq!(value, original);
    }

    #[test]
    fn test_include_replaces_table() {
        let dir = tempdir::TempDir::new("kupcake-include-test").unwrap();
        let base = dir.path();

        // Write the included file
        let batcher_content = r#"
            container_name = "my-batcher"
            docker_image = "custom-image"
        "#;
        fs::write(base.join("batcher.toml"), batcher_content).unwrap();

        // Main config with include
        let mut value: toml::Value = toml::from_str(
            r#"
            [op_batcher]
            include = "./batcher.toml"
            "#,
        )
        .unwrap();

        resolve_includes(&mut value, base).unwrap();

        let batcher = value.get("op_batcher").unwrap();
        assert_eq!(
            batcher.get("container_name").unwrap().as_str().unwrap(),
            "my-batcher"
        );
        assert_eq!(
            batcher.get("docker_image").unwrap().as_str().unwrap(),
            "custom-image"
        );
        // include key should be gone
        assert!(batcher.get("include").is_none());
    }

    #[test]
    fn test_include_in_array() {
        let dir = tempdir::TempDir::new("kupcake-include-array-test").unwrap();
        let base = dir.path();

        fs::write(base.join("seq0.toml"), r#"container_name = "seq-0""#).unwrap();
        fs::write(base.join("seq1.toml"), r#"container_name = "seq-1""#).unwrap();

        let mut value: toml::Value = toml::from_str(
            r#"
            [[sequencers]]
            include = "./seq0.toml"

            [[sequencers]]
            include = "./seq1.toml"
            "#,
        )
        .unwrap();

        resolve_includes(&mut value, base).unwrap();

        let sequencers = value.get("sequencers").unwrap().as_array().unwrap();
        assert_eq!(sequencers.len(), 2);
        assert_eq!(
            sequencers[0]
                .get("container_name")
                .unwrap()
                .as_str()
                .unwrap(),
            "seq-0"
        );
        assert_eq!(
            sequencers[1]
                .get("container_name")
                .unwrap()
                .as_str()
                .unwrap(),
            "seq-1"
        );
    }

    #[test]
    fn test_nested_includes() {
        let dir = tempdir::TempDir::new("kupcake-nested-include-test").unwrap();
        let base = dir.path();

        // Inner file
        fs::write(base.join("inner.toml"), r#"value = "from-inner""#).unwrap();

        // Outer file includes inner
        fs::write(
            base.join("outer.toml"),
            r#"
            [nested]
            include = "./inner.toml"
            "#,
        )
        .unwrap();

        let mut value: toml::Value = toml::from_str(
            r#"
            [section]
            include = "./outer.toml"
            "#,
        )
        .unwrap();

        resolve_includes(&mut value, base).unwrap();

        let inner_value = value
            .get("section")
            .unwrap()
            .get("nested")
            .unwrap()
            .get("value")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(inner_value, "from-inner");
    }

    #[test]
    fn test_circular_include_detected() {
        let dir = tempdir::TempDir::new("kupcake-circular-test").unwrap();
        let base = dir.path();

        // a.toml includes b.toml, b.toml includes a.toml
        fs::write(
            base.join("a.toml"),
            r#"
            [child]
            include = "./b.toml"
            "#,
        )
        .unwrap();
        fs::write(
            base.join("b.toml"),
            r#"
            [child]
            include = "./a.toml"
            "#,
        )
        .unwrap();

        let mut value: toml::Value = toml::from_str(
            r#"
            [section]
            include = "./a.toml"
            "#,
        )
        .unwrap();

        let result = resolve_includes(&mut value, base);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Circular include"),
            "Expected circular include error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_missing_include_file_errors() {
        let dir = tempdir::TempDir::new("kupcake-missing-test").unwrap();
        let base = dir.path();

        let mut value: toml::Value = toml::from_str(
            r#"
            [section]
            include = "./nonexistent.toml"
            "#,
        )
        .unwrap();

        let result = resolve_includes(&mut value, base);
        assert!(result.is_err());
    }

    #[test]
    fn test_include_key_as_non_string_ignored() {
        // If `include` is not a string (e.g., a number), it's not an include directive
        let mut value: toml::Value = toml::from_str(
            r#"
            [section]
            include = 42
            name = "test"
            "#,
        )
        .unwrap();

        let original = value.clone();
        resolve_includes(&mut value, Path::new("/tmp")).unwrap();
        assert_eq!(value, original);
    }
}
