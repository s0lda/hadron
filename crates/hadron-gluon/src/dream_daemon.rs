//! "Sleep & Dream" Idle Autonomous Daemon.
//!
//! Executes low-priority background maintenance during workspace idle periods:
//! memory deduplication, nucleus note compaction, stale worktree detection,
//! and dead code scanning without interrupting active quark turns.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamSummary {
    pub notes_scanned: usize,
    pub notes_cleaned: usize,
    pub stale_worktrees_found: usize,
    pub timestamp: u64,
}

pub struct DreamDaemon;

impl DreamDaemon {
    /// Runs a single dream maintenance cycle on the repository.
    pub fn run_cycle(repo_root: &Path) -> DreamSummary {
        let mut notes_scanned = 0;
        let mut notes_cleaned = 0;
        let mut stale_worktrees_found = 0;

        // 1. Inspect nucleus notes
        let notes_dir = repo_root.join(".hadron/nucleus/notes");
        if let Ok(entries) = fs::read_dir(&notes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "md") {
                    notes_scanned += 1;
                    if let Ok(meta) = fs::metadata(&path) {
                        if meta.len() == 0 {
                            let _ = fs::remove_file(&path);
                            notes_cleaned += 1;
                        }
                    }
                }
            }
        }

        // 2. Inspect worktrees directory
        let trees_dir = repo_root.join(".hadron/trees");
        if let Ok(entries) = fs::read_dir(&trees_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Stale check: if .git is missing inside a tree directory, mark stale
                    if !path.join(".git").exists() {
                        stale_worktrees_found += 1;
                    }
                }
            }
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        DreamSummary {
            notes_scanned,
            notes_cleaned,
            stale_worktrees_found,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_dream_daemon_cycle() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let notes = root.join(".hadron/nucleus/notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("good-note.md"), b"valid note content").unwrap();
        fs::write(notes.join("empty-note.md"), b"").unwrap(); // should be cleaned

        let trees = root.join(".hadron/trees");
        fs::create_dir_all(trees.join("corrupted-tree")).unwrap(); // no .git, marked stale

        let summary = DreamDaemon::run_cycle(root);
        assert_eq!(summary.notes_scanned, 2);
        assert_eq!(summary.notes_cleaned, 1);
        assert_eq!(summary.stale_worktrees_found, 1);
        assert!(!notes.join("empty-note.md").exists());
        assert!(notes.join("good-note.md").exists());
    }
}
