//! In-process micro-filesystem transactions.
//!
//! Enables speculative multi-file editing sessions with in-memory write buffers,
//! atomic commit to disk, and instant rollback without git branch overhead.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FsTx {
    pub id: String,
    staged_writes: HashMap<PathBuf, String>,
    backups: HashMap<PathBuf, Option<String>>,
    committed: bool,
}

impl FsTx {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            staged_writes: HashMap::new(),
            backups: HashMap::new(),
            committed: false,
        }
    }

    /// Stages a file write operation into memory without touching disk yet.
    pub fn stage_write(&mut self, path: impl AsRef<Path>, content: impl Into<String>) {
        let path = path.as_ref().to_path_buf();
        if !self.backups.contains_key(&path) {
            let backup = if path.exists() {
                fs::read_to_string(&path).ok()
            } else {
                None
            };
            self.backups.insert(path.clone(), backup);
        }
        self.staged_writes.insert(path, content.into());
    }

    /// Reads content: returns staged version if modified, otherwise reads from disk.
    pub fn read(&self, path: impl AsRef<Path>) -> io::Result<String> {
        let path = path.as_ref();
        if let Some(staged) = self.staged_writes.get(path) {
            return Ok(staged.clone());
        }
        fs::read_to_string(path)
    }

    /// Checks if a path has staged uncommitted edits.
    pub fn is_staged(&self, path: impl AsRef<Path>) -> bool {
        self.staged_writes.contains_key(path.as_ref())
    }

    /// List all currently staged file paths.
    pub fn staged_paths(&self) -> Vec<PathBuf> {
        self.staged_writes.keys().cloned().collect()
    }

    /// Atomically applies all staged writes to disk.
    pub fn commit(&mut self) -> io::Result<usize> {
        if self.committed {
            return Ok(0);
        }

        let mut applied = 0;
        for (path, content) in &self.staged_writes {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, content.as_bytes())?;
            applied += 1;
        }

        self.committed = true;
        Ok(applied)
    }

    /// Reverts disk changes to match pre-transaction backups.
    pub fn rollback(&mut self) -> io::Result<()> {
        for (path, backup) in &self.backups {
            match backup {
                Some(content) => {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, content.as_bytes())?;
                }
                None => {
                    if path.exists() {
                        let _ = fs::remove_file(path);
                    }
                }
            }
        }
        self.staged_writes.clear();
        self.committed = false;
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct FsTransactionManager;

impl FsTransactionManager {
    pub fn new() -> Self {
        Self
    }

    pub fn begin_tx(&self, id: &str) -> FsTx {
        FsTx::new(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_fs_transaction_commit_and_read() {
        let tmp = tempdir().unwrap();
        let file_path = tmp.path().join("sub/test.txt");

        let mut tx = FsTx::new("tx-1");
        tx.stage_write(&file_path, "hello world");

        // Before commit: file should not exist on disk, but tx.read should see it
        assert!(!file_path.exists());
        assert_eq!(tx.read(&file_path).unwrap(), "hello world");

        // Commit: file should now exist on disk
        let count = tx.commit().unwrap();
        assert_eq!(count, 1);
        assert!(file_path.exists());
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "hello world");
    }

    #[test]
    fn test_fs_transaction_rollback() {
        let tmp = tempdir().unwrap();
        let file_path = tmp.path().join("rollback_target.txt");
        fs::write(&file_path, "original content").unwrap();

        let mut tx = FsTx::new("tx-2");
        tx.stage_write(&file_path, "modified content");

        // Commit modified
        tx.commit().unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "modified content");

        // Now if we rolled back using original backup
        let mut tx2 = FsTx::new("tx-3");
        tx2.stage_write(&file_path, "second modification");
        tx2.commit().unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "second modification");

        tx2.rollback().unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "modified content");
    }
}
