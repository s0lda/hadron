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
    needs_step_break: bool,
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
            needs_step_break: false,
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
                if !self.draft.is_empty() && doing != Doing::Speaking {
                    self.needs_step_break = true;
                }
                self.last_doing = doing;
                self.last_detail = detail.clone();
                let mut act = Activity::new(self.quark_id.clone(), doing, &detail);
                if !self.draft.is_empty() {
                    act.full = Some(self.draft.clone());
                }
                act
            }
            AdapterStreamEvent::ToolStarted { name, summary } => {
                if !self.draft.is_empty() {
                    self.needs_step_break = true;
                }
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
                if !self.draft.is_empty() {
                    self.needs_step_break = true;
                }
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
                append_message_chunk_with_step_break(
                    &mut self.draft,
                    &text,
                    &mut self.needs_step_break,
                );
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

/// Append a streaming message chunk to `transcript`, inserting a newline when
/// either `needs_step_break` is set (a tool call, thought step, or status transition
/// occurred since the previous text chunk) or `chunk` starts with `@` (so
/// `parse_all_addressees` line-start mention routing always succeeds), without
/// inserting spurious mid-word newlines during token-by-token streaming within a step.
pub fn append_message_chunk_with_step_break(
    transcript: &mut String,
    chunk: &str,
    needs_step_break: &mut bool,
) {
    if chunk.is_empty() {
        return;
    }
    if !transcript.is_empty()
        && (*needs_step_break || chunk.starts_with('@'))
        && !transcript.ends_with('\n')
        && !chunk.starts_with('\n')
    {
        transcript.push('\n');
    }
    *needs_step_break = false;
    transcript.push_str(chunk);
}

fn is_fence_delimiter(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn is_transient_waiting_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let body = trimmed.strip_prefix('-').map(str::trim_start).unwrap_or(trimmed);
    let lower = body.to_ascii_lowercase();
    lower.starts_with("waiting for ")
        && (lower.ends_with('.') || lower.ends_with('…') || lower.ends_with("..."))
        && trimmed.len() <= 140
}

fn collapse_inline_repetition(line: &str) -> String {
    // First split glued `- Waiting for ` polling status fragments on the same line.
    let split_waiting = if line.contains(".- Waiting for ") {
        line.replace(".- Waiting for ", ".\n- Waiting for ")
    } else {
        line.to_string()
    };
    let mut out_lines = Vec::new();
    for raw_sub in split_waiting.split('\n') {
        let trimmed = raw_sub.trim();
        let len = trimmed.len();
        if len >= 24 {
            let half = len / 2;
            if len % 2 == 0 && trimmed.is_char_boundary(half) && trimmed[..half] == trimmed[half..]
            {
                out_lines.push(trimmed[..half].to_string());
                continue;
            }
            if len % 2 == 1
                && trimmed.is_char_boundary(half)
                && trimmed.is_char_boundary(half + 1)
                && &trimmed[half..=half] == " "
                && trimmed[..half] == trimmed[half + 1..]
            {
                out_lines.push(trimmed[..half].to_string());
                continue;
            }
        }
        out_lines.push(raw_sub.to_string());
    }
    out_lines.join("\n")
}

/// Normalize and deduplicate a Quark's turn message across all providers (Antigravity,
/// Claude, OpenAI/Codex, Gemini, Local HTTP) and transports (ACP, CLI, Local).
///
/// Handles three universal failure modes without touching content inside markdown
/// code fences (` ``` ` / `~~~`):
/// 1. Splits glued mid-line `- Waiting for ...` polling updates and collapses exact
///    intra-line doubled sentences (`S + S`).
/// 2. Strips leading `- Waiting for ...` transient polling status lines when followed
///    by the actual response/report (preserving them if the turn was cancelled while
///    waiting and produced no other text).
/// 3. Collapses consecutive duplicate single-line and multi-line response blocks
///    (e.g., when a multi-step agent or stream emits the final `@orchestrator` /
///    `**Done**:` report or paragraph block twice across consecutive steps or
///    delta+summary events).
pub fn deduplicate_turn_message(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }

    // Pass 1: expand glued inline waiting updates & collapse exact inline `S + S` outside fences.
    let mut expanded_lines: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in raw.lines() {
        if is_fence_delimiter(line) {
            in_fence = !in_fence;
            expanded_lines.push(line.to_string());
            continue;
        }
        if in_fence {
            expanded_lines.push(line.to_string());
        } else {
            let collapsed = collapse_inline_repetition(line);
            for sub in collapsed.split('\n') {
                expanded_lines.push(sub.to_string());
            }
        }
    }

    // Pass 2: strip leading transient `- Waiting for ...` status lines when real content follows.
    let first_non_waiting = expanded_lines.iter().position(|l| {
        let t = l.trim();
        !t.is_empty() && !is_transient_waiting_line(t)
    });
    let working_slice = match first_non_waiting {
        Some(idx) if idx > 0 => &expanded_lines[idx..],
        _ => &expanded_lines[..],
    };

    // Pass 3: collapse consecutive duplicate single-line and multi-line blocks at
    // non-fenced boundaries.
    let mut out: Vec<String> = Vec::with_capacity(working_slice.len());
    // `fence_before[i]` records whether line `out[i]` starts inside an open code fence.
    let mut fence_before: Vec<bool> = Vec::with_capacity(working_slice.len());
    let mut cur_fence = false;

    for line in working_slice {
        fence_before.push(cur_fence);
        if is_fence_delimiter(line) {
            cur_fence = !cur_fence;
        }
        out.push(line.clone());

        // Only evaluate block deduplication when we are outside a code fence after `line`.
        if cur_fence {
            continue;
        }

        loop {
            let m = out.len();
            let mut truncated_to = None;

            for k in (1..=m / 2).rev() {
                let start1 = m - 2 * k;
                let start2 = m - k;
                if !fence_before[start1] && !fence_before[start2] {
                    let block = &out[start2..m];
                    let has_non_empty = block.iter().any(|l| !l.trim().is_empty());
                    if !has_non_empty {
                        continue;
                    }
                    if k == 1 {
                        let t = block[0].trim();
                        let qualifying_single = t.len() >= 12
                            || t.starts_with('@')
                            || t.starts_with("**")
                            || t.starts_with("- ");
                        if !qualifying_single {
                            continue;
                        }
                    }
                    let matches_adjacent = (0..k).all(|i| {
                        out[start1 + i].trim_end() == out[start2 + i].trim_end()
                    });
                    if matches_adjacent {
                        truncated_to = Some(start2);
                        break;
                    }
                }

                // Also check multi-line blocks (k >= 2) separated by a single blank line.
                if k >= 2 && m >= 2 * k + 1 {
                    let sep = m - k - 1;
                    let b1 = m - 2 * k - 1;
                    let b2 = m - k;
                    if out[sep].trim().is_empty()
                        && !fence_before[b1]
                        && !fence_before[sep]
                        && !fence_before[b2]
                    {
                        let block = &out[b2..m];
                        if block.iter().any(|l| !l.trim().is_empty())
                            && (0..k).all(|i| out[b1 + i].trim_end() == out[b2 + i].trim_end())
                        {
                            truncated_to = Some(sep);
                            break;
                        }
                    }
                }
            }

            if let Some(new_len) = truncated_to {
                out.truncate(new_len);
                fence_before.truncate(new_len);
            } else {
                break;
            }
        }
    }

    let mut joined = out.join("\n");
    if raw.ends_with('\n') && !joined.is_empty() && !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
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

    #[test]
    fn deduplicate_turn_message_cleans_glued_waiting_lines_and_doubled_report() {
        let report = "@orchestrator\n\
**Done**: Executed Task 2 of `/home/Jake/dev/hadron/.hadron/docs/plans/2026-11-03-single-lane-instant-job-queue.md` in `crates/hadron-chamber/` (`3cfd81e0`).\n\
\n\
- **Done**:\n\
  - Retired `/steer` and `/cancel` commands from `COMMANDS` in `crates/hadron-chamber/src/text.rs`.\n\
  - Unlisted `/kill` (`listed: false`) and updated `/stop` detail string.\n\
- **Evidence**:\n\
  - `cargo test --lib -p hadron`: `test result: ok. 598 passed; 0 failed`.\n\
- **Risks**: No new attack surface.\n\
- **What I did not verify / clean up**: Did not touch `crates/hadron-gluon/` (Task 1 scope).";

        let raw = format!(
            "- Waiting for `cargo test --lib -p hadron` task completion.- Waiting for `cargo test --lib -p hadron` completion.- Waiting for `cargo test` to complete.- Waiting for `cargo test --lib -p hadron` completion.\n{report}\n{report}"
        );

        let cleaned = deduplicate_turn_message(&raw);
        assert_eq!(cleaned, report);
    }

    #[test]
    fn deduplicate_turn_message_handles_blank_line_separated_and_inline_repeats_across_providers() {
        // Multi-line block separated by a blank line (e.g. Claude / OpenAI multi-step or delta+summary)
        let block = "@orchestrator\n**Done**: Completed changes (`1234abcd`).\n\n- **Evidence**: `cargo test` ok.";
        let doubled_with_blank = format!("{block}\n\n{block}");
        assert_eq!(deduplicate_turn_message(&doubled_with_blank), block);

        // Inline doubled sentence on the same line (e.g. NDJSON delta + full text chunk)
        let sentence = "The build succeeded with zero warnings.";
        let inline_doubled = format!("{sentence}{sentence}");
        assert_eq!(deduplicate_turn_message(&inline_doubled), sentence);

        // Cancelled turn with ONLY waiting lines preserves one waiting line instead of emptying
        let waiting_only = "- Waiting for `cargo test` completion.- Waiting for `cargo test` completion.";
        assert_eq!(
            deduplicate_turn_message(waiting_only),
            "- Waiting for `cargo test` completion."
        );
    }

    #[test]
    fn deduplicate_turn_message_preserves_repeated_lines_inside_code_fences() {
        let with_fence = "Here is the output:\n```text\nduplicate line inside fence\nduplicate line inside fence\n```\nDone.";
        assert_eq!(deduplicate_turn_message(with_fence), with_fence);

        // But if the entire response block (including a balanced code fence) is emitted twice, deduplicate it:
        let doubled_whole = format!("{with_fence}\n{with_fence}");
        assert_eq!(deduplicate_turn_message(&doubled_whole), with_fence);
    }

    #[test]
    fn append_message_chunk_with_step_break_separates_steps_without_breaking_token_streams() {
        let mut transcript = String::new();
        let mut step_break = false;

        // Token-by-token within Step 1 (e.g. Claude / OpenAI streaming)
        append_message_chunk_with_step_break(&mut transcript, "Checking ", &mut step_break);
        append_message_chunk_with_step_break(&mut transcript, "files.", &mut step_break);
        assert_eq!(transcript, "Checking files.");

        // Tool call occurs between Step 1 and Step 2
        step_break = true;
        append_message_chunk_with_step_break(&mut transcript, "All tests ", &mut step_break);
        append_message_chunk_with_step_break(&mut transcript, "passed.", &mut step_break);
        assert_eq!(transcript, "Checking files.\nAll tests passed.");
    }
}
