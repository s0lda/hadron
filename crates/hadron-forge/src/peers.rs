//! Cross-worktree peer inspector.
//!
//! Provides read-only visibility into sibling quark worktrees located in
//! `.hadron/trees/<peer-id>`. Allows querying peer branch names, latest commit
//! metadata, dirty file status, and commits ahead of base.

use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};

use crate::file::{ForgeError, Root};

/// Summary of a peer quark's worktree state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerWorktreeInfo {
    pub peer_id: String,
    pub worktree_path: String,
    pub branch: String,
    pub latest_commit_hash: Option<String>,
    pub latest_commit_subject: Option<String>,
    pub latest_commit_author: Option<String>,
    pub is_dirty: bool,
    pub modified_files: Vec<String>,
    pub commits_ahead_base: usize,
}

/// Conflict severity between concurrent worktrees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictSeverity {
    Warning,
    DirectCollision,
}

/// Description of a cross-worktree file or symbol conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeConflict {
    pub file_path: String,
    pub conflicting_peers: Vec<String>,
    pub branches: Vec<String>,
    pub severity: ConflictSeverity,
    pub description: String,
}

/// Comprehensive cross-worktree collision report across .hadron/trees/*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerConflictReport {
    pub total_peers_inspected: usize,
    pub conflicts_detected: usize,
    pub conflicts: Vec<WorktreeConflict>,
    pub summary: String,
}


/// Derive the `.hadron/trees` directory from any worktree or project path.
pub fn derive_trees_dir(project: &Path) -> Result<PathBuf, ForgeError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .map_err(|e| ForgeError::Io(e.to_string()))?;

    if !output.status.success() {
        return Err(ForgeError::Io(format!(
            "failed to find main git repo root from {}: {}",
            project.display(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let git_common = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let repo_root = git_common
        .parent()
        .ok_or_else(|| ForgeError::Io(format!("git common dir has no parent: {}", git_common.display())))?;

    Ok(repo_root.join(".hadron").join("trees"))
}

/// Inspect a specific peer worktree by its peer_id.
pub fn inspect_peer_worktree(project_root: &Root, peer_id: &str) -> Result<PeerWorktreeInfo, ForgeError> {
    let clean_id = peer_id.trim();
    if clean_id.is_empty() || clean_id.contains('/') || clean_id.contains('\\') || clean_id.contains("..") {
        return Err(ForgeError::Rejected(format!("invalid peer_id: {peer_id:?}")));
    }

    let trees_dir = derive_trees_dir(project_root.path())?;
    let peer_dir = trees_dir.join(clean_id);

    if !peer_dir.is_dir() {
        return Err(ForgeError::Io(format!("peer worktree not found at {}", peer_dir.display())));
    }

    inspect_single_peer_dir(clean_id, &peer_dir)
}

/// List and inspect all sibling peer worktrees under `.hadron/trees/`.
pub fn list_peer_worktrees(project_root: &Root) -> Result<Vec<PeerWorktreeInfo>, ForgeError> {
    let trees_dir = match derive_trees_dir(project_root.path()) {
        Ok(dir) => dir,
        Err(_) => return Ok(Vec::new()),
    };

    if !trees_dir.is_dir() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(&trees_dir).map_err(|e| ForgeError::Io(e.to_string()))?;
    let mut peers = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let id = entry.file_name().to_string_lossy().to_string();
            if let Ok(info) = inspect_single_peer_dir(&id, &path) {
                peers.push(info);
            }
        }
    }

    peers.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
    Ok(peers)
}

/// Detect conflicting and overlapping file edits across all sibling worktrees.
pub fn detect_cross_worktree_conflicts(project_root: &Root) -> Result<PeerConflictReport, ForgeError> {
    let peers = list_peer_worktrees(project_root)?;
    let mut file_to_peers: std::collections::HashMap<String, Vec<(String, String)>> = std::collections::HashMap::new();

    for peer in &peers {
        // Collect dirty modified files
        for file in &peer.modified_files {
            file_to_peers
                .entry(file.clone())
                .or_default()
                .push((peer.peer_id.clone(), peer.branch.clone()));
        }

        // Check committed files ahead of base if commits_ahead_base > 0
        if peer.commits_ahead_base > 0 {
            let log_out = Command::new("git")
                .arg("-C")
                .arg(&peer.worktree_path)
                .args(&["diff", "--name-only", "main..HEAD"])
                .output();

            if let Ok(out) = log_out {
                if out.status.success() {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let f = line.trim();
                        if !f.is_empty() {
                            let entry = file_to_peers.entry(f.to_string()).or_default();
                            if !entry.iter().any(|(p, _)| p == &peer.peer_id) {
                                entry.push((peer.peer_id.clone(), peer.branch.clone()));
                            }
                        }
                    }
                }
            }
        }
    }

    let mut conflicts = Vec::new();
    for (file_path, peer_list) in file_to_peers {
        if peer_list.len() >= 2 {
            let conflicting_peers: Vec<String> = peer_list.iter().map(|(p, _)| p.clone()).collect();
            let branches: Vec<String> = peer_list.iter().map(|(_, b)| b.clone()).collect();
            conflicts.push(WorktreeConflict {
                file_path: file_path.clone(),
                conflicting_peers: conflicting_peers.clone(),
                branches,
                severity: ConflictSeverity::DirectCollision,
                description: format!(
                    "File '{}' modified concurrently across {} peer worktree(s): {}",
                    file_path,
                    conflicting_peers.len(),
                    conflicting_peers.join(", ")
                ),
            });
        }
    }

    conflicts.sort_by(|a, b| a.file_path.cmp(&b.file_path));

    let conflicts_detected = conflicts.len();
    let total_peers_inspected = peers.len();
    let summary = if conflicts_detected > 0 {
        format!("Cross-worktree collision WARNING: {conflicts_detected} file(s) concurrently modified across {total_peers_inspected} active peer trees")
    } else {
        format!("Cross-worktree check CLEAN: 0 file collisions across {total_peers_inspected} active peer trees")
    };

    Ok(PeerConflictReport {
        total_peers_inspected,
        conflicts_detected,
        conflicts,
        summary,
    })
}


fn inspect_single_peer_dir(peer_id: &str, peer_dir: &Path) -> Result<PeerWorktreeInfo, ForgeError> {
    // 1. Branch name
    let branch_out = Command::new("git")
        .arg("-C")
        .arg(peer_dir)
        .args(&["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| ForgeError::Io(e.to_string()))?;

    let branch = String::from_utf8_lossy(&branch_out.stdout).trim().to_string();

    // 2. Latest commit
    let log_out = Command::new("git")
        .arg("-C")
        .arg(peer_dir)
        .args(&["log", "-1", "--format=%H%x1f%s%x1f%an"])
        .output();

    let (latest_commit_hash, latest_commit_subject, latest_commit_author) = match log_out {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut parts = s.trim().split('\x1f');
            let hash = parts.next().filter(|h| !h.is_empty()).map(ToString::to_string);
            let subj = parts.next().map(ToString::to_string);
            let auth = parts.next().map(ToString::to_string);
            (hash, subj, auth)
        }
        _ => (None, None, None),
    };

    // 3. Status & dirty files
    let status_out = Command::new("git")
        .arg("-C")
        .arg(peer_dir)
        .args(&["status", "--porcelain"])
        .output();

    let (is_dirty, modified_files) = match status_out {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            let files: Vec<String> = s
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.chars().skip(3).collect::<String>().trim().to_string())
                .collect();
            let dirty = !files.is_empty();
            (dirty, files)
        }
        _ => (false, Vec::new()),
    };

    // 4. Commits ahead of base
    let count_out = Command::new("git")
        .arg("-C")
        .arg(peer_dir)
        .args(&["rev-list", "--count", "main..HEAD"])
        .output();

    let commits_ahead_base = match count_out {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0)
        }
        _ => 0,
    };

    Ok(PeerWorktreeInfo {
        peer_id: peer_id.to_string(),
        worktree_path: peer_dir.display().to_string(),
        branch,
        latest_commit_hash,
        latest_commit_subject,
        latest_commit_author,
        is_dirty,
        modified_files,
        commits_ahead_base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_multitree_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main-repo");
        std::fs::create_dir_all(&main).unwrap();

        let run = |args: &[&str], cwd: &Path| {
            let status = Command::new("git")
                .args(args)
                .current_dir(cwd)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed in {}", cwd.display());
        };

        run(&["init", "-q"], &main);
        run(&["config", "user.email", "test@test.com"], &main);
        run(&["config", "user.name", "Tester"], &main);
        std::fs::write(main.join("file.txt"), "hello\n").unwrap();
        run(&["add", "file.txt"], &main);
        run(&["commit", "-q", "-m", "initial commit"], &main);

        let trees = main.join(".hadron").join("trees");
        std::fs::create_dir_all(&trees).unwrap();

        let peer_a = trees.join("peer-alpha");
        run(&["worktree", "add", "-q", "-b", "quark/peer-alpha/feat1", peer_a.to_str().unwrap()], &main);
        std::fs::write(peer_a.join("peer_a.txt"), "work from alpha\n").unwrap();

        let peer_b = trees.join("peer-beta");
        run(&["worktree", "add", "-q", "-b", "quark/peer-beta/feat2", peer_b.to_str().unwrap()], &main);

        (tmp, main, peer_a)
    }

    #[test]
    fn inspect_peer_worktree_reads_peer_status() {
        let (_tmp, _main, peer_a) = fixture_multitree_repo();
        let root = Root::new(&peer_a);

        let peer_info = inspect_peer_worktree(&root, "peer-alpha").unwrap();
        assert_eq!(peer_info.peer_id, "peer-alpha");
        assert_eq!(peer_info.branch, "quark/peer-alpha/feat1");
        assert!(peer_info.is_dirty);
        assert!(peer_info.modified_files.contains(&"peer_a.txt".to_string()));
    }

    #[test]
    fn list_peer_worktrees_finds_all_siblings() {
        let (_tmp, _main, peer_a) = fixture_multitree_repo();
        let root = Root::new(&peer_a);

        let peers = list_peer_worktrees(&root).unwrap();
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].peer_id, "peer-alpha");
        assert_eq!(peers[1].peer_id, "peer-beta");
    }

    #[test]
    fn inspect_peer_rejects_path_traversal() {
        let (_tmp, _main, peer_a) = fixture_multitree_repo();
        let root = Root::new(&peer_a);

        let err = inspect_peer_worktree(&root, "../peer-alpha");
        assert!(matches!(err, Err(ForgeError::Rejected(_))));
    }

    #[test]
    fn detects_overlapping_modified_files_across_peer_worktrees() {
        let (_tmp, main, peer_a) = fixture_multitree_repo();
        let peer_b = main.join(".hadron").join("trees").join("peer-beta");

        // Both peer-alpha and peer-beta edit shared.rs
        std::fs::write(peer_a.join("shared.rs"), "pub fn shared_fn() -> i32 { 1 }\n").unwrap();
        std::fs::write(peer_b.join("shared.rs"), "pub fn shared_fn() -> i32 { 2 }\n").unwrap();

        let root = Root::new(&peer_a);
        let report = detect_cross_worktree_conflicts(&root).expect("conflict detection should run");

        assert_eq!(report.conflicts_detected, 1);
        assert_eq!(report.conflicts[0].file_path, "shared.rs");
        assert_eq!(report.conflicts[0].conflicting_peers.len(), 2);
        assert!(report.conflicts[0].conflicting_peers.contains(&"peer-alpha".to_string()));
        assert!(report.conflicts[0].conflicting_peers.contains(&"peer-beta".to_string()));
    }
}

