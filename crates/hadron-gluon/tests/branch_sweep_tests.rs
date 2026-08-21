use std::fs;
use std::process::Command;
use hadron_gluon::worktree::{archive_and_prune_branch, sweep_merged_branches};
use tempfile::tempdir;

fn run_cmd(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run git {:?}: {}", args, e));
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn test_sweep_and_archive_pruning() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    run_cmd(root, &["init", "-b", "main"]);
    run_cmd(root, &["config", "user.email", "hadron@test.local"]);
    run_cmd(root, &["config", "user.name", "Hadron Test"]);
    run_cmd(root, &["commit", "--allow-empty", "-m", "initial commit"]);

    // Create branch 1 (merged)
    run_cmd(root, &["branch", "quark/worker1/task-1"]);

    // Create branch 2 (unmerged abandoned)
    run_cmd(root, &["checkout", "-b", "quark/worker2/task-2"]);
    fs::write(root.join("unmerged.txt"), "abandoned work").unwrap();
    run_cmd(root, &["add", "unmerged.txt"]);
    run_cmd(root, &["commit", "-m", "abandoned commit"]);
    let abandoned_sha = run_cmd(root, &["rev-parse", "HEAD"]);

    run_cmd(root, &["checkout", "main"]);

    // 1. Test sweep_merged_branches
    let swept = sweep_merged_branches(root, "main").unwrap();
    assert_eq!(swept, vec!["quark/worker1/task-1"]);

    // 2. Test archive_and_prune_branch for abandoned branch
    let tag = archive_and_prune_branch(root, "quark/worker2/task-2").unwrap();
    assert_eq!(tag, "archive/quark-worker2-task-2");

    // Verify tag points to abandoned_sha
    let tag_sha = run_cmd(root, &["rev-parse", &tag]);
    assert_eq!(tag_sha, abandoned_sha);

    // Verify branch is deleted
    let branches = run_cmd(root, &["branch", "--list", "quark/worker2/task-2"]);
    assert!(branches.is_empty());
}
