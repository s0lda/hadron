use hadron_gluon::nucleus_store::NucleusStore;
use tempfile::tempdir;

#[tokio::test]
async fn test_nucleus_central_store_note_and_index_lifecycle() {
    let dir = tempdir().expect("tempdir");
    let nucleus_dir = dir.path().join(".hadron/nucleus");
    std::fs::create_dir_all(nucleus_dir.join("notes")).expect("create notes dir");

    let store = NucleusStore::new(&nucleus_dir);

    // Write note
    store.write_note(
        "test-lesson",
        "A lesson learned in a worktree",
        "Testing non-obvious invariant",
        "How to apply in practice",
    ).await.expect("write note");

    assert!(nucleus_dir.join("notes/test-lesson.md").is_file());
    assert!(nucleus_dir.join("index.md").is_file());

    let index_content = std::fs::read_to_string(nucleus_dir.join("index.md")).expect("read index");
    assert!(index_content.contains("- [test-lesson](notes/test-lesson.md)"));

    let loaded = store.read_note("test-lesson").await.expect("read note");
    assert!(loaded.contains("A lesson learned in a worktree"));
}
