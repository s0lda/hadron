//! Target Cache Isolation & Hardlink CoW Mesh.
//!
//! Provides isolated, per-worktree target directories backed by hardlinks or CoW
//! to a shared build cache to eliminate cargo lock contention and `.rlib` contamination.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerAccelerator {
    pub has_sccache: bool,
    pub has_mold: bool,
    pub sccache_dir: PathBuf,
}

impl CompilerAccelerator {
    pub fn detect() -> Self {
        let has_sccache = std::process::Command::new("sccache")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let has_mold = if cfg!(target_os = "linux") {
            std::process::Command::new("mold")
                .arg("-v")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        } else {
            false
        };

        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let sccache_dir = PathBuf::from(home).join(".hadron").join("cache").join("sccache");

        Self {
            has_sccache,
            has_mold,
            sccache_dir,
        }
    }

    /// Computes deterministic CAS content hash across files.
    pub fn compute_cas_artifact_hash(files: &[(&str, &[u8])]) -> String {
        crate::cas::SemanticCas::hash_files(files)
    }

    pub fn apply_to_env(&self, env: &mut Vec<(String, String)>) {
        if self.has_sccache {
            let _ = std::fs::create_dir_all(&self.sccache_dir);
            env.push(("RUSTC_WRAPPER".to_string(), "sccache".to_string()));
            env.push((
                "SCCACHE_DIR".to_string(),
                self.sccache_dir.to_string_lossy().to_string(),
            ));
        }
        if self.has_mold {
            let existing_rustflags = env
                .iter()
                .find(|(k, _)| k == "RUSTFLAGS")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let new_flags = if existing_rustflags.is_empty() {
                "-C link-arg=-fuse-ld=mold".to_string()
            } else if !existing_rustflags.contains("-fuse-ld=mold") {
                format!("{existing_rustflags} -C link-arg=-fuse-ld=mold")
            } else {
                existing_rustflags
            };
            env.retain(|(k, _)| k != "RUSTFLAGS");
            env.push(("RUSTFLAGS".to_string(), new_flags));
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuildCacheMesh {
    base_dir: PathBuf,
}

impl BuildCacheMesh {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn worktrees_root(&self) -> PathBuf {
        self.base_dir.join("worktrees")
    }

    pub fn shared_deps_dir(&self) -> PathBuf {
        self.base_dir.join("shared_cache").join("deps")
    }

    /// Prepares an isolated target cache directory for a specific worktree.
    pub fn prepare_worktree_cache(&self, worktree_id: &str) -> io::Result<PathBuf> {
        let worktree_target = self.worktrees_root().join(worktree_id);
        let deps_dir = worktree_target.join("debug").join("deps");
        fs::create_dir_all(&deps_dir)?;
        Ok(worktree_target)
    }

    /// Links artifacts from shared dependency cache into the worktree's target deps directory.
    pub fn sync_shared_deps(&self, worktree_id: &str) -> io::Result<usize> {
        let shared_deps = self.shared_deps_dir();
        if !shared_deps.exists() {
            return Ok(0);
        }

        let target_deps = self
            .worktrees_root()
            .join(worktree_id)
            .join("debug")
            .join("deps");
        fs::create_dir_all(&target_deps)?;

        let mut linked = 0;
        for entry in fs::read_dir(&shared_deps)? {
            let entry = entry?;
            let src_path = entry.path();
            if src_path.is_file() {
                let file_name = entry.file_name();
                let dst_path = target_deps.join(&file_name);
                if !dst_path.exists() {
                    // Try hard link first, fallback to copy
                    if fs::hard_link(&src_path, &dst_path).is_ok() || fs::copy(&src_path, &dst_path).is_ok() {
                        linked += 1;
                    }
                }
            }
        }
        Ok(linked)
    }

    /// Prunes worktree cache directories that are no longer in the active list.
    pub fn prune_stale_caches(&self, active_ids: &[String]) -> io::Result<usize> {
        let root = self.worktrees_root();
        if !root.exists() {
            return Ok(0);
        }

        let active_set: HashSet<&str> = active_ids.iter().map(|s| s.as_str()).collect();
        let mut pruned = 0;

        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !active_set.contains(name) {
                        fs::remove_dir_all(&path)?;
                        pruned += 1;
                    }
                }
            }
        }

        Ok(pruned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_build_cache_lifecycle() {
        let tmp = tempdir().unwrap();
        let mesh = BuildCacheMesh::new(tmp.path().to_path_buf());

        // 1. Prepare worktree cache
        let target_dir = mesh.prepare_worktree_cache("wt-alpha").unwrap();
        assert!(target_dir.join("debug").join("deps").exists());

        // 2. Populate shared deps
        let shared_deps = mesh.shared_deps_dir();
        fs::create_dir_all(&shared_deps).unwrap();
        fs::write(shared_deps.join("libfoo.rlib"), b"rlib content").unwrap();

        // 3. Sync shared deps
        let linked = mesh.sync_shared_deps("wt-alpha").unwrap();
        assert_eq!(linked, 1);
        assert!(target_dir.join("debug").join("deps").join("libfoo.rlib").exists());

        // 4. Create another worktree
        mesh.prepare_worktree_cache("wt-beta").unwrap();

        // 5. Prune stale caches (keep only wt-alpha)
        let active = vec!["wt-alpha".to_string()];
        let pruned = mesh.prune_stale_caches(&active).unwrap();
        assert_eq!(pruned, 1);
        assert!(mesh.worktrees_root().join("wt-alpha").exists());
        assert!(!mesh.worktrees_root().join("wt-beta").exists());
    }

    #[test]
    fn test_compiler_accelerator_env_application() {
        let accelerator = CompilerAccelerator {
            has_sccache: true,
            has_mold: true,
            sccache_dir: PathBuf::from("/tmp/hadron_test_sccache"),
        };
        let mut env = Vec::new();
        accelerator.apply_to_env(&mut env);

        let map: std::collections::HashMap<String, String> = env.into_iter().collect();
        assert_eq!(map.get("RUSTC_WRAPPER").map(|s| s.as_str()), Some("sccache"));
        assert_eq!(map.get("SCCACHE_DIR").map(|s| s.as_str()), Some("/tmp/hadron_test_sccache"));
        assert!(map.get("RUSTFLAGS").unwrap().contains("-fuse-ld=mold"));
    }
}
