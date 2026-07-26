use std::path::Path;

use hadron_lattice::Team;

/// SSOT for the DEFAULT nucleus index budget, shared with
/// `engine::nucleus::NUCLEUS_INDEX_BUDGET` (which re-exports this constant rather
/// than declaring its own — see that module for why the budget exists). Public so
/// the chamber can check it without reaching into the engine's internals.
///
/// A re-export, not a declaration: the true SSOT is
/// `hadron_lattice::DEFAULT_NUCLEUS_INDEX_BUDGET_BYTES`, because `Projection` (in
/// `hadron-lattice`) needs the same default for its own serde fallback and
/// `hadron-lattice` cannot depend back on this crate.
pub use hadron_lattice::DEFAULT_NUCLEUS_INDEX_BUDGET_BYTES as BUDGET_BYTES;

/// Resolve this project's nucleus index budget, in bytes, from repo policy
/// (`Team::nucleus_index_budget_kb`). Absent or `0` falls back to [`BUDGET_BYTES`] —
/// `0` is treated as "not set" rather than "no budget at all", the same tolerance
/// `max_exchanges` gives a hand-edited `0`.
pub fn resolve_budget_bytes(team: &Team) -> usize {
    team.nucleus_index_budget_kb
        .filter(|&kb| kb > 0)
        .map(|kb| kb * 1024)
        .unwrap_or(BUDGET_BYTES)
}

/// Whether `.hadron/nucleus/index.md` under `workspace_root` currently exceeds
/// `budget_bytes` — the RESOLVED budget (see [`resolve_budget_bytes`]), not always
/// [`BUDGET_BYTES`]: a caller that hardcoded the default here would drift from a
/// repo that configured a different one. A missing file is not over budget — it is
/// the normal first-run case.
pub fn index_over_budget(workspace_root: &Path, budget_bytes: usize) -> bool {
    let path = workspace_root.join(".hadron").join("nucleus").join("index.md");
    std::fs::metadata(&path).map(|m| m.len() as usize > budget_bytes).unwrap_or(false)
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
        assert!(!index_over_budget(dir.path(), BUDGET_BYTES));
    }

    #[test]
    fn index_over_budget_is_true_past_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();
        let big = "- **x** — ".to_string() + &"a".repeat(BUDGET_BYTES + 1);
        std::fs::write(nucleus.join("index.md"), big).unwrap();
        assert!(index_over_budget(dir.path(), BUDGET_BYTES));
    }

    #[test]
    fn index_over_budget_is_false_when_the_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!index_over_budget(dir.path(), BUDGET_BYTES));
    }

    /// A file under the default (32 KiB) but over a smaller CONFIGURED budget must
    /// read as over budget — the whole point of threading the resolved value through
    /// rather than hardcoding `BUDGET_BYTES` at the call site.
    #[test]
    fn index_over_budget_respects_a_smaller_configured_budget() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();
        std::fs::write(nucleus.join("index.md"), "a".repeat(2000)).unwrap();
        assert!(!index_over_budget(dir.path(), BUDGET_BYTES));
        assert!(index_over_budget(dir.path(), 1024));
    }

    #[test]
    fn resolve_budget_bytes_falls_back_to_the_default_when_absent() {
        let team = Team::default();
        assert_eq!(resolve_budget_bytes(&team), BUDGET_BYTES);
    }

    #[test]
    fn resolve_budget_bytes_falls_back_to_the_default_when_zero() {
        // A hand-edited `0` is treated as "not set", the same tolerance
        // `max_exchanges` gives a `0` — a real `0` budget would make the index
        // permanently over budget the instant it holds a single byte.
        let team = Team { nucleus_index_budget_kb: Some(0), ..Team::default() };
        assert_eq!(resolve_budget_bytes(&team), BUDGET_BYTES);
    }

    #[test]
    fn resolve_budget_bytes_honours_a_configured_value() {
        let team = Team { nucleus_index_budget_kb: Some(64), ..Team::default() };
        assert_eq!(resolve_budget_bytes(&team), 64 * 1024);
    }
}
