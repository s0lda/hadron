//! Visual Time-Lapse Feature Replay Generator.
//!
//! Synthesizes git commit history and event timelines into visual replay frames,
//! illustrating the architectural evolution of a feature from inception to merge.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelapseFrame {
    pub frame_index: usize,
    pub commit_hash: String,
    pub summary: String,
    pub files_changed: usize,
    pub active_quarks: Vec<String>,
}

pub struct TimelapseGenerator;

impl TimelapseGenerator {
    /// Builds structured time-lapse frames from raw commit history tuples.
    pub fn generate_frames(
        commits: &[(&str, &str, usize, &[&str])],
    ) -> Vec<TimelapseFrame> {
        commits
            .iter()
            .enumerate()
            .map(|(idx, (hash, summary, files_count, quarks))| {
                TimelapseFrame {
                    frame_index: idx + 1,
                    commit_hash: hash.to_string(),
                    summary: summary.to_string(),
                    files_changed: *files_count,
                    active_quarks: quarks.iter().map(|s| s.to_string()).collect(),
                }
            })
            .collect()
    }

    /// Renders the time-lapse sequence as a markdown visual timeline.
    pub fn render_markdown_timeline(frames: &[TimelapseFrame]) -> String {
        let mut out = String::from("### Feature Architectural Time-Lapse\n\n");
        for f in frames {
            out.push_str(&format!(
                "**Frame {:02}** | `{}` | {} ({} files modified) — Quarks: {}\n",
                f.frame_index,
                f.commit_hash,
                f.summary,
                f.files_changed,
                f.active_quarks.join(", ")
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timelapse_generation_and_render() {
        let history = vec![
            ("a1b2c3d", "feat(auth): initial JWT validation", 3, &["@agy"][..]),
            ("e4f5g6h", "test(auth): edge case token expiry", 1, &["@reviewer"][..]),
            ("i7j8k9l", "feat(auth): land in main gate", 4, &["@orchestrator", "@agy"][..]),
        ];

        let frames = TimelapseGenerator::generate_frames(&history);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].frame_index, 1);
        assert_eq!(frames[0].commit_hash, "a1b2c3d");

        let md = TimelapseGenerator::render_markdown_timeline(&frames);
        assert!(md.contains("Frame 01"));
        assert!(md.contains("a1b2c3d"));
        assert!(md.contains("initial JWT validation"));
        assert!(md.contains("Quarks: @agy"));
    }
}
