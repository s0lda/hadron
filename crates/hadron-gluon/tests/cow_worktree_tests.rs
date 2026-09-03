use hadron_gluon::worktree::cow::CowWorkspace;
use hadron_gluon::worktree::sccache_guard::SccacheGuard;
use tempfile::tempdir;

#[test]
fn test_sccache_env_generation_and_target_isolation() {
    let dir = tempdir().expect("tempdir");
    let guard = SccacheGuard::new(dir.path());
    let env_vars = guard.build_env_for_quark("http-ollama");

    assert_eq!(env_vars.get("RUSTC_WRAPPER"), Some(&"sccache".to_string()));
    let target_dir = env_vars.get("CARGO_TARGET_DIR").expect("must set target dir");
    assert!(target_dir.contains("quarks/http-ollama"));
}

#[test]
fn test_cow_workspace_creation() {
    let dir = tempdir().expect("tempdir");
    let ws = CowWorkspace::create(dir.path(), "http-ollama").expect("create cow workspace");
    assert_eq!(ws.quark_id, "http-ollama");
    assert!(ws.path.is_dir());
}
