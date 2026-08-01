//! Parses a [`hadron_lattice::CliSpec::stream`] seat's NDJSON stdout, one line at a
//! time, into incremental deltas (for `live::publish`) and a final
//! `(message, TokenSpend)` pair.
//!
//! Kept separate from `adapter::cli` because it is pure data-in/data-out — no
//! subprocess, no `live_dir` — and is therefore unit-testable without a runner at
//! all.

use hadron_lattice::{StreamFormat, TokenSpend};
use serde_json::Value;

/// Resolve a dotted JSON path against `value`. A segment that parses as a plain
/// integer indexes an array (`"choices.0.delta.content"`); any other segment is
/// an object key. `None` if the path does not resolve on this particular line —
/// callers treat that as "this line said nothing about that field", not an error.
fn resolve_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = value;
    for seg in path.split('.') {
        cur = match seg.parse::<usize>() {
            Ok(idx) => cur.as_array()?.get(idx)?,
            Err(_) => cur.as_object()?.get(seg)?,
        };
    }
    Some(cur)
}

fn path_as_str<'a>(value: &'a Value, path: &str) -> Option<&'a str> {
    resolve_path(value, path)?.as_str()
}

fn path_as_u32(value: &Value, path: &str) -> Option<u32> {
    resolve_path(value, path)?.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// Accumulates one streaming turn's lines. Feed every stdout line via
/// [`Self::on_line`], throttle-publish [`Self::draft`] when it returns `true`,
/// and read the outcome off [`Self::finish`] once the subprocess exits.
pub struct StreamAccumulator {
    format: StreamFormat,
    draft: String,
    status: String,
    final_text: Option<String>,
    spend: TokenSpend,
}

impl StreamAccumulator {
    pub fn new(format: StreamFormat) -> Self {
        StreamAccumulator {
            format,
            draft: String::new(),
            status: String::new(),
            final_text: None,
            spend: TokenSpend::default(),
        }
    }

    /// Feed one stdout line. Returns whether it moved anything a watcher should see
    /// — new delta text, or a new step/tool the quark started. Malformed JSON and
    /// lines carrying neither are simply skipped (most lines in a real stream are
    /// bookkeeping), never an error.
    pub fn on_line(&mut self, line: &str) -> bool {
        let line = line.trim();
        if line.is_empty() {
            return false;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else { return false };
        match &self.format {
            StreamFormat::AgyStreamJson => self.on_agy_line(&value),
            StreamFormat::Ndjson {
                text_delta_path,
                input_tokens_path,
                output_tokens_path,
            } => {
                let mut changed = false;
                if let Some(p) = text_delta_path.as_deref() {
                    if let Some(d) = path_as_str(&value, p) {
                        if !d.is_empty() {
                            self.draft.push_str(d);
                            changed = true;
                        }
                    }
                }
                Self::apply_usage_path(&mut self.spend.input, &value, input_tokens_path.as_deref());
                Self::apply_usage_path(&mut self.spend.output, &value, output_tokens_path.as_deref());
                changed
            }
        }
    }

    fn apply_usage_path(slot: &mut Option<u32>, value: &Value, path: Option<&str>) {
        if let Some(p) = path {
            if let Some(v) = path_as_u32(value, p) {
                *slot = Some(v);
            }
        }
    }

    /// `agy --output-format stream-json`'s shape (verified live 2026-08-01):
    /// `{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"…"}}`
    /// per chunk of text (not every `step_update` carries one — tool calls and
    /// non-text steps have none), and exactly one
    /// `{"event":"result","result":{"response":"…","usage":{"input_tokens":…,"output_tokens":…,"thinking_tokens":…,"cache_read_tokens":…,"total_tokens":…}}}`
    /// at the end.
    ///
    /// `thinking_tokens` is read from nowhere here — see the module doc on
    /// [`crate::adapter::cli::CliQuark`]'s streaming path for why: two live runs
    /// confirmed `total_tokens == input_tokens + output_tokens` exactly, meaning
    /// `output_tokens` already counts thinking; adding it again would double it.
    fn on_agy_line(&mut self, value: &Value) -> bool {
        match value.get("event").and_then(Value::as_str) {
            Some("step_update") => {
                if let Some(delta) = path_as_str(value, "step_update.text_delta") {
                    if !delta.is_empty() {
                        self.draft.push_str(delta);
                        return true;
                    }
                }
                // **Why a step with no text still counts.** Measured live on
                // 2026-08-01: `agy --print --output-format stream-json` emits
                // `text_delta` exactly ONCE, on the final `state:"DONE"`
                // `agent_response` step — there is no token-by-token text at all.
                // Publishing only on a delta therefore left the Live card reading
                // `working…` for the whole turn and flashing the finished reply for
                // one throttle window before `live::clear`, which is precisely the
                // "your stream still doesn't work" the human saw. What IS
                // incremental is the step feed: `state:"ACTIVE"` tool steps naming
                // `view_file`, `run_command`, … So the step's label (see
                // `agy_step_label`) becomes the status line, and that is what streams.
                let Some(step) = value.get("step_update") else { return false };
                let Some(label) = agy_step_label(step) else { return false };
                if label == self.status {
                    return false; // the same step going ACTIVE → DONE says nothing new
                }
                self.status = label;
                true
            }
            Some("result") => {
                if let Some(text) = path_as_str(value, "result.response") {
                    self.final_text = Some(text.to_string());
                }
                if let Some(v) = path_as_u32(value, "result.usage.input_tokens") {
                    self.spend.input = Some(v);
                }
                if let Some(v) = path_as_u32(value, "result.usage.output_tokens") {
                    self.spend.output = Some(v);
                }
                if let Some(v) = path_as_u32(value, "result.usage.cache_read_tokens") {
                    self.spend.cache_read = Some(v);
                }
                false // the result line is bookkeeping, never a delta to publish
            }
            _ => false,
        }
    }

    /// The full text accumulated so far — what a throttled `live::publish` should
    /// show after `on_line` returns `true`.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// The last step or tool the stream named — what the Live card shows while the
    /// draft is still empty. Empty until a step names one.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Consume the accumulator: the final message (the format's own final-text
    /// field if it ever reported one, else the deltas concatenated so far — a
    /// fallback for a stream that dies before its result line) and the spend.
    pub fn finish(self) -> (Option<String>, TokenSpend) {
        let message = self.final_text.or_else(|| (!self.draft.is_empty()).then_some(self.draft));
        (message, self.spend)
    }
}

/// The Live-card line for one agy `step_update`, or `None` when the step names
/// no work worth showing.
///
/// A tool step also carries `tool_info.parameters` — captured live on
/// 2026-08-01: `{"name":"run_command","parameters":{"CommandLine":"ls -la"}}`,
/// `{"name":"view_file","parameters":{"AbsolutePath":"/tmp/x/a.txt"}}` — so the
/// card can say `run_command: ls -la` rather than a bare tool name. The
/// parameter KEY differs per tool, so the first usable string value is taken
/// instead of a hardcoded key.
fn agy_step_label(step: &Value) -> Option<String> {
    if let Some(tool) = step.get("tool_name").and_then(Value::as_str) {
        let arg = step
            .get("tool_info")
            .and_then(|info| info.get("parameters"))
            .and_then(Value::as_object)
            .and_then(|params| params.values().filter_map(Value::as_str).find_map(short_arg));
        return Some(match arg {
            Some(arg) => format!("{tool}: {arg}"),
            None => tool.to_string(),
        });
    }
    match step.get("step_type").and_then(Value::as_str)? {
        // Neither names any work: `checkpoint` and `unknown` were 2 of the 17
        // steps in the live capture and say nothing a human can act on.
        "checkpoint" | "unknown" => None,
        "agent_response" => Some("thinking".to_string()),
        "user_input" => Some("reading the message".to_string()),
        other => Some(other.replace('_', " ")),
    }
}

/// One short line for a tool parameter. `None` for anything that is not a
/// glanceable label: empty, or long enough to be a file body rather than a path
/// (an `edit_file`-shaped tool passes both, and the map is not ordered).
fn short_arg(value: &str) -> Option<String> {
    const MAX_SOURCE: usize = 200;
    const MAX_SHOWN: usize = 48;
    if value.len() > MAX_SOURCE {
        return None;
    }
    let line = value.lines().next().unwrap_or_default().trim();
    // An absolute path is mostly directory the human already knows.
    let short = line.rsplit('/').next().filter(|_| line.starts_with('/')).unwrap_or(line);
    if short.is_empty() {
        return None;
    }
    Some(match short.char_indices().nth(MAX_SHOWN) {
        Some((cut, _)) => format!("{}…", &short[..cut]),
        None => short.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agy() -> StreamAccumulator {
        StreamAccumulator::new(StreamFormat::AgyStreamJson)
    }

    /// The real shape, captured live (see module docs): `init` and non-text
    /// `step_update`s carry no delta; text arrives on `agent_response` steps;
    /// the `result` line's `response`/`usage` sit one level under `result`.
    #[test]
    fn agy_shape_accumulates_deltas_and_reads_the_nested_result() {
        let mut acc = agy();
        let lines = [
            r#"{"event":"init","conversation_id":"abc"}"#,
            r#"{"event":"step_update","step_update":{"step_type":"user_input","state":"DONE"}}"#,
            r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"A git "}}"#,
            r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"rebase moves commits."}}"#,
            r#"{"event":"result","result":{"response":"A git rebase moves commits.","usage":{"input_tokens":100,"output_tokens":20,"thinking_tokens":15,"cache_read_tokens":50,"total_tokens":120}}}"#,
        ];
        let mut deltas = 0;
        for line in lines {
            if acc.on_line(line) {
                deltas += 1;
            }
        }
        assert_eq!(
            deltas, 3,
            "the two agent_response deltas, plus the `user_input` step naming a new status; \
             `init` carries neither"
        );
        assert_eq!(acc.draft(), "A git rebase moves commits.");

        let (message, spend) = acc.finish();
        assert_eq!(message.as_deref(), Some("A git rebase moves commits."));
        assert_eq!(spend.input, Some(100));
        assert_eq!(spend.output, Some(20));
        assert_eq!(spend.cache_read, Some(50));
        assert_eq!(
            spend.output,
            Some(20),
            "thinking_tokens (15) must NOT be folded into output — total_tokens (120) already \
             equals input+output (100+20) with no room for it, so it is already inside output_tokens"
        );
        assert_eq!(spend.cache_write, None, "agy's stream-json reports no cache-write column");
    }

    /// A line with no delta and a genuinely malformed line must both be skipped,
    /// not crash the accumulator or the turn.
    #[test]
    fn unparseable_and_delta_free_lines_are_skipped_not_errors() {
        let mut acc = agy();
        assert!(!acc.on_line("not json at all"));
        assert!(!acc.on_line(""));
        assert!(!acc.on_line(r#"{"event":"step_update"}"#), "a step_update with no step at all");
        assert!(!acc.on_line(r#"{"event":"step_update","step_update":{"state":"DONE"}}"#), "no tool_name, no step_type");
        assert!(!acc.on_line(r#"{"event":"result","result":{"response":"Canonical final message","usage":{"input_tokens":100,"output_tokens":25}}}"#));
        let (message, spend) = acc.finish();
        assert_eq!(message.as_deref(), Some("Canonical final message"));
        assert_eq!(spend.input, Some(100));
        assert_eq!(spend.output, Some(25), "thinking_tokens are folded into output");
        assert_eq!(spend.cache_read, None);
    }

    /// **The bug the human actually saw.** Captured verbatim from a live
    /// `agy --print --output-format stream-json` run on 2026-08-01: the ONLY
    /// `text_delta` in the whole stream is on the last `agent_response` step, so a
    /// watcher that publishes only on deltas has nothing to show for the entire
    /// turn — `working…`, then the finished answer. The tool steps in between are
    /// the incremental signal, and they must be publish-worthy.
    #[test]
    fn a_stream_whose_text_only_arrives_at_the_end_still_reports_its_tools() {
        let mut acc = agy();
        let lines = [
            r#"{"event":"init","conversation_id":"abc"}"#,
            r#"{"event":"step_update","step_update":{"step_index":0,"state":"DONE","step_type":"user_input"}}"#,
            r#"{"event":"step_update","step_update":{"step_index":3,"state":"ACTIVE","step_type":"tool","tool_name":"view_file"}}"#,
            r#"{"event":"step_update","step_update":{"step_index":3,"state":"DONE","step_type":"tool","tool_name":"view_file"}}"#,
            r#"{"event":"step_update","step_update":{"step_index":4,"state":"ACTIVE","step_type":"tool","tool_name":"run_command"}}"#,
        ];
        let published: Vec<&str> = lines
            .iter()
            .filter(|l| acc.on_line(l))
            .map(|_| "")
            .collect();
        assert_eq!(
            published.len(),
            3,
            "user_input, view_file and run_command each move the card; view_file going ACTIVE→DONE does not"
        );
        assert_eq!(acc.status(), "run_command", "the card shows the tool running now");
        assert!(acc.draft().is_empty(), "not one delta arrived — that is the point");

        // …and the single end-of-turn delta still becomes the reply.
        assert!(acc.on_line(
            r#"{"event":"step_update","step_update":{"state":"DONE","step_type":"agent_response","text_delta":"Hello! Greetings user!"}}"#
        ));
        assert_eq!(acc.draft(), "Hello! Greetings user!");
    }

    /// **What the human saw next**: the card moved, but it read `user_input`,
    /// `agent_response` — agy's own slugs, naming no work. Captured verbatim from
    /// a second live run on 2026-08-01, this time one that used tools: every tool
    /// step also carries `tool_info.parameters`, so the card can name the file or
    /// the command. `checkpoint` and `unknown` steps name nothing and are dropped.
    #[test]
    fn a_tool_step_is_labelled_with_its_file_or_command_and_noise_steps_are_dropped() {
        let mut acc = agy();
        let mut statuses: Vec<String> = Vec::new();
        for line in [
            r#"{"event":"step_update","step_update":{"step_index":0,"state":"DONE","step_type":"user_input"}}"#,
            r#"{"event":"step_update","step_update":{"step_index":1,"state":"DONE","step_type":"unknown"}}"#,
            r#"{"event":"step_update","step_update":{"step_index":2,"state":"DONE","step_type":"agent_response"}}"#,
            r#"{"event":"step_update","step_update":{"step_index":3,"state":"ACTIVE","step_type":"tool","tool_name":"view_file","tool_info":{"name":"view_file","parameters":{"AbsolutePath":"/tmp/agy-stream-probe/a.txt"}}}}"#,
            r#"{"event":"step_update","step_update":{"step_index":4,"state":"DONE","step_type":"checkpoint"}}"#,
            r#"{"event":"step_update","step_update":{"step_index":6,"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"ls -la"}}}}"#,
        ] {
            if acc.on_line(line) {
                statuses.push(acc.status().to_string());
            }
        }

        assert_eq!(
            statuses,
            [
                "reading the message",
                "thinking",
                "view_file: a.txt",
                "run_command: ls -la"
            ],
            "the two work-free steps (`unknown`, `checkpoint`) publish nothing, and a tool step \
             names its argument — the absolute path shortened to its basename"
        );
    }

    /// A parameter that is a file BODY rather than a path must never reach the
    /// card: the parameter map is unordered, so an edit-shaped tool would
    /// otherwise have a coin-flip chance of pasting a whole file into it.
    #[test]
    fn an_oversized_or_absent_tool_parameter_leaves_the_bare_tool_name() {
        let body = "x".repeat(500);
        let mut acc = agy();
        assert!(acc.on_line(&format!(
            r#"{{"event":"step_update","step_update":{{"step_type":"tool","tool_name":"edit_file","tool_info":{{"parameters":{{"Content":"{body}"}}}}}}}}"#
        )));
        assert_eq!(acc.status(), "edit_file", "a 500-char body is not a label");

        let mut acc = agy();
        assert!(acc.on_line(
            r#"{"event":"step_update","step_update":{"step_type":"tool","tool_name":"list_dir"}}"#
        ));
        assert_eq!(acc.status(), "list_dir", "no tool_info at all is still a usable label");
    }

    #[test]
    fn missing_result_line_falls_back_to_accumulated_deltas() {
        let mut acc = agy();
        acc.on_line(r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"partial reply"}}"#);
        let (message, spend) = acc.finish();
        assert_eq!(message.as_deref(), Some("partial reply"));
        assert_eq!(spend.input, None, "no usage was ever reported");
    }

    #[test]
    fn ndjson_format_resolves_dotted_paths_including_array_indices() {
        let mut acc = StreamAccumulator::new(StreamFormat::Ndjson {
            text_delta_path: Some("choices.0.delta.content".to_string()),
            input_tokens_path: Some("usage.prompt_tokens".to_string()),
            output_tokens_path: Some("usage.completion_tokens".to_string()),
        });
        assert!(acc.on_line(r#"{"choices":[{"delta":{"content":"Hel"}}]}"#));
        assert!(acc.on_line(r#"{"choices":[{"delta":{"content":"lo"}}]}"#));
        assert!(!acc.on_line(r#"{"choices":[{"delta":{}}]}"#), "no content key at all: no delta");
        assert!(!acc.on_line(
            r#"{"usage":{"prompt_tokens":5,"completion_tokens":2}}"#
        ), "usage line");

        let (message, spend) = acc.finish();
        assert_eq!(message.as_deref(), Some("Hello"));
        assert_eq!(spend.input, Some(5));
        assert_eq!(spend.output, Some(2));
    }

    #[test]
    fn ndjson_format_without_a_final_text_path_falls_back_to_deltas() {
        let mut acc = StreamAccumulator::new(StreamFormat::Ndjson {
            text_delta_path: Some("delta".to_string()),
            input_tokens_path: None,
            output_tokens_path: None,
        });
        acc.on_line(r#"{"delta":"hi there"}"#);
        let (message, _) = acc.finish();
        assert_eq!(message.as_deref(), Some("hi there"));
    }
}
