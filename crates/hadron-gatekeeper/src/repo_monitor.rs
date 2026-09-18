use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// Health report returned by the periodic repository monitor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoHealthReport {
    pub has_cargo_lock_drift: bool,
    pub stale_worktrees: Vec<PathBuf>,
    pub nucleus_issues: usize,
    #[serde(default)]
    pub nucleus_broken_links: Vec<String>,
    #[serde(default)]
    pub nucleus_orphaned_notes: Vec<PathBuf>,
    #[serde(default)]
    pub nucleus_budget_exceeded: bool,
    #[serde(default)]
    pub nucleus_index_bytes: usize,
    #[serde(default)]
    pub nucleus_index_budget: usize,
    pub healthy: bool,
    pub timestamp: u64,
}

impl RepoHealthReport {
    /// Returns true if the repo is completely issue-free.
    pub fn is_healthy(&self) -> bool {
        !self.has_cargo_lock_drift && self.stale_worktrees.is_empty() && self.nucleus_issues == 0
    }
}

/// Periodic repository diagnostics runner (drift, stale trees, nucleus health).
pub struct RepoMonitor;

impl RepoMonitor {
    /// Checks whether `Cargo.lock` in `repo_root` has uncommitted drift.
    pub fn check_cargo_lock_drift(repo_root: &Path) -> bool {
        if let Ok(output) = std::process::Command::new("git")
            .args(["status", "--porcelain", "Cargo.lock"])
            .current_dir(repo_root)
            .output()
        {
            !output.stdout.is_empty()
        } else {
            false
        }
    }

    /// Checks for stale worktrees in `.hadron/trees/` (marked with `.stale`).
    pub fn check_stale_worktrees(repo_root: &Path) -> Vec<PathBuf> {
        let trees_dir = repo_root.join(".hadron").join("trees");
        let mut stale = Vec::new();
        if trees_dir.exists() && trees_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&trees_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let stale_marker = path.join(".stale");
                        if stale_marker.exists() {
                            stale.push(path);
                        }
                    }
                }
            }
        }
        stale.sort();
        stale
    }

    /// Audits nucleus integrity using `NucleusIntegrityLinter`.
    pub fn check_nucleus(repo_root: &Path) -> usize {
        let nucleus_dir = repo_root.join(".hadron").join("nucleus");
        if !nucleus_dir.exists() {
            return 0;
        }
        let linter = crate::nucleus_linter::NucleusIntegrityLinter::for_repo(repo_root);
        let report = linter.lint(&nucleus_dir);
        let mut issues = 0;
        if report.budget_exceeded {
            issues += 1;
        }
        issues += report.broken_links.len();
        issues += report.orphaned_notes.len();
        issues
    }

    /// Runs all diagnostic health checks on the repository root.
    pub fn check_repo(repo_root: &Path) -> RepoHealthReport {
        let has_cargo_lock_drift = Self::check_cargo_lock_drift(repo_root);
        let stale_worktrees = Self::check_stale_worktrees(repo_root);
        let nucleus_dir = repo_root.join(".hadron").join("nucleus");
        let team = hadron_lattice::load_team_for_repo(repo_root);
        let budget = team.nucleus_index_budget_bytes();
        let (
            nucleus_issues,
            nucleus_broken_links,
            nucleus_orphaned_notes,
            nucleus_budget_exceeded,
            nucleus_index_bytes,
            nucleus_index_budget,
        ) = if nucleus_dir.exists() {
            let linter = crate::nucleus_linter::NucleusIntegrityLinter::new(budget);
            let report = linter.lint(&nucleus_dir);
            let mut issues = 0;
            if report.budget_exceeded {
                issues += 1;
            }
            issues += report.broken_links.len();
            issues += report.orphaned_notes.len();
            (
                issues,
                report.broken_links,
                report.orphaned_notes,
                report.budget_exceeded,
                report.index_bytes,
                report.index_budget,
            )
        } else {
            (
                0,
                Vec::new(),
                Vec::new(),
                false,
                0,
                budget,
            )
        };
        let healthy = !has_cargo_lock_drift && stale_worktrees.is_empty() && nucleus_issues == 0;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        RepoHealthReport {
            has_cargo_lock_drift,
            stale_worktrees,
            nucleus_issues,
            nucleus_broken_links,
            nucleus_orphaned_notes,
            nucleus_budget_exceeded,
            nucleus_index_bytes,
            nucleus_index_budget,
            healthy,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_repo_monitor_clean_repo() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let report = RepoMonitor::check_repo(root);
        assert!(report.is_healthy());
        assert_eq!(report.stale_worktrees.len(), 0);
        assert_eq!(report.nucleus_issues, 0);
    }

    #[test]
    fn test_repo_monitor_detects_stale_worktrees() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let stale_tree = root.join(".hadron").join("trees").join("stale-quark");
        std::fs::create_dir_all(&stale_tree).unwrap();
        std::fs::write(stale_tree.join(".stale"), "stale").unwrap();

        let stale = RepoMonitor::check_stale_worktrees(root);
        assert_eq!(stale, vec![stale_tree]);

        let report = RepoMonitor::check_repo(root);
        assert!(!report.is_healthy());
        assert_eq!(report.stale_worktrees.len(), 1);
    }

    #[test]
    fn test_repo_monitor_detects_nucleus_issues() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let nucleus = root.join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();

        // broken link in index.md
        std::fs::write(
            nucleus.join("index.md"),
            "- [broken](notes/broken.md) — hook\n",
        )
        .unwrap();

        let issues = RepoMonitor::check_nucleus(root);
        assert!(issues > 0);

        let report = RepoMonitor::check_repo(root);
        assert!(!report.is_healthy());
        assert!(report.nucleus_issues > 0);
        assert_eq!(report.nucleus_broken_links, vec!["notes/broken.md".to_string()]);
        assert!(report.nucleus_index_bytes > 0);
    }

    #[test]
    fn test_repo_monitor_respects_custom_team_budget() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let hadron_dir = root.join(".hadron");
        let nucleus = hadron_dir.join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();

        // Write team.json with 64 KiB budget
        let team_json = r#"{"quarks":[],"nucleus_index_budget_kb":64}"#;
        std::fs::write(hadron_dir.join("team.json"), team_json).unwrap();

        // Write 40 KB index.md (exceeds default 32 KB, but under 64 KB)
        let big_index = "A".repeat(40 * 1024);
        std::fs::write(nucleus.join("index.md"), big_index).unwrap();

        let report = RepoMonitor::check_repo(root);
        assert_eq!(report.nucleus_index_budget, 64 * 1024);
        assert!(!report.nucleus_budget_exceeded);
        assert_eq!(report.nucleus_issues, 0);
        assert!(report.is_healthy());
    }
}
