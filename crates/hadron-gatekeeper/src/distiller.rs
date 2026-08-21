use hadron_lattice::nucleus_distill::{distill_failure_recovery, write_distilled_lesson, CandidateLesson};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct FailureDistillationGatekeeper;

impl FailureDistillationGatekeeper {
    pub fn distill_and_persist(
        repo_root: &Path,
        error_tail: &str,
        fix_diff: &str,
    ) -> Option<(CandidateLesson, PathBuf)> {
        let lesson = distill_failure_recovery(error_tail, fix_diff)?;
        let path = write_distilled_lesson(repo_root, &lesson).ok()?;
        Some((lesson, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_gatekeeper_distiller_integration() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let error = "error: mismatched types expected `usize`, found `u32`";
        let diff = "-let len: u32 = 0;\n+let len: usize = 0;";

        let result = FailureDistillationGatekeeper::distill_and_persist(root, error, diff);
        assert!(result.is_some());
        let (lesson, path) = result.unwrap();
        assert_eq!(lesson.slug, "type-contract-agreement");
        assert!(path.exists());
    }
}
