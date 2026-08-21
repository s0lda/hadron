use std::fs;
use std::process::Command;
use hadron_gluon::worktree::{stream_rebase_to_active_worktrees, RebaseOutcome};
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
fn test_rebase_streamer_clean_fast_forward_and_conflict() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Initialize git repo with main branch
    run_cmd(root, &["init", "-b", "main"]);
    run_cmd(root, &["config", "user.email", "hadron@test.local"]);
    run_cmd(root, &["config", "user.name", "Hadron Test"]);
    run_cmd(root, &["commit", "--allow-empty", "-m", "initial commit"]);

    fs::write(root.join("shared.txt"), "line 1\nline 2\n").unwrap();
    run_cmd(root, &["add", "shared.txt"]);
    run_cmd(root, &["commit", "-m", "add shared.txt"]);

    // Create worktree 1 (non-conflicting branch)
    let wt1_dir = root.join(".hadron").join("trees").join("wt1");
    fs::create_dir_all(&wt1_dir).unwrap();
    run_cmd(root, &["worktree", "add", "-b", "quark/wt1/task-1", wt1_dir.to_str().unwrap(), "main"]);
    fs::write(wt1_dir.join("file_wt1.txt"), "created by wt1\n").unwrap();
    run_cmd(&wt1_dir, &["add", "file_wt1.txt"]);
    run_cmd(&wt1_dir, &["commit", "-m", "wt1 commit"]);

    // Create worktree 2 (conflicting branch on shared.txt)
    let wt2_dir = root.join(".hadron").join("trees").join("wt2");
    fs::create_dir_all(&wt2_dir).unwrap();
    run_cmd(root, &["worktree", "add", "-b", "quark/wt2/task-2", wt2_dir.to_str().unwrap(), "main"]);
    fs::write(wt2_dir.join("shared.txt"), "conflict from wt2\n").unwrap();
    run_cmd(&wt2_dir, &["add", "shared.txt"]);
    run_cmd(&wt2_dir, &["commit", "-m", "wt2 commit"]);

    // Now land a new commit on main in repo_root
    fs::write(root.join("shared.txt"), "main update\n").unwrap();
    run_cmd(root, &["add", "shared.txt"]);
    run_cmd(root, &["commit", "-m", "main update to shared.txt"]);

    // Stream rebase to active worktrees
    let outcomes = stream_rebase_to_active_worktrees(root, &[wt1_dir.clone(), wt2_dir.clone()], "main");
    assert_eq!(outcomes.len(), 2);

    // wt1 should rebase cleanly
    match &outcomes[0] {
        RebaseOutcome::CleanFastForward { tree, new_head } => {
            assert_eq!(tree, &wt1_dir);
            assert!(!new_head.is_empty());
        }
        other => panic!("Expected CleanFastForward for wt1, got {:?}", other),
    }

    // wt2 should detect conflict and abort rebase cleanly
    match &outcomes[1] {
        RebaseOutcome::ConflictDetected { tree, conflicted_files } => {
            assert_eq!(tree, &wt2_dir);
            assert!(!conflicted_files.is_empty());
        }
        other => panic!("Expected ConflictDetected for wt2, got {:?}", other),
    }
}
