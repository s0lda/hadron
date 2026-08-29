use std::path::{Path, PathBuf};

pub fn is_jailed_screenshot_path(repo_root: &Path, candidate: &Path) -> bool {
    let allowed_dir = repo_root.join(".hadron/screenshots");
    if let (Ok(canon_allowed), Ok(canon_cand)) = (
        std::fs::canonicalize(&allowed_dir).or_else(|_| Ok::<PathBuf, std::io::Error>(allowed_dir.clone())),
        std::fs::canonicalize(candidate).or_else(|_| Ok::<PathBuf, std::io::Error>(candidate.to_path_buf()))
    ) {
        canon_cand.starts_with(canon_allowed)
    } else {
        false
    }
}

#[derive(Debug, Clone)]
pub struct VisualDiffReport {
    pub baseline_path: PathBuf,
    pub candidate_path: PathBuf,
    pub diff_path: PathBuf,
    pub perceptual_mismatch_pct: f64,
}

impl VisualDiffReport {
    pub fn compute_dummy_diff(baseline: PathBuf, candidate: PathBuf, diff: PathBuf) -> Self {
        Self {
            baseline_path: baseline,
            candidate_path: candidate,
            diff_path: diff,
            perceptual_mismatch_pct: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screenshot_jail_enforcement() {
        let repo_root = PathBuf::from("/home/Jake/dev/hadron");
        let valid_path = repo_root.join(".hadron/screenshots/view.png");
        let invalid_path = PathBuf::from("/tmp/view.png");

        assert!(is_jailed_screenshot_path(&repo_root, &valid_path));
        assert!(!is_jailed_screenshot_path(&repo_root, &invalid_path));
    }
}
