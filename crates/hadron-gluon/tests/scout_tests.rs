use std::fs;
use hadron_gluon::scout::{spawn_ephemeral_scout, ScoutInvocation};
use tempfile::tempdir;

#[tokio::test]
async fn test_ephemeral_scout_zero_footprint_execution() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create a mock repo directory + ignored directories (.git, target, .hadron/trees)
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src").join("main.rs"),
        "fn helper() {}\nfn main() { println!(\"hello scout\"); }\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(root.join("target").join("ignored.rs"), "fn main() {}").unwrap();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();

    let invocation = ScoutInvocation::new("Search for main function in src/");
    let result = spawn_ephemeral_scout(root, &invocation).await.unwrap();

    assert!(result.created_no_worktree);
    assert!(result.summary.contains("Search for main"));
    assert!(
        result.findings.iter().any(|f| f.starts_with("src/main.rs:2:") && f.contains("fn main()")),
        "expected concrete file:line finding in src/main.rs, got: {:?}",
        result.findings
    );
    assert!(
        !result.findings.iter().any(|f| f.starts_with("target/") || f.starts_with(".git/")),
        "scout must skip target/ and .git/, got: {:?}",
        result.findings
    );

    // Verify no worktrees or .hadron/trees directory created
    assert!(!root.join(".hadron").join("trees").exists());
}

#[tokio::test]
async fn test_ephemeral_scout_rejects_mutating_tools() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let mut bad_invocation = ScoutInvocation::new("Illegal write task");
    bad_invocation.tool_allowlist.push("write_to_file".to_string());

    let res = spawn_ephemeral_scout(root, &bad_invocation).await;
    assert!(res.is_err(), "Scout must reject mutating tools");
}
