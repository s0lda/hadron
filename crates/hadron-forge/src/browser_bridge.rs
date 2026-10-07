use std::path::{Path, PathBuf};

pub fn is_jailed_screenshot_path(repo_root: &Path, candidate: &Path) -> bool {
    let allowed_dir = repo_root.join(".hadron/screenshots");
    let canon_allowed = match std::fs::canonicalize(&allowed_dir) {
        Ok(p) => p,
        Err(_) => allowed_dir.clone(),
    };
    let canon_cand = if candidate.exists() {
        match std::fs::canonicalize(candidate) {
            Ok(p) => p,
            Err(_) => return false,
        }
    } else if let Some(parent) = candidate.parent() {
        match std::fs::canonicalize(parent) {
            Ok(p) => {
                if let Some(file_name) = candidate.file_name() {
                    p.join(file_name)
                } else {
                    p
                }
            }
            Err(_) => candidate.to_path_buf(),
        }
    } else {
        candidate.to_path_buf()
    };
    canon_cand.starts_with(&canon_allowed)
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
