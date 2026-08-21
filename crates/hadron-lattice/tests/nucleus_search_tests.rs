use hadron_lattice::nucleus_search::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_vectorized_nucleus_semantic_search() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let notes = root.join(".hadron").join("nucleus").join("notes");
    fs::create_dir_all(&notes).unwrap();

    fs::write(
        notes.join("the-gate-rebases-before-it-tests.md"),
        "---\nname: the-gate-rebases-before-it-tests\ndescription: The merge gate always runs sync rebase onto base branch before executing tests\n---\nRule 5 requires knowing baseline. Merge gate syncs base and verifies cleanly.",
    ).unwrap();

    fs::write(
        notes.join("vulkan-lavapipe-fallback.md"),
        "---\nname: vulkan-lavapipe-fallback\ndescription: Software rendering fallback for GPUI on WSL and Linux\n---\nAvoid redundant layout recalculations under software CPU rasterization.",
    ).unwrap();

    let results = query_nucleus_semantic(root, "rebase merge gate tests", 5).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].slug, "the-gate-rebases-before-it-tests");
    assert!(results[0].score > 0.0);
    assert!(results[0].description.contains("merge gate"));
}
