use std::collections::HashMap;

pub struct DiffSteeringState {
    hunk_staged: HashMap<usize, bool>,
    hunk_comments: HashMap<usize, String>,
}

impl DiffSteeringState {
    pub fn new() -> Self {
        Self {
            hunk_staged: HashMap::new(),
            hunk_comments: HashMap::new(),
        }
    }

    pub fn register_hunk(&mut self, hunk_idx: usize, staged: bool) {
        self.hunk_staged.insert(hunk_idx, staged);
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

    pub fn set_hunk_comment(&mut self, hunk_idx: usize, comment: &str) {
        self.hunk_comments.insert(hunk_idx, comment.to_string());
    }

    pub fn get_hunk_comment(&self, hunk_idx: usize) -> Option<&String> {
        self.hunk_comments.get(&hunk_idx)
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
    }
}
