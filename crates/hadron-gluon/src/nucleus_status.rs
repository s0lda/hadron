use std::path::Path;

/// SSOT for the nucleus index budget, shared with `engine::nucleus::NUCLEUS_INDEX_BUDGET`
/// (which re-exports this constant rather than declaring its own — see that module for
/// why the budget exists). Public so the chamber can check it without reaching into
/// the engine's internals.
pub const BUDGET_BYTES: usize = 32 * 1024;

/// Whether `.hadron/nucleus/index.md` under `workspace_root` currently exceeds the
/// budget the prompt builder enforces. A missing file is not over budget — it is
/// the normal first-run case.
pub fn index_over_budget(workspace_root: &Path) -> bool {
    let path = workspace_root.join(".hadron").join("nucleus").join("index.md");
    std::fs::metadata(&path).map(|m| m.len() as usize > BUDGET_BYTES).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_over_budget_is_false_for_a_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();
        std::fs::write(nucleus.join("index.md"), "- **x** — short\n").unwrap();
        assert!(!index_over_budget(dir.path()));
    }

    #[test]
    fn index_over_budget_is_true_past_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();
        let big = "- **x** — ".to_string() + &"a".repeat(BUDGET_BYTES + 1);
        std::fs::write(nucleus.join("index.md"), big).unwrap();
        assert!(index_over_budget(dir.path()));
    }

    #[test]
    fn index_over_budget_is_false_when_the_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!index_over_budget(dir.path()));
    }
}
