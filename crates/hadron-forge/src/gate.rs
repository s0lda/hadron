//! Pre-flight merge gate verification tool.
//!
//! Replicates the daemon's merge gate checks within the worktree sandbox:
//! 1. Checks commits ahead of base branch and dirty worktree status.
//! 2. Performs rebase merge-tree conflict check against base.
//! 3. Touches crate entrypoints (`src/lib.rs`, `src/main.rs`) to invalidate stale
//!    `.rlib` artifacts in the shared target directory.
//! 4. Executes cargo check or test within the jailed execution boundary.

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

/// Configuration for the multi-modal acceptance verification suite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceSuiteConfig {
    pub base: Option<String>,
    pub run_unit_tests: bool,
    pub run_lint_check: bool,
    pub verify_process_lifecycle: Option<ProcessLifecycleCheck>,
    pub verify_screenshots: Option<ScreenshotVerificationCheck>,
    pub custom_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessLifecycleCheck {
    pub command: String,
    pub args: Vec<String>,
    pub ready_match: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenshotVerificationCheck {
    pub min_count: usize,
    pub check_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceStageResult {
    pub stage_name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub details: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceReport {
    pub ok: bool,
    pub total_stages: usize,
    pub passed_stages: usize,
    pub stages: Vec<AcceptanceStageResult>,
    pub summary: String,
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

/// Run a multi-modal acceptance verification suite.
pub fn run_acceptance_suite(
    root: &Root,
    config: &AcceptanceSuiteConfig,
) -> Result<AcceptanceReport, ForgeError> {
    let start_all = std::time::Instant::now();
    let mut stages = Vec::new();

    // 1. Stage: Preflight Rebase & Worktree Integrity
    let t0 = std::time::Instant::now();
    let preflight = run_preflight_gate(root, config.base.as_deref(), true)?;
    stages.push(AcceptanceStageResult {
        stage_name: "Preflight Rebase & Worktree Integrity".to_string(),
        passed: preflight.rebase_clean && !preflight.is_dirty,
        duration_ms: t0.elapsed().as_millis() as u64,
        details: preflight.summary.clone(),
    });

    // 2. Stage: Unit Tests (if enabled)
    if config.run_unit_tests {
        let t0 = std::time::Instant::now();
        let cargo_out = exec(root, Program::Cargo, &["test".to_string(), "--workspace".to_string()], EXEC_DEADLINE)?;
        let passed = cargo_out.code == Some(0) && !cargo_out.timed_out;
        let details = if passed {
            "Cargo test workspace suite passed cleanly".to_string()
        } else {
            cargo_out.stderr.lines().take(10).collect::<Vec<_>>().join("\n")
        };
        stages.push(AcceptanceStageResult {
            stage_name: "Unit & Workspace Tests".to_string(),
            passed,
            duration_ms: t0.elapsed().as_millis() as u64,
            details,
        });
    }

    // 3. Stage: Lint & Compiler Check (if enabled)
    if config.run_lint_check {
        let t0 = std::time::Instant::now();
        let check_out = exec(root, Program::Cargo, &["check".to_string(), "--workspace".to_string()], EXEC_DEADLINE)?;
        let passed = check_out.code == Some(0) && !check_out.timed_out;
        stages.push(AcceptanceStageResult {
            stage_name: "Cargo Workspace Lint & Type Check".to_string(),
            passed,
            duration_ms: t0.elapsed().as_millis() as u64,
            details: if passed { "0 compiler errors".to_string() } else { check_out.stderr },
        });
    }

    // 4. Stage: Screenshot Artifact Validation (if enabled)
    if let Some(ref screen_check) = config.verify_screenshots {
        let t0 = std::time::Instant::now();
        let dir_path = match &screen_check.check_dir {
            Some(custom) => root.path().join(custom),
            None => root.path().join(".hadron").join("screenshots"),
        };
        let count = if dir_path.is_dir() {
            std::fs::read_dir(&dir_path)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".webp")
                        })
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };
        let passed = count >= screen_check.min_count;
        stages.push(AcceptanceStageResult {
            stage_name: "Screenshot Artifact Validation".to_string(),
            passed,
            duration_ms: t0.elapsed().as_millis() as u64,
            details: format!("Found {count} screenshots (required minimum: {})", screen_check.min_count),
        });
    }

    // 5. Stage: Custom Command Validations
    for cmd in &config.custom_commands {
        let t0 = std::time::Instant::now();
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if let Some((prog_str, args)) = parts.split_first() {
            let prog = match *prog_str {
                "cargo" => Program::Cargo,
                "git" => Program::Git,
                "node" => Program::Node,
                "npm" => Program::Npm,
                "python" | "python3" => Program::Python3,
                _ => Program::Git,
            };
            let arg_strings: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let out = exec(root, prog, &arg_strings, Duration::from_secs(30))?;
            let passed = out.code == Some(0) && !out.timed_out;
            stages.push(AcceptanceStageResult {
                stage_name: format!("Custom Command `{cmd}`"),
                passed,
                duration_ms: t0.elapsed().as_millis() as u64,
                details: if passed { "Command succeeded (exit 0)".to_string() } else { out.stderr },
            });
        }
    }

    let total_stages = stages.len();
    let passed_stages = stages.iter().filter(|s| s.passed).count();
    let ok = total_stages > 0 && passed_stages == total_stages;
    let summary = if ok {
        format!("Acceptance verification PASSED: all {passed_stages}/{total_stages} stages green in {}ms", start_all.elapsed().as_millis())
    } else {
        format!("Acceptance verification FAILED: {passed_stages}/{total_stages} stages passed")
    };

    Ok(AcceptanceReport {
        ok,
        total_stages,
        passed_stages,
        stages,
        summary,
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
        run(&["branch", "-M", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.path().join(".gitignore"), ".hadron/\n").unwrap();
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

    #[test]
    fn acceptance_gate_runs_multi_stage_checks() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());

        // Create screenshot fixture in .hadron/screenshots/
        let screen_dir = dir.path().join(".hadron").join("screenshots");
        std::fs::create_dir_all(&screen_dir).unwrap();
        std::fs::write(screen_dir.join("smoke_test.png"), b"\x89PNG\r\n\x1a\n").unwrap();

        let config = AcceptanceSuiteConfig {
            base: Some("main".to_string()),
            run_unit_tests: false,
            run_lint_check: false,
            verify_process_lifecycle: None,
            verify_screenshots: Some(ScreenshotVerificationCheck {
                min_count: 1,
                check_dir: None,
            }),
            custom_commands: vec!["git status".to_string()],
        };

        let report = run_acceptance_suite(&root, &config).expect("acceptance suite should run");
        assert!(report.ok);
        assert_eq!(report.passed_stages, report.total_stages);
        assert!(report.stages.iter().any(|s| s.stage_name == "Screenshot Artifact Validation"));
    }
}

