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
    final_text: Option<String>,
    spend: TokenSpend,
}

impl StreamAccumulator {
    pub fn new(format: StreamFormat) -> Self {
        StreamAccumulator { format, draft: String::new(), final_text: None, spend: TokenSpend::default() }
    }

    /// Feed one stdout line. Returns whether it carried new delta text — malformed
    /// JSON and lines with no matching field are simply skipped (most lines in a
    /// real stream are bookkeeping, not text), never an error.
    pub fn on_line(&mut self, line: &str) -> bool {
        let line = line.trim();
        if line.is_empty() {
            return false;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else { return false };
        match &self.format {
            StreamFormat::AgyStreamJson => self.on_agy_line(&value),
            StreamFormat::Ndjson {
                delta,
                final_text,
                usage_input,
                usage_output,
                usage_cache_read,
                usage_cache_write,
            } => {
                let mut changed = false;
                if let Some(d) = path_as_str(&value, delta) {
                    if !d.is_empty() {
                        self.draft.push_str(d);
                        changed = true;
                    }
                }
                if let Some(p) = final_text.as_deref() {
                    if let Some(text) = path_as_str(&value, p) {
                        self.final_text = Some(text.to_string());
                    }
                }
                Self::apply_usage_path(&mut self.spend.input, &value, usage_input.as_deref());
                Self::apply_usage_path(&mut self.spend.output, &value, usage_output.as_deref());
                Self::apply_usage_path(&mut self.spend.cache_read, &value, usage_cache_read.as_deref());
                Self::apply_usage_path(&mut self.spend.cache_write, &value, usage_cache_write.as_deref());
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
                let Some(delta) = path_as_str(value, "step_update.text_delta") else { return false };
                if delta.is_empty() {
                    return false;
                }
                self.draft.push_str(delta);
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

    /// Consume the accumulator: the final message (the format's own final-text
    /// field if it ever reported one, else the deltas concatenated so far — a
    /// fallback for a stream that dies before its result line) and the spend.
    pub fn finish(self) -> (Option<String>, TokenSpend) {
        let message = self.final_text.or_else(|| (!self.draft.is_empty()).then_some(self.draft));
        (message, self.spend)
    }
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
        assert_eq!(deltas, 2, "only the two agent_response lines carry a delta");
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
        assert!(!acc.on_line(r#"{"event":"step_update","step_update":{"step_type":"tool"}}"#));
        assert!(!acc.on_line(""));
        assert_eq!(acc.draft(), "");
    }

    /// If the stream dies before a `result` line ever arrives, the deltas seen so
    /// far are the best available reply — never silently `None` after real work
    /// was streamed to the human.
    #[test]
    fn missing_result_line_falls_back_to_accumulated_deltas() {
        let mut acc = agy();
        acc.on_line(r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"partial reply"}}"#);
        let (message, spend) = acc.finish();
        assert_eq!(message.as_deref(), Some("partial reply"));
        assert_eq!(spend.input, None, "no usage was ever reported");
    }

    /// The generic `Ndjson` format resolves dotted paths, including a numeric
    /// segment indexing an array — the OpenAI-compatible SSE-as-NDJSON shape a
    /// Custom CLI seat would describe for a provider Hadron has no preset for.
    #[test]
    fn ndjson_format_resolves_dotted_paths_including_array_indices() {
        let mut acc = StreamAccumulator::new(StreamFormat::Ndjson {
            delta: "choices.0.delta.content".to_string(),
            final_text: Some("choices.0.message.content".to_string()),
            usage_input: Some("usage.prompt_tokens".to_string()),
            usage_output: Some("usage.completion_tokens".to_string()),
            usage_cache_read: None,
            usage_cache_write: None,
        });
        assert!(acc.on_line(r#"{"choices":[{"delta":{"content":"Hel"}}]}"#));
        assert!(acc.on_line(r#"{"choices":[{"delta":{"content":"lo"}}]}"#));
        assert!(!acc.on_line(r#"{"choices":[{"delta":{}}]}"#), "no content key at all: no delta");
        assert!(!acc.on_line(
            r#"{"choices":[{"message":{"content":"Hello"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#
        ), "this line carries the final text + usage, not a delta");

        let (message, spend) = acc.finish();
        assert_eq!(message.as_deref(), Some("Hello"), "final_text path wins over the accumulated draft");
        assert_eq!(spend.input, Some(5));
        assert_eq!(spend.output, Some(2));
        assert_eq!(spend.cache_read, None, "path not configured — never a guessed 0");
    }

    /// No `final_text` path configured (or it never resolves) falls back to
    /// whatever deltas were accumulated — same fallback as the agy format.
    #[test]
    fn ndjson_format_without_a_final_text_path_falls_back_to_deltas() {
        let mut acc = StreamAccumulator::new(StreamFormat::Ndjson {
            delta: "delta".to_string(),
            final_text: None,
            usage_input: None,
            usage_output: None,
            usage_cache_read: None,
            usage_cache_write: None,
        });
        acc.on_line(r#"{"delta":"hi there"}"#);
        let (message, _) = acc.finish();
        assert_eq!(message.as_deref(), Some("hi there"));
    }
}
