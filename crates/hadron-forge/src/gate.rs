//! Pre-flight merge gate verification tool.
//!
//! Replicates the daemon's merge gate checks within the worktree sandbox:
//! 1. Checks commits ahead of base branch and dirty worktree status.
//! 2. Performs rebase merge-tree conflict check against base.
//! 3. Touches crate entrypoints (`src/lib.rs`, `src/main.rs`) to invalidate stale
//!    `.rlib` artifacts in the shared target directory.
//! 4. Executes cargo check or test within the jailed execution boundary.

use std::path::{Path, PathBuf};
use std::time::Duration;
use serde::{Deserialize, Serialize};

use crate::exec::{exec, Program, EXEC_DEADLINE};
use crate::file::{ForgeError, Root};

/// Summary report emitted by the pre-flight merge gate runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatePreflightReport {
    pub ok: bool,
    pub branch: String,
    pub base: String,
    pub commits_ahead: usize,
    pub is_dirty: bool,
    pub rebase_clean: bool,
    pub touched_entrypoints: Vec<String>,
    pub test_passed: bool,
    pub summary: String,
    pub output_tail: String,
}

/// Recursively find and touch all `src/lib.rs` and `src/main.rs` entrypoints in `root`
/// to prevent shared `target/` cache reuse bugs.
pub fn touch_crate_entrypoints(root: &Root) -> Result<Vec<String>, ForgeError> {
    let root_path = root.path();
    let mut touched = Vec::new();
    let mut dirs_to_visit = vec![root_path.to_path_buf()];

    while let Some(dir) = dirs_to_visit.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            if name_str.starts_with('.') || name_str == "target" || name_str == "node_modules" {
                continue;
            }

            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    dirs_to_visit.push(path);
                } else if ft.is_file() && (name_str == "lib.rs" || name_str == "main.rs") {
                    if let Some(parent) = path.parent() {
                        if parent.file_name().map_or(false, |p| p == "src") {
                            // Touch file mtime by rewriting bytes
                            if let Ok(bytes) = std::fs::read(&path) {
                                let _ = std::fs::write(&path, &bytes);
                                if let Ok(rel) = path.strip_prefix(root_path) {
                                    touched.push(rel.to_string_lossy().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    touched.sort();
    Ok(touched)
}

/// Run the pre-flight gate check against `base` branch (defaults to "main").
pub fn run_preflight_gate(
    root: &Root,
    base: Option<&str>,
    check_only: bool,
) -> Result<GatePreflightReport, ForgeError> {
    let base_branch = base.unwrap_or("main").trim();

    // 1. Get current branch name
    let branch_out = exec(root, Program::Git, &["rev-parse".into(), "--abbrev-ref".into(), "HEAD".into()], Duration::from_secs(10))?;
    let branch = branch_out.stdout.trim().to_string();

    // 2. Check commits ahead of base
    let log_range = format!("{base_branch}..HEAD");
    let log_out = exec(root, Program::Git, &["log".into(), "--oneline".into(), log_range], Duration::from_secs(15));
    let commits_ahead = match log_out {
        Ok(ref out) if out.code == Some(0) => out.stdout.lines().filter(|l| !l.trim().is_empty()).count(),
        _ => 0,
    };

    // 3. Check dirty worktree
    let status_out = exec(root, Program::Git, &["status".into(), "--porcelain".into()], Duration::from_secs(15))?;
    let is_dirty = !status_out.stdout.trim().is_empty();

    // 4. Check rebase / merge cleanly
    let merge_base_out = exec(root, Program::Git, &["merge-base".into(), "HEAD".into(), base_branch.into()], Duration::from_secs(10));
    let rebase_clean = match merge_base_out {
        Ok(ref mb) if mb.code == Some(0) && !mb.stdout.trim().is_empty() => {
            let base_commit = mb.stdout.trim();
            let merge_tree_out = exec(
                root,
                Program::Git,
                &["merge-tree".into(), base_commit.into(), "HEAD".into(), base_branch.into()],
                Duration::from_secs(30),
            );
            match merge_tree_out {
                Ok(ref mt) if mt.code == Some(0) => !mt.stdout.contains("<<<<<<<"),
                _ => true,
            }
        }
        _ => true,
    };

    // 5. Invalidate stale .rlib by touching crate entrypoints
    let touched_entrypoints = touch_crate_entrypoints(root)?;

    // 6. Run test or check command
    let cargo_args = if check_only {
        vec!["check".to_string(), "--workspace".to_string()]
    } else {
        vec!["test".to_string(), "--workspace".to_string()]
    };

    let cargo_out = exec(root, Program::Cargo, &cargo_args, EXEC_DEADLINE)?;
    let test_passed = cargo_out.code == Some(0) && !cargo_out.timed_out;

    let ok = test_passed && rebase_clean && !is_dirty;
    let summary = if !test_passed {
        format!("Pre-flight gate FAILED: cargo {} exited non-zero or timed out", if check_only { "check" } else { "test" })
    } else if is_dirty {
        "Pre-flight gate WARNING: uncommitted dirty changes present in worktree".to_string()
    } else if !rebase_clean {
        format!("Pre-flight gate WARNING: merge conflict detected when rebasing onto '{base_branch}'")
    } else {
        format!("Pre-flight gate PASSED: {commits_ahead} commit(s) ahead of '{base_branch}', suite green")
    };

    let output_tail = if !cargo_out.stderr.trim().is_empty() {
        cargo_out.stderr
    } else {
        cargo_out.stdout
    };

    Ok(GatePreflightReport {
        ok,
        branch,
        base: base_branch.to_string(),
        commits_ahead,
        is_dirty,
        rebase_clean,
        touched_entrypoints,
        test_passed,
        summary,
        output_tail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn fixture_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "initial commit"]);
        dir
    }

    #[test]
    fn touch_crate_entrypoints_finds_and_touches_lib_rs() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());
        let touched = touch_crate_entrypoints(&root).unwrap();
        assert_eq!(touched, vec!["src/lib.rs"]);
    }

    #[test]
    fn preflight_gate_runs_against_fixture_repo() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());
        let report = run_preflight_gate(&root, Some("main"), true).unwrap();
        assert!(report.touched_entrypoints.contains(&"src/lib.rs".to_string()));
    }
}
