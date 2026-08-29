use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowState {
    Queued,
    Compiling,
    Passed,
    Failed,
}

pub struct ShadowGate {
    pub base_branch: String,
    queue: VecDeque<String>,
    states: HashMap<String, ShadowState>,
}

impl ShadowGate {
    pub fn new(base: &str) -> Self {
        Self {
            base_branch: base.to_string(),
            queue: VecDeque::new(),
            states: HashMap::new(),
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
}
