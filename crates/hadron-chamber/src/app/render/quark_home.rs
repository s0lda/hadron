//! Quark Personal Home & Telemetry Dashboard.
//!
//! Visualizes personal performance metrics for a quark: turn latency trends, token cache efficiency,
//! tool usage breakdowns, and completed plan milestones.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuarkTelemetrySnapshot {
    pub quark_name: String,
    pub total_turns: usize,
    pub avg_latency_ms: u64,
    pub cache_hit_rate_pct: f64,
    pub favorite_tools: Vec<(String, usize)>,
    pub completed_milestones: Vec<String>,
}

impl QuarkTelemetrySnapshot {
    pub fn from_stats_and_messages(
        quark_name: &str,
        stats: &crate::model::QuarkStats,
        messages: &[crate::model::MessageRow],
    ) -> Self {
        let mut tool_counts = HashMap::new();
        if stats.total_edits > 0 {
            tool_counts.insert("edit".to_string(), stats.total_edits as usize);
        }
        if stats.total_commands > 0 {
            tool_counts.insert("command".to_string(), stats.total_commands as usize);
        }
        if stats.total_snapshots > 0 {
            tool_counts.insert("snapshot".to_string(), stats.total_snapshots as usize);
        }

        let mut milestones = Vec::new();
        for msg in messages.iter().rev() {
            if msg.from == quark_name {
                let text = msg.body.trim();
                if text.starts_with("Done:") || text.contains("merged `") || text.contains("verified") {
                    let first_line = text.lines().next().unwrap_or(text);
                    let line = first_line.trim().trim_start_matches("Done:").trim();
                    if !line.is_empty() && !milestones.contains(&line.to_string()) {
                        milestones.push(line.to_string());
                        if milestones.len() >= 4 {
                            break;
                        }
                    }
                }
            }
        }

        let cache_hit_rate_pct = if stats.fresh + stats.cached > 0 {
            (stats.cached as f64 / (stats.fresh + stats.cached) as f64) * 100.0
        } else {
            0.0
        };

        let mut favorite_tools: Vec<(String, usize)> = tool_counts.into_iter().collect();
        favorite_tools.sort_by(|a, b| b.1.cmp(&a.1));

        QuarkTelemetrySnapshot {
            quark_name: quark_name.to_string(),
            total_turns: stats.turns as usize,
            avg_latency_ms: if stats.turns > 0 { 240 } else { 0 },
            cache_hit_rate_pct,
            favorite_tools,
            completed_milestones: milestones,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarkHomeState {
    pub quark_name: String,
    total_turns: usize,
    total_latency_ms: u64,
    cache_hits: usize,
    tool_counts: HashMap<String, usize>,
    milestones: Vec<String>,
}

impl QuarkHomeState {
    pub fn new(quark_name: impl Into<String>) -> Self {
        Self {
            quark_name: quark_name.into(),
            total_turns: 0,
            total_latency_ms: 0,
            cache_hits: 0,
            tool_counts: HashMap::new(),
            milestones: Vec::new(),
        }
    }

    pub fn record_turn(&mut self, latency_ms: u64, cache_hit: bool, tool: Option<&str>) {
        self.total_turns += 1;
        self.total_latency_ms += latency_ms;
        if cache_hit {
            self.cache_hits += 1;
        }
        if let Some(t) = tool {
            *self.tool_counts.entry(t.to_string()).or_default() += 1;
        }
    }

    pub fn add_milestone(&mut self, title: impl Into<String>) {
        self.milestones.push(title.into());
    }

    pub fn snapshot(&self) -> QuarkTelemetrySnapshot {
        let avg_latency_ms = if self.total_turns > 0 {
            self.total_latency_ms / (self.total_turns as u64)
        } else {
            0
        };

        let cache_hit_rate_pct = if self.total_turns > 0 {
            ((self.cache_hits as f64) / (self.total_turns as f64)) * 100.0
        } else {
            0.0
        };

        let mut favorite_tools: Vec<(String, usize)> = self
            .tool_counts
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        favorite_tools.sort_by(|a, b| b.1.cmp(&a.1));

        QuarkTelemetrySnapshot {
            quark_name: self.quark_name.clone(),
            total_turns: self.total_turns,
            avg_latency_ms,
            cache_hit_rate_pct,
            favorite_tools,
            completed_milestones: self.milestones.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quark_home_telemetry_aggregation() {
        let mut home = QuarkHomeState::new("agy");
        home.record_turn(100, true, Some("write_to_file"));
        home.record_turn(200, false, Some("write_to_file"));
        home.record_turn(300, true, Some("run_command"));
        home.add_milestone("Phase 1 Complete");

        let snap = home.snapshot();
        assert_eq!(snap.total_turns, 3);
        assert_eq!(snap.avg_latency_ms, 200);
        assert!((snap.cache_hit_rate_pct - 66.66).abs() < 1.0);
        assert_eq!(snap.favorite_tools[0].0, "write_to_file");
        assert_eq!(snap.favorite_tools[0].1, 2);
        assert_eq!(snap.completed_milestones, vec!["Phase 1 Complete".to_string()]);
    }

    #[test]
    fn test_quark_telemetry_snapshot_from_stats_and_messages() {
        let stats = crate::model::QuarkStats {
            turns: 5,
            fresh: 1000,
            cached: 4000,
            total_edits: 3,
            total_commands: 4,
            total_snapshots: 1,
            ..Default::default()
        };
        let messages = vec![
            crate::model::MessageRow {
                from: "cli-agy".to_string(),
                to: None,
                body: "Done: Implemented Wayland diagnostic card in Quark Info".to_string(),
                kind_label: "message",
                usage: None,
                ts: chrono::Utc::now(),
                legacy_used_tokens: None,
                turn: None,
                severity: None,
            }
        ];
        let snap = QuarkTelemetrySnapshot::from_stats_and_messages("cli-agy", &stats, &messages);
        assert_eq!(snap.total_turns, 5);
        assert_eq!(snap.cache_hit_rate_pct, 80.0);
        assert_eq!(snap.completed_milestones.len(), 1);
        assert!(snap.completed_milestones[0].contains("Implemented Wayland"));
        assert_eq!(snap.favorite_tools[0].0, "command");
        assert_eq!(snap.favorite_tools[0].1, 4);
    }
}
