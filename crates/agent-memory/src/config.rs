//! Database-path resolution and data-directory creation.
//!
//! This is the ONE place that touches platform paths (`dirs`) — keep it here so
//! Windows support (deferred to v2) is a single-file change, not a scattered
//! `cfg(unix)` hunt.

use std::path::PathBuf;

use anyhow::Context;

/// Resolve the SQLite database path with precedence (D-01/D-02):
///   1. the `--db <PATH>` flag, if given
///   2. the `AGENT_MEMORY_DB` environment variable
///   3. `<data_dir>/agent-memory/memory.db` (D-01 — `data_dir`, not `config_dir`)
///
/// The parent directory is created (mode `0700` on Unix) if it does not exist, so
/// other local users cannot read the memory corpus (threat T-01-03 / Security V4).
pub fn resolve_db_path(flag: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = if let Some(flag_path) = flag {
        flag_path
    } else if let Some(env_path) = std::env::var_os("AGENT_MEMORY_DB") {
        PathBuf::from(env_path)
    } else {
        let data_dir = dirs::data_dir()
            .context("could not determine the OS data directory for the default database path")?;
        data_dir.join("agent-memory").join("memory.db")
    };

    if let Some(parent) = path.parent() {
        create_data_dir(parent)
            .with_context(|| format!("creating data directory {}", parent.display()))?;
    }

    Ok(path)
}

/// Create `dir` (and parents) if missing, restricting it to the owner on Unix.
#[cfg(unix)]
fn create_data_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if dir.exists() {
        return Ok(());
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// Non-Unix fallback (Windows support is deferred to v2 — DIST-03).
#[cfg(not(unix))]
fn create_data_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_takes_precedence_over_env_and_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let explicit = dir.path().join("explicit").join("memory.db");
        let resolved = resolve_db_path(Some(explicit.clone())).expect("resolve");
        assert_eq!(resolved, explicit);
        assert!(explicit.parent().expect("parent").exists());
    }

    #[test]
    fn env_var_is_used_when_no_flag() {
        let dir = tempfile::tempdir().expect("tempdir");
        let env_path = dir.path().join("env").join("memory.db");
        // SAFETY: single-threaded test; no other thread reads the env concurrently.
        std::env::set_var("AGENT_MEMORY_DB", &env_path);
        let resolved = resolve_db_path(None).expect("resolve");
        std::env::remove_var("AGENT_MEMORY_DB");
        assert_eq!(resolved, env_path);
    }
}
