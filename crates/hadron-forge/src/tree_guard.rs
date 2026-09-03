//! Stale Buffer & Concurrent Mutation Inotify Guard.
//!
//! Watches buffer snapshots and modification timestamps, detecting external changes
//! or sibling worktree mutations to prevent destructive overwrites.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleBufferConflict {
    pub path: PathBuf,
    pub expected_hash: String,
    pub disk_hash: String,
}

impl std::fmt::Display for StaleBufferConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Stale buffer conflict on {:?}: expected hash {}, found on disk {}",
            self.path, self.expected_hash, self.disk_hash
        )
    }
}

impl std::error::Error for StaleBufferConflict {}

#[derive(Debug, Clone)]
pub struct WatchedSnapshot {
    pub hash: String,
    pub mtime: SystemTime,
}

#[derive(Debug, Default, Clone)]
pub struct TreeGuard {
    watched: HashMap<PathBuf, WatchedSnapshot>,
}

impl TreeGuard {
    pub fn new() -> Self {
        Self {
            watched: HashMap::new(),
        }
    }

    /// Computes the blake3 hash of a byte slice.
    pub fn hash_bytes(bytes: &[u8]) -> String {
        blake3::hash(bytes).to_hex().to_string()
    }

    /// Watches a file by computing its current hash and recording its mtime.
    pub fn watch_file(&mut self, path: impl AsRef<Path>) -> io::Result<String> {
        let path = path.as_ref().to_path_buf();
        let metadata = fs::metadata(&path)?;
        let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let content = fs::read(&path)?;
        let hash = Self::hash_bytes(&content);

        self.watched.insert(path, WatchedSnapshot {
            hash: hash.clone(),
            mtime,
        });

        Ok(hash)
    }

    /// Verifies that the file on disk has not diverged from the watched snapshot.
    pub fn verify_snapshot(&self, path: impl AsRef<Path>) -> Result<(), StaleBufferConflict> {
        let path = path.as_ref().to_path_buf();
        let snapshot = match self.watched.get(&path) {
            Some(s) => s,
            None => return Ok(()),
        };

        let current_bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => {
                return Err(StaleBufferConflict {
                    path: path.clone(),
                    expected_hash: snapshot.hash.clone(),
                    disk_hash: "<unreadable/deleted>".to_string(),
                });
            }
        };

        let disk_hash = Self::hash_bytes(&current_bytes);
        if disk_hash != snapshot.hash {
            return Err(StaleBufferConflict {
                path,
                expected_hash: snapshot.hash.clone(),
                disk_hash,
            });
        }

        Ok(())
    }

    pub fn unwatch(&mut self, path: impl AsRef<Path>) {
        self.watched.remove(path.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_tree_guard_detects_mutation() {
        let tmp = tempdir().unwrap();
        let target = tmp.path().join("guarded.txt");
        fs::write(&target, b"initial content").unwrap();

        let mut guard = TreeGuard::new();
        let hash = guard.watch_file(&target).unwrap();
        assert!(!hash.is_empty());

        // Baseline verification passes
        assert!(guard.verify_snapshot(&target).is_ok());

        // External modification
        fs::write(&target, b"mutated content by external process").unwrap();

        // Verification fails with StaleBufferConflict
        let err = guard.verify_snapshot(&target).unwrap_err();
        assert_eq!(err.expected_hash, hash);
        assert_ne!(err.disk_hash, hash);
    }
}
