use std::collections::HashMap;

#[allow(dead_code)]
pub struct DiffSteeringState {
    hunk_staged: HashMap<usize, bool>,
    hunk_rejected: HashMap<usize, bool>,
    hunk_comments: HashMap<usize, String>,
}

#[allow(dead_code)]
impl DiffSteeringState {
    pub fn new() -> Self {
        Self {
            hunk_staged: HashMap::new(),
            hunk_rejected: HashMap::new(),
            hunk_comments: HashMap::new(),
        }
    }

    pub fn register_hunk(&mut self, hunk_idx: usize, staged: bool) {
        self.hunk_staged.insert(hunk_idx, staged);
        if staged {
            self.hunk_rejected.insert(hunk_idx, false);
        }
    }

    pub fn stage_hunk(&mut self, hunk_idx: usize) {
        self.hunk_staged.insert(hunk_idx, true);
        self.hunk_rejected.insert(hunk_idx, false);
    }

    pub fn unstage_hunk(&mut self, hunk_idx: usize) {
        self.hunk_staged.insert(hunk_idx, false);
    }

    pub fn toggle_hunk(&mut self, hunk_idx: usize) -> bool {
        let current = self.hunk_staged.get(&hunk_idx).copied().unwrap_or(false);
        let next = !current;
        self.register_hunk(hunk_idx, next);
        next
    }

    pub fn reject_hunk(&mut self, hunk_idx: usize) {
        self.hunk_staged.insert(hunk_idx, false);
        self.hunk_rejected.insert(hunk_idx, true);
    }

    pub fn staged_hunks(&self) -> Vec<usize> {
        let mut staged: Vec<usize> = self
            .hunk_staged
            .iter()
            .filter(|(_, &s)| s)
            .map(|(&idx, _)| idx)
            .collect();
        staged.sort();
        staged
    }

    pub fn rejected_hunks(&self) -> Vec<usize> {
        let mut rejected: Vec<usize> = self
            .hunk_rejected
            .iter()
            .filter(|(_, &r)| r)
            .map(|(&idx, _)| idx)
            .collect();
        rejected.sort();
        rejected
    }

    pub fn set_hunk_comment(&mut self, hunk_idx: usize, comment: &str) {
        self.hunk_comments.insert(hunk_idx, comment.to_string());
    }

    pub fn get_hunk_comment(&self, hunk_idx: usize) -> Option<&String> {
        self.hunk_comments.get(&hunk_idx)
    }

    pub fn draft_commit_message(&self, title: &str) -> String {
        let staged = self.staged_hunks();
        let rejected = self.rejected_hunks();
        format!(
            "{}\n\n[Staged hunks: {:?}, Rejected hunks: {:?}]",
            title, staged, rejected
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_hunk_selection_and_steering() {
        let mut steering = DiffSteeringState::new();
        steering.register_hunk(0, true);
        steering.register_hunk(1, false);

        assert_eq!(steering.staged_hunks(), vec![0]);
        steering.set_hunk_comment(1, "Please simplify this match expression.");

        let comment = steering.get_hunk_comment(1).unwrap();
        assert!(comment.contains("simplify this match"));

        // Toggle hunk 1
        assert!(steering.toggle_hunk(1));
        assert_eq!(steering.staged_hunks(), vec![0, 1]);

        // Reject hunk 0
        steering.reject_hunk(0);
        assert_eq!(steering.staged_hunks(), vec![1]);
        assert_eq!(steering.rejected_hunks(), vec![0]);

        let msg = steering.draft_commit_message("feat(auth): add token");
        assert!(msg.contains("Staged hunks: [1]"));
        assert!(msg.contains("Rejected hunks: [0]"));
    }
}
