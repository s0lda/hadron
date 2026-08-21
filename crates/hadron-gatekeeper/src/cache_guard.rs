//! Unified Shared-Target Build Cache Validation and Metadata Salting.
//!
//! Enforces rustc `-C metadata` salting per worktree and crate to eliminate foreign `.rlib`
//! cache collisions in the shared cargo target directory.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Compute deterministic cache salt for a worktree and crate to avoid cross-worktree rlib collisions.
pub fn compute_cache_salt(crate_name: &str, tree_path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    crate_name.hash(&mut hasher);
    tree_path.to_string_lossy().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Inject RUSTFLAGS with metadata salt into a cargo command to prevent foreign rlib collision in shared target dir.
pub fn inject_cache_isolation_flags(cmd: &mut Command, crate_name: &str, tree_path: &Path) {
    let salt = compute_cache_salt(crate_name, tree_path);
    let flag = format!("-C metadata={}", salt);

    let current_flags = std::env::var("RUSTFLAGS").unwrap_or_default();
    let new_flags = if current_flags.trim().is_empty() {
        flag
    } else {
        format!("{} {}", current_flags.trim(), flag)
    };
    cmd.env("RUSTFLAGS", new_flags);
}

/// Verification report for shared target directory integrity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheIntegrityReport {
    pub total_artifacts: usize,
    pub corrupted_artifacts: Vec<PathBuf>,
    pub is_valid: bool,
}

/// Inspect target directory rlib freshness and file validity.
pub fn verify_shared_rlib_integrity(target_dir: &Path) -> std::io::Result<CacheIntegrityReport> {
    let deps_dir = target_dir.join("debug").join("deps");
    if !deps_dir.exists() {
        return Ok(CacheIntegrityReport {
            total_artifacts: 0,
            corrupted_artifacts: Vec::new(),
            is_valid: true,
        });
    }

    let mut total = 0;
    let mut corrupted = Vec::new();

    for entry in fs::read_dir(&deps_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("rlib") {
            total += 1;
            if let Ok(bytes) = fs::read(&path) {
                if bytes.is_empty() || (bytes.len() >= 8 && &bytes[..8] != b"!<arch>\n") {
                    corrupted.push(path);
                }
            } else {
                corrupted.push(path);
            }
        }
    }

    let is_valid = corrupted.is_empty();
    Ok(CacheIntegrityReport {
        total_artifacts: total,
        corrupted_artifacts: corrupted,
        is_valid,
    })
}
