use std::path::Path;
use hadron_lattice::{Actor, Event, Kind, Projection};
use serde::{Deserialize, Serialize};

/// Configuration for the sliding context pruner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlidingPrunerConfig {
    /// Number of recent events to preserve verbatim.
    pub max_recent_events: usize,
    /// Maximum estimated tokens for the generated summary block.
    pub max_summary_tokens: usize,
    /// Token threshold after which older events are summarized rather than kept raw.
    pub compaction_threshold_tokens: usize,
}

impl Default for SlidingPrunerConfig {
    fn default() -> Self {
        Self {
            max_recent_events: 8,
            max_summary_tokens: 300,
            compaction_threshold_tokens: 2000,
        }
    }
}

/// A sliding context pruner that compacts older field events into a rolling summary
/// while preserving the static cache prefix and recent turns verbatim.
pub struct SlidingContextPruner;

impl SlidingContextPruner {
    /// Estimate token count using a simple ~4 characters per token heuristic.
    pub fn estimate_tokens(text: &str) -> usize {
        (text.len() + 3) / 4
    }

    /// Summarize a slice of older field events into a compact summary string.
    pub fn summarize_events(events: &[Event], max_tokens: usize) -> String {
        if events.is_empty() {
            return String::new();
        }

        let mut lines = Vec::new();
        let max_chars = max_tokens * 4;
        let mut current_chars = 0;

        for event in events {
            if let Kind::Message { body } = &event.kind {
                let sender = match &event.from {
                    Actor::Human => "human".to_string(),
                    Actor::Gluon => "gluon".to_string(),
                    Actor::Quark(id) => id.as_str().to_string(),
                };

                let recipient = event.to.as_ref().map(|t| format!(" -> @{}", t.as_str())).unwrap_or_default();

                // Take first non-empty line or first 120 chars
                let preview = body
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or(body.as_str())
                    .trim();
                let clipped: String = if preview.chars().count() > 120 {
                    format!("{}...", preview.chars().take(117).collect::<String>())
                } else {
                    preview.to_string()
                };

                let entry = format!("- [{sender}{recipient}]: {clipped}");
                if current_chars + entry.len() > max_chars {
                    lines.push("... [additional older events omitted]".to_string());
                    break;
                }
                current_chars += entry.len() + 1;
                lines.push(entry);
            }
        }

        lines.join("\n")
    }

    /// Prune an event list, summarizing older events when count or token size exceeds limits.
    /// Returns the pruned events and a bool indicating if compaction occurred.
    pub fn prune_events(events: &[Event], config: &SlidingPrunerConfig) -> (Vec<Event>, bool) {
        if events.is_empty() {
            return (Vec::new(), false);
        }

        let total_chars: usize = events
            .iter()
            .map(|e| match &e.kind {
                Kind::Message { body } => body.len(),
                _ => 0,
            })
            .sum();
        let total_tokens = (total_chars + 3) / 4;

        if events.len() <= config.max_recent_events && total_tokens <= config.compaction_threshold_tokens {
            return (events.to_vec(), false);
        }

        // Determine split point
        let split_ix = events.len().saturating_sub(config.max_recent_events);
        if split_ix == 0 {
            return (events.to_vec(), false);
        }

        let older = &events[..split_ix];
        let recent = &events[split_ix..];

        let summary = Self::summarize_events(older, config.max_summary_tokens);
        let summary_body = format!(
            "[Sliding Context Pruner: compacted {} older field events into summary]\n{}",
            older.len(),
            summary
        );

        let summary_event = Event::new(
            Actor::Gluon,
            None,
            Kind::Message {
                body: summary_body,
            },
        );

        let mut pruned = Vec::with_capacity(recent.len() + 1);
        pruned.push(summary_event);
        pruned.extend_from_slice(recent);

        (pruned, true)
    }

    /// Compact a projection's field window in place using the sliding pruner.
    pub fn prune_projection(projection: &mut Projection, config: &SlidingPrunerConfig) -> bool {
        let scratch_dir = projection.cwd.join(".hadron").join("scratch");
        let _ = std::fs::create_dir_all(&scratch_dir);
        let mut folded_any = false;
        for ev in &mut projection.field_window {
            if let Kind::Message { body } = &mut ev.kind {
                let (folded, did_fold) = Self::fold_test_output(body, &scratch_dir);
                if did_fold {
                    *body = folded;
                    folded_any = true;
                }
            }
        }
        let (pruned, compacted) = Self::prune_events(&projection.field_window, config);
        if compacted {
            projection.field_window = pruned;
            projection.field_truncated = true;
        }
        compacted || folded_any
    }

    /// Fold long test runner output into a compact summary, persisting the full output to disk.
    pub fn fold_test_output(raw: &str, scratch_dir: &Path) -> (String, bool) {
        let line_count = raw.lines().count();
        if line_count < 30 || !raw.contains("test result:") {
            return (raw.to_string(), false);
        }

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let log_file = scratch_dir.join(format!("test_output_{ts}.log"));
        let _ = std::fs::write(&log_file, raw);

        let summary_line = raw
            .lines()
            .rev()
            .find(|l| l.contains("test result:"))
            .unwrap_or("test suite completed");

        let folded = format!(
            "[Folded test output: {line_count} lines. Full log saved to {}]\n{}",
            log_file.display(),
            summary_line
        );
        (folded, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{QuarkId, Kind};

    #[test]
    fn test_sliding_pruner_no_op_when_small() {
        let events = vec![
            Event::new(Actor::Human, None, Kind::Message { body: "Hello".to_string() }),
            Event::new(Actor::Quark(QuarkId::new("agy")), None, Kind::Message { body: "Hi there".to_string() }),
        ];
        let config = SlidingPrunerConfig::default();
        let (pruned, compacted) = SlidingContextPruner::prune_events(&events, &config);
        assert!(!compacted);
        assert_eq!(pruned.len(), 2);
    }

    #[test]
    fn test_sliding_pruner_compacts_older_events() {
        let mut events = Vec::new();
        for i in 0..15 {
            events.push(Event::new(
                Actor::Human,
                Some(QuarkId::new("agy")),
                Kind::Message {
                    body: format!("Message number {} with details", i),
                },
            ));
        }

        let config = SlidingPrunerConfig {
            max_recent_events: 5,
            max_summary_tokens: 200,
            compaction_threshold_tokens: 100,
        };

        let (pruned, compacted) = SlidingContextPruner::prune_events(&events, &config);
        assert!(compacted);
        assert_eq!(pruned.len(), 6); // 1 summary + 5 recent

        if let Kind::Message { body } = &pruned[0].kind {
            assert!(body.contains("compacted 10 older field events"));
            assert!(body.contains("Message number 0"));
        } else {
            panic!("Expected summary message event at head");
        }

        // The last event should still be message 14
        if let Kind::Message { body } = &pruned.last().unwrap().kind {
            assert!(body.contains("Message number 14"));
        } else {
            panic!("Expected message 14 at tail");
        }
    }

    #[test]
    fn test_fold_test_output_compacts_large_test_suites() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = Vec::new();
        lines.push("running 45 tests".to_string());
        for i in 0..60 {
            lines.push(format!("test test_case_{i} ... ok"));
        }
        lines.push("test result: ok. 45 passed; 0 failed; 0 ignored".to_string());
        let raw = lines.join("\n");

        let (folded, did_fold) = SlidingContextPruner::fold_test_output(&raw, tmp.path());
        assert!(did_fold);
        assert!(folded.contains("[Folded test output: 62 lines. Full log saved to"));
        assert!(folded.contains("test result: ok. 45 passed; 0 failed"));
    }
}
