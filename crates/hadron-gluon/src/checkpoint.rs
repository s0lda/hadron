use std::collections::BTreeMap;

pub struct CheckpointStore {
    pub turn_id: String,
    checkpoints: BTreeMap<usize, String>,
}

impl CheckpointStore {
    pub fn new(turn_id: &str) -> Self {
        Self {
            turn_id: turn_id.to_string(),
            checkpoints: BTreeMap::new(),
        }
    }

    pub fn record_checkpoint(&mut self, step: usize, sha: &str) {
        self.checkpoints.insert(step, sha.to_string());
    }

    pub fn get_rewind_target(&self, step: usize) -> Option<String> {
        self.checkpoints.get(&step).cloned()
    }

    pub fn format_ref(&self, step: usize) -> String {
        format!("refs/hadron/checkpoints/{}/step-{}", self.turn_id, step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_recording_and_rewind() {
        let mut store = CheckpointStore::new("turn-101");
        store.record_checkpoint(1, "commit_sha_step_1");
        store.record_checkpoint(2, "commit_sha_step_2");

        let target = store.get_rewind_target(1);
        assert_eq!(target, Some("commit_sha_step_1".to_string()));
    }
}
