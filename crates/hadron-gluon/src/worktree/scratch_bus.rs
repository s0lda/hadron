//! Shared Cross-Worktree Scratch Bus.
//!
//! Provides a shared, git-untracked scratch directory across active worker quarks
//! for real-time exchange of plans, specs, execution traces, and ephemeral notes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SharedScratchBus {
    scratch_dir: PathBuf,
}

impl SharedScratchBus {
    pub fn new(repo_root: &Path) -> Self {
        let scratch_dir = repo_root.join(".hadron").join("scratch");
        Self { scratch_dir }
    }

    /// Ensure the shared repository scratch directory exists.
    pub fn ensure_dir(&self) -> io::Result<&Path> {
        fs::create_dir_all(&self.scratch_dir)?;
        Ok(&self.scratch_dir)
    }

    pub fn scratch_dir(&self) -> &Path {
        &self.scratch_dir
    }

    /// Projects the shared scratch directory into a specific worktree.
    ///
    /// Creates `<worktree_path>/.hadron/scratch` as a symlink pointing to
    /// `<repo_root>/.hadron/scratch`. If symlink creation is not permitted,
    /// ensures the directory exists as a fallback.
    pub fn project_into_worktree(&self, worktree_path: &Path) -> io::Result<PathBuf> {
        self.ensure_dir()?;
        let target_hadron = worktree_path.join(".hadron");
        fs::create_dir_all(&target_hadron)?;
        let target_scratch = target_hadron.join("scratch");

        if target_scratch.exists() || target_scratch.is_symlink() {
            return Ok(target_scratch);
        }

        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(&self.scratch_dir, &target_scratch).is_ok() {
                return Ok(target_scratch);
            }
        }

        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(&self.scratch_dir, &target_scratch).is_ok() {
                return Ok(target_scratch);
            }
        }

        fs::create_dir_all(&target_scratch)?;
        Ok(target_scratch)
    }

    /// Write an entry to the shared scratch bus.
    pub fn write_entry(&self, name: &str, content: &[u8]) -> io::Result<PathBuf> {
        self.ensure_dir()?;
        let file_path = self.scratch_dir.join(name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file_path, content)?;
        Ok(file_path)
    }

    /// Read an entry from the shared scratch bus.
    pub fn read_entry(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let file_path = self.scratch_dir.join(name);
        if !file_path.is_file() {
            return Ok(None);
        }
        fs::read(file_path).map(Some)
    }

    /// List all entries currently in the shared scratch bus.
    pub fn list_entries(&self) -> io::Result<Vec<String>> {
        if !self.scratch_dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.scratch_dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                entries.push(name.to_string());
            }
        }
        entries.sort();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_shared_scratch_bus_lifecycle() {
        let repo = tempdir().unwrap();
        let wt = tempdir().unwrap();

        let bus = SharedScratchBus::new(repo.path());
        assert!(bus.ensure_dir().is_ok());

        // 1. Write an entry from one perspective
        let path = bus.write_entry("spec-alpha.md", b"# Feature Alpha Spec").unwrap();
        assert!(path.exists());

        // 2. Read the entry
        let content = bus.read_entry("spec-alpha.md").unwrap().unwrap();
        assert_eq!(content, b"# Feature Alpha Spec");

        // 3. List entries
        let list = bus.list_entries().unwrap();
        assert_eq!(list, vec!["spec-alpha.md".to_string()]);

        // 4. Project into a worktree
        let projected = bus.project_into_worktree(wt.path()).unwrap();
        assert!(projected.exists());

        // If symlink worked, entry is visible from projected path
        if projected.is_symlink() {
            assert!(projected.join("spec-alpha.md").exists());
        }
    }
}
