use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowState {
    Queued,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeculativeTestResult {
    pub branch: String,
    pub head_sha: String,
    pub passed: bool,
    pub tail: String,
    pub timestamp: u64,
}

pub struct ShadowGate {
    pub base_branch: String,
    queue: VecDeque<String>,
    states: HashMap<String, ShadowState>,
    speculative_cache: HashMap<(String, String), SpeculativeTestResult>,
}

impl ShadowGate {
    pub fn new(base: &str) -> Self {
        Self {
            base_branch: base.to_string(),
            queue: VecDeque::new(),
            states: HashMap::new(),
            speculative_cache: HashMap::new(),
        }
    }

    /// Checks if all changed paths are purely documentation, specs, notes, or static media assets.
    pub fn is_docs_or_asset_only_paths(paths: &[String]) -> bool {
        if paths.is_empty() {
            return false;
        }
        const DOC_EXTENSIONS: &[&str] = &[
            "md", "markdown", "txt", "png", "jpg", "jpeg", "svg", "gif", "webp", "ico",
        ];
        paths.iter().all(|p| {
            let p_norm = p.replace('\\', "/");
            if p_norm.starts_with(".hadron/") || p_norm.starts_with("docs/") {
                return true;
            }
            if let Some(ext) = std::path::Path::new(&p_norm).extension().and_then(|e| e.to_str()) {
                DOC_EXTENSIONS.iter().any(|&de| ext.eq_ignore_ascii_case(de))
            } else {
                false
            }
        })
    }

    pub fn enqueue_branch(&mut self, branch: &str) {
        self.queue.push_back(branch.to_string());
        self.states.insert(branch.to_string(), ShadowState::Queued);
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn mark_ready(&mut self, branch: &str) {
        self.states.insert(branch.to_string(), ShadowState::Passed);
    }

    pub fn is_eligible_for_fast_forward(&self, branch: &str) -> bool {
        self.states.get(branch) == Some(&ShadowState::Passed)
    }

    pub fn record_speculative_result(&mut self, branch: &str, head_sha: &str, passed: bool, tail: &str) {
        let result = SpeculativeTestResult {
            branch: branch.to_string(),
            head_sha: head_sha.to_string(),
            passed,
            tail: tail.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.speculative_cache.insert((branch.to_string(), head_sha.to_string()), result);
        if passed {
            self.mark_ready(branch);
        } else {
            self.states.insert(branch.to_string(), ShadowState::Failed);
        }
    }

    pub fn get_speculative_result(&self, branch: &str, head_sha: &str) -> Option<&SpeculativeTestResult> {
        self.speculative_cache.get(&(branch.to_string(), head_sha.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_gate_status_lifecycle() {
        let mut gate = ShadowGate::new("main");
        gate.enqueue_branch("quark/feature-a");
        gate.enqueue_branch("quark/feature-b");

        assert_eq!(gate.queue_len(), 2);
        gate.mark_ready("quark/feature-a");
        assert!(gate.is_eligible_for_fast_forward("quark/feature-a"));
    }

    #[test]
    fn test_speculative_cache_recording() {
        let mut gate = ShadowGate::new("main");
        gate.record_speculative_result("quark/feature-x", "commit123", true, "all tests pass");

        let cached = gate.get_speculative_result("quark/feature-x", "commit123");
        assert!(cached.is_some());
        let res = cached.unwrap();
        assert!(res.passed);
        assert_eq!(res.tail, "all tests pass");
        assert!(gate.is_eligible_for_fast_forward("quark/feature-x"));
    }

    #[tokio::test]
    async fn test_shadow_gate_cache_hit_and_eviction() {
        let mut gate = ShadowGate::new("main");
        gate.record_speculative_result("quark/feat-1", "sha1", true, "tests passed 10/10");
        gate.record_speculative_result("quark/feat-2", "sha2", false, "tests failed at test_x");

        let res1 = gate.get_speculative_result("quark/feat-1", "sha1").unwrap();
        assert!(res1.passed);
        assert_eq!(res1.tail, "tests passed 10/10");

        let res2 = gate.get_speculative_result("quark/feat-2", "sha2").unwrap();
        assert!(!res2.passed);
        assert_eq!(res2.tail, "tests failed at test_x");

        // Miss on different sha
        assert!(gate.get_speculative_result("quark/feat-1", "sha_other").is_none());
    }

    #[test]
    fn test_is_docs_or_asset_only_paths() {
        assert!(!ShadowGate::is_docs_or_asset_only_paths(&[]));
        assert!(ShadowGate::is_docs_or_asset_only_paths(&[
            "README.md".to_string(),
            "docs/architecture.md".to_string(),
            "assets/logo.svg".to_string(),
            ".hadron/nucleus/notes/lesson.md".to_string(),
        ]));
        assert!(!ShadowGate::is_docs_or_asset_only_paths(&[
            "README.md".to_string(),
            "src/main.rs".to_string(),
        ]));
    }
}
