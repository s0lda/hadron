use serde::{Deserialize, Serialize};

/// A pointer to one worktree snapshot, stored in the project repo as the shadow
/// ref `refs/hadron/snapshots/<id>`. `commit` is the full snapshot commit SHA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRef {
    pub id: String,
    pub label: String,
    pub commit: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_ref_round_trips() {
        let s = SnapshotRef {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            label: "before edit".into(),
            commit: "9f86d081884c7d659a2feaa0c55ad015".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: SnapshotRef = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
