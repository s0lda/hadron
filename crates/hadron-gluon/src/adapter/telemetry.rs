//! Unified turn stream telemetry sink and normalized stream events across ACP,
//! streaming CLI, and local HTTP adapters.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use hadron_lattice::live::{self, Activity, Doing};
use hadron_lattice::QuarkId;

/// Normalized mid-turn stream event emitted by an adapter (ACP, CLI stream, or HTTP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterStreamEvent {
    /// Transition to a high-level execution phase (`Thinking`, `Working`, `Planning`, `Speaking`).
    PhaseChanged { doing: Doing, detail: String },
    /// A tool invocation started during the turn.
    ToolStarted {
        name: String,
        summary: Option<String>,
    },
    /// A tool invocation finished.
    ToolCompleted { name: String },
    /// Incremental assistant reply text delta.
    DraftDelta { text: String },
    /// Incremental or cumulative token usage telemetry.
    TokenUsage {
        input: u64,
        output: u64,
        cache_read: u64,
    },
}

/// Unified telemetry sink that normalizes live `.hadron/live/<quark>.json` updates
/// and preserves turn start timestamps across ACP, CLI stream, and local HTTP turns.
#[derive(Debug, Clone)]
pub struct TurnTelemetrySink {
    quark_id: QuarkId,
    live_dir: Option<PathBuf>,
    started: DateTime<Utc>,
    draft: String,
    last_doing: Doing,
    last_detail: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
}

impl TurnTelemetrySink {
    /// Create a new telemetry sink for `quark_id` writing to `live_dir` when present.
    pub fn new(quark_id: QuarkId, live_dir: Option<PathBuf>) -> Self {
        Self {
            quark_id,
            live_dir,
            started: Utc::now(),
            draft: String::new(),
            last_doing: Doing::Thinking,
            last_detail: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
        }
    }

    /// Return the accumulated reply draft text so far.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Return the turn's initial start timestamp.
    pub fn started(&self) -> DateTime<Utc> {
        self.started
    }

    /// Record a normalized [`AdapterStreamEvent`], updating sink state and publishing
    /// the resulting [`Activity`] to `live_dir` when configured.
    pub fn record(&mut self, event: AdapterStreamEvent) -> Option<Activity> {
        let mut activity = match event {
            AdapterStreamEvent::PhaseChanged { doing, detail } => {
                self.last_doing = doing;
                self.last_detail = detail.clone();
                let mut act = Activity::new(self.quark_id.clone(), doing, &detail);
                if !self.draft.is_empty() {
                    act.full = Some(self.draft.clone());
                }
                act
            }
            AdapterStreamEvent::ToolStarted { name, summary } => {
                let detail = match summary.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    Some(s) => format!("{name}: {s}"),
                    None => name,
                };
                self.last_doing = Doing::Working;
                self.last_detail = detail.clone();
                let mut act = Activity::new(self.quark_id.clone(), Doing::Working, &detail);
                if !self.draft.is_empty() {
                    act.full = Some(self.draft.clone());
                }
                act
            }
            AdapterStreamEvent::ToolCompleted { name } => {
                self.last_doing = Doing::Thinking;
                self.last_detail = format!("completed {name}");
                let mut act =
                    Activity::new(self.quark_id.clone(), Doing::Thinking, &self.last_detail);
                if !self.draft.is_empty() {
                    act.full = Some(self.draft.clone());
                }
                act
            }
            AdapterStreamEvent::DraftDelta { text } => {
                self.draft.push_str(&text);
                self.last_doing = Doing::Speaking;
                self.last_detail.clear();
                Activity::speaking(self.quark_id.clone(), &self.draft)
            }
            AdapterStreamEvent::TokenUsage {
                input,
                output,
                cache_read,
            } => {
                self.input_tokens = input;
                self.output_tokens = output;
                self.cache_read_tokens = cache_read;
                return None;
            }
        };

        activity.started = Some(self.started);
        if let Some(dir) = &self.live_dir {
            Self::publish_activity(dir, &mut activity);
        }
        Some(activity)
    }

    /// Publish `activity` to `dir`, preserving the initial turn `started` timestamp if
    /// not already set on `activity`.
    pub fn publish_activity(dir: &Path, activity: &mut Activity) {
        if activity.started.is_none() {
            let prev_started = live::read(dir, &activity.quark, Utc::now())
                .and_then(|prev| prev.started.or(Some(prev.at)));
            activity.started = Some(prev_started.unwrap_or(activity.at));
        }
        let _ = live::publish(dir, activity);
    }

    /// Clear the live activity file for `quark` in `dir`.
    pub fn clear_live(dir: &Path, quark: &QuarkId) {
        let _ = live::clear(dir, quark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_sink_records_events_preserves_started_and_writes_live_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let qid = QuarkId::new("acp-agy");
        let mut sink = TurnTelemetrySink::new(qid.clone(), Some(dir.path().to_path_buf()));

        let act1 = sink
            .record(AdapterStreamEvent::ToolStarted {
                name: "read_file".to_string(),
                summary: Some("src/lib.rs".to_string()),
            })
            .expect("tool activity");
        assert_eq!(act1.doing, Doing::Working);
        assert_eq!(act1.detail, "read_file: src/lib.rs");

        let on_disk = live::read(dir.path(), &qid, Utc::now()).expect("live file written");
        assert_eq!(on_disk.doing, Doing::Working);
        assert_eq!(on_disk.started, Some(sink.started()));

        let act2 = sink
            .record(AdapterStreamEvent::DraftDelta {
                text: "Hello ".to_string(),
            })
            .expect("draft 1");
        assert_eq!(act2.doing, Doing::Speaking);
        assert_eq!(act2.full.as_deref(), Some("Hello "));

        // Subsequent tool call preserves accumulated draft in `full` and original `started`
        let act3 = sink
            .record(AdapterStreamEvent::ToolStarted {
                name: "edit_file".to_string(),
                summary: None,
            })
            .expect("tool 2");
        assert_eq!(act3.doing, Doing::Working);
        assert_eq!(act3.full.as_deref(), Some("Hello "));
        assert_eq!(act3.started, Some(sink.started()));

        assert!(sink
            .record(AdapterStreamEvent::TokenUsage {
                input: 1200,
                output: 340,
                cache_read: 5000,
            })
            .is_none());
        assert_eq!(sink.input_tokens, 1200);
        assert_eq!(sink.output_tokens, 340);
        assert_eq!(sink.cache_read_tokens, 5000);

        TurnTelemetrySink::clear_live(dir.path(), &qid);
        assert!(live::read(dir.path(), &qid, Utc::now()).is_none());
    }
}
