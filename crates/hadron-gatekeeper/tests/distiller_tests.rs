use hadron_gatekeeper::distiller::FailureDistillationGatekeeper;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_distiller_extracts_and_persists_post_mortem() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let error_log = "error[E0432]: unresolved import `hadron_lattice::artifacts`\n --> crates/hadron-lattice/tests/artifact_bus_tests.rs:1:19";
    let fix_diff = "+pub mod artifacts;\n+pub use artifacts::*;";

    let result = FailureDistillationGatekeeper::distill_and_persist(root, error_log, fix_diff);
    assert!(result.is_some());

    let (lesson, path) = result.unwrap();
    assert_eq!(lesson.slug, "missing-export-or-module-seam");
    assert!(path.exists());

    let index_file = root.join(".hadron").join("nucleus").join("index.md");
    assert!(index_file.exists());
    let index_content = fs::read_to_string(index_file).unwrap();
    assert!(index_content.contains("- [missing-export-or-module-seam](notes/missing-export-or-module-seam.md)"));
}
