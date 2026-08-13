//! `sccache`-backed compiler environment configuration for isolated worktree builds.
//!
//! Replaces single shared `target/` directory contention with isolated per-worktree
//! targets backed by a shared `sccache` compiler caching daemon, avoiding `.rlib`
//! timestamp and collision races while maintaining high compilation throughput.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Checks whether the `sccache` binary is installed and executable in PATH.
pub fn is_sccache_available() -> bool {
    Command::new("sccache")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Configures compiler environment for worktree-isolated builds backed by `sccache`.
///
/// Ensures:
/// - `RUSTC_WRAPPER` is set to "sccache"
/// - `CARGO_TARGET_DIR` points to the worktree's own target directory
/// - `SCCACHE_DIR` points to a shared caching location in user hadron dir
/// - `CARGO_INCREMENTAL` is set to "0" (as sccache cannot cache incremental artifacts)
pub fn configure_sccache_worktree_env(worktree: &Path) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("RUSTC_WRAPPER".to_string(), "sccache".to_string());
    env.insert(
        "CARGO_TARGET_DIR".to_string(),
        worktree.join("target").to_string_lossy().to_string(),
    );
    let sccache_dir = hadron_lattice::user_hadron_dir()
        .map(|d| d.join("sccache"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/hadron-sccache"));
    env.insert(
        "SCCACHE_DIR".to_string(),
        sccache_dir.to_string_lossy().to_string(),
    );
    env.insert("CARGO_INCREMENTAL".to_string(), "0".to_string());
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sccache_env_configures_isolated_target_with_shared_compiler_cache() {
        let env = configure_sccache_worktree_env(Path::new("/tmp/worktree-1"));
        assert_eq!(env.get("RUSTC_WRAPPER").map(|s| s.as_str()), Some("sccache"));
        assert_eq!(
            env.get("CARGO_TARGET_DIR").map(|s| s.as_str()),
            Some("/tmp/worktree-1/target")
        );
        assert!(env.contains_key("SCCACHE_DIR"));
        assert_eq!(env.get("CARGO_INCREMENTAL").map(|s| s.as_str()), Some("0"));
    }

    #[test]
    fn checks_sccache_availability() {
        // Just verify is_sccache_available() executes cleanly without panic
        let _ = is_sccache_available();
    }
}
