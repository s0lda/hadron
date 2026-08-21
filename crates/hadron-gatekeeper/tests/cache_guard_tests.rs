use std::fs;
use std::path::PathBuf;
use std::process::Command;
use hadron_gatekeeper::cache_guard::{
    compute_cache_salt, inject_cache_isolation_flags, verify_shared_rlib_integrity,
};
use tempfile::tempdir;

#[test]
fn test_metadata_salting_and_isolation_flags() {
    let tree_1 = PathBuf::from("/home/dev/.hadron/trees/wt1");
    let tree_2 = PathBuf::from("/home/dev/.hadron/trees/wt2");

    let salt_1 = compute_cache_salt("hadron-gluon", &tree_1);
    let salt_2 = compute_cache_salt("hadron-gluon", &tree_2);

    assert_ne!(salt_1, salt_2, "Different worktrees must produce distinct salts");

    let mut cmd = Command::new("cargo");
    inject_cache_isolation_flags(&mut cmd, "hadron-gluon", &tree_1);
    // Command env contains RUSTFLAGS with metadata salt
}

#[test]
fn test_verify_shared_rlib_integrity() {
    let dir = tempdir().unwrap();
    let target_dir = dir.path();
    let deps_dir = target_dir.join("debug").join("deps");
    fs::create_dir_all(&deps_dir).unwrap();

    // Write a valid dummy rlib
    let valid_rlib = deps_dir.join("libgood.rlib");
    fs::write(&valid_rlib, b"!<arch>\nvalid payload").unwrap();

    // Write a corrupted rlib
    let corrupt_rlib = deps_dir.join("libbad.rlib");
    fs::write(&corrupt_rlib, b"corrupted header bytes").unwrap();

    let report = verify_shared_rlib_integrity(target_dir).unwrap();
    assert_eq!(report.total_artifacts, 2);
    assert_eq!(report.corrupted_artifacts, vec![corrupt_rlib]);
    assert!(!report.is_valid);
}
