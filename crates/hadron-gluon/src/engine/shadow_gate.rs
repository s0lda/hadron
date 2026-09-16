use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowState {
    Queued,
    Compiling,
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

    pub async fn run_speculative_test(
        wt: &crate::worktree::Worktree,
        runner: &Arc<dyn crate::merge::MergeRunner>,
        gate: &Arc<AsyncMutex<ShadowGate>>,
    ) -> anyhow::Result<SpeculativeTestResult> {
        let head_sha = crate::worktree::head(&wt.path).unwrap_or_default();
        {
            let mut g = gate.lock().await;
            g.states.insert(wt.branch.clone(), ShadowState::Compiling);
        }
        let (passed, tail) = runner.tests(wt).await?;
        let mut g = gate.lock().await;
        g.record_speculative_result(&wt.branch, &head_sha, passed, &tail);
        Ok(SpeculativeTestResult {
            branch: wt.branch.clone(),
            head_sha,
            passed,
            tail,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::Path;
    use crate::merge::Landed;

    struct MockRunner {
        pass: bool,
    }

    #[async_trait]
    impl crate::merge::MergeRunner for MockRunner {
        async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
            Ok((self.pass, "mock test output".to_string()))
        }
        fn land(&self, _repo_root: &Path, _wt: &crate::worktree::Worktree, _base: &str) -> anyhow::Result<Landed> {
            Ok(Landed::FastForward)
        }
    }

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
    async fn test_run_speculative_test_async() {
        let gate = Arc::new(AsyncMutex::new(ShadowGate::new("main")));
        let runner: Arc<dyn crate::merge::MergeRunner> = Arc::new(MockRunner { pass: true });
        let wt = crate::worktree::Worktree {
            quark: hadron_lattice::QuarkId::new("mock"),
            path: std::env::current_dir().unwrap(),
            branch: "quark/feature-spec".to_string(),
        };

        let result = ShadowGate::run_speculative_test(&wt, &runner, &gate).await.unwrap();
        assert!(result.passed);

        let g = gate.lock().await;
        assert!(g.is_eligible_for_fast_forward("quark/feature-spec"));
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
}
