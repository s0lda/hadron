//! Swarm Attention & File Heatmap Model (Capability #13).
//!
//! Aggregates active file focus across quarks' in-flight activities and recent tool/edit events.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionLevel {
    Cold,
    Warm,
    Hot,
    ActiveEditing,
}

impl AttentionLevel {
    pub fn badge_label(&self) -> &'static str {
        match self {
            Self::Cold => "idle",
            Self::Warm => "reading",
            Self::Hot => "active",
            Self::ActiveEditing => "editing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAttention {
    pub path: String,
    pub level: AttentionLevel,
    pub active_quarks: Vec<String>,
    pub access_count: usize,
}

/// Extracts active file attention from in-flight quark activities and recent tool activity.
pub fn compute_file_attention(
    live_activities: &HashMap<String, Option<hadron_lattice::live::Activity>>,
    recent_file_edits: &[(String, String)], // (quark_id, file_path)
) -> Vec<FileAttention> {
    let mut file_map: HashMap<String, (HashSet<String>, usize, bool)> = HashMap::new();

    // 1. Scan live activities for paths mentioned in detail
    for (quark_id, activity_opt) in live_activities {
        if let Some(act) = activity_opt {
            if act.is_fresh(chrono::Utc::now()) {
                // Find path-like tokens in act.detail
                for token in act.detail.split_whitespace() {
                    let cleaned = token.trim_matches(|c| c == '\'' || c == '"' || c == '`' || c == ',' || c == ':');
                    if (cleaned.contains('/') || cleaned.contains('.')) && !cleaned.starts_with("http") && cleaned.len() > 3 {
                        let entry = file_map.entry(cleaned.to_string()).or_insert_with(|| (HashSet::new(), 0, false));
                        entry.0.insert(quark_id.clone());
                        entry.1 += 1;
                        if act.doing == hadron_lattice::live::Doing::Working && (act.detail.contains("edit") || act.detail.contains("write")) {
                            entry.2 = true;
                        }
                    }
                }
            }
        }
    }

    // 2. Scan recent edits
    for (quark_id, path) in recent_file_edits {
        let entry = file_map.entry(path.clone()).or_insert_with(|| (HashSet::new(), 0, false));
        entry.0.insert(quark_id.clone());
        entry.1 += 1;
        entry.2 = true;
    }

    let mut result = Vec::new();
    for (path, (quarks, count, is_editing)) in file_map {
        let level = if is_editing {
            AttentionLevel::ActiveEditing
        } else if count >= 3 || quarks.len() > 1 {
            AttentionLevel::Hot
        } else if count > 0 {
            AttentionLevel::Warm
        } else {
            AttentionLevel::Cold
        };

        let mut quark_list: Vec<String> = quarks.into_iter().collect();
        quark_list.sort();

        result.push(FileAttention {
            path,
            level,
            active_quarks: quark_list,
            access_count: count,
        });
    }

    result.sort_by(|a, b| b.level.cmp(&a.level).then_with(|| b.access_count.cmp(&a.access_count)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::live::{Activity, Doing};
    use chrono::Utc;

    #[test]
    fn test_compute_file_attention() {
        let mut live = HashMap::new();
        live.insert(
            "cli-agy".to_string(),
            Some(Activity {
                quark: hadron_lattice::QuarkId::new("cli-agy"),
                at: Utc::now(),
                doing: Doing::Working,
                detail: "editing crates/hadron-chamber/src/app/mod.rs".to_string(),
                full: None,
                started: None,
            }),
        );
        live.insert(
            "claude".to_string(),
            Some(Activity {
                quark: hadron_lattice::QuarkId::new("claude"),
                at: Utc::now(),
                doing: Doing::Working,
                detail: "reading crates/hadron-chamber/src/app/mod.rs".to_string(),
                full: None,
                started: None,
            }),
        );

        let edits = vec![("cli-agy".to_string(), "crates/hadron-lattice/src/lib.rs".to_string())];
        let attentions = compute_file_attention(&live, &edits);

        assert_eq!(attentions.len(), 2);
        let top = &attentions[0];
        assert_eq!(top.path, "crates/hadron-chamber/src/app/mod.rs");
        assert_eq!(top.level, AttentionLevel::ActiveEditing);
        assert_eq!(top.active_quarks.len(), 2);
    }
}
