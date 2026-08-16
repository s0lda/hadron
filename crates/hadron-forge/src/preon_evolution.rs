use serde::{Deserialize, Serialize};

/// Preon evolution helper in hadron-forge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureCluster {
    pub category: String,
    pub sample_notes: Vec<String>,
    pub recurrence: usize,
}

pub struct PreonForge;

impl PreonForge {
    pub fn cluster_notes(notes: &[(String, String)]) -> Vec<FailureCluster> {
        let mut clusters = std::collections::HashMap::new();

        for (slug, content) in notes {
            let cat = if content.contains("lavapipe") || content.contains("gpu") || content.contains("render") {
                "rendering"
            } else if content.contains("acp") || content.contains("ipc") || content.contains("ndjson") {
                "ipc"
            } else if content.contains("worktree") || content.contains("gate") || content.contains("merge") {
                "worktree_gate"
            } else {
                "general"
            };

            clusters
                .entry(cat.to_string())
                .or_insert_with(Vec::new)
                .push(slug.clone());
        }

        clusters
            .into_iter()
            .map(|(category, sample_notes)| {
                let recurrence = sample_notes.len();
                FailureCluster {
                    category,
                    sample_notes,
                    recurrence,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preon_forge_clustering() {
        let notes = vec![
            ("note-1".into(), "lavapipe rendering issue".into()),
            ("note-2".into(), "acp ipc framing error".into()),
        ];
        let clusters = PreonForge::cluster_notes(&notes);
        assert_eq!(clusters.len(), 2);
    }
}
