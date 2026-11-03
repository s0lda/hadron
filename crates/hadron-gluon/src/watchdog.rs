//! Auto-diagnostic loop watchdog for quark tool-call and streaming loops.
//!
//! Detects when a quark gets trapped in a repetitive loop (e.g. invoking the exact same
//! tool call with identical arguments or repeating the exact same failing step $\ge 4$
//! consecutive times) and produces an actionable diagnostic summary so the adapter can
//! break the loop gracefully instead of burning energy until the turn ceiling.

/// Default number of consecutive identical tool/step signatures before [`LoopWatchdog`] fires.
pub const DEFAULT_LOOP_THRESHOLD: usize = 4;

#[derive(Debug, Clone)]
pub struct LoopWatchdog {
    threshold: usize,
    last_signature: Option<String>,
    consecutive_count: usize,
}

impl Default for LoopWatchdog {
    fn default() -> Self {
        Self::new(DEFAULT_LOOP_THRESHOLD)
    }
}

impl LoopWatchdog {
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold: threshold.max(2),
            last_signature: None,
            consecutive_count: 0,
        }
    }

    /// Record a tool-call or step signature and return `Some(diagnostic)` if the quark
    /// has repeated the exact same non-trivial signature at least `self.threshold` times
    /// in a row.
    pub fn observe(&mut self, signature: &str) -> Option<String> {
        let sig = signature.trim();
        if sig.is_empty()
            || sig == "thinking"
            || sig.starts_with("thinking:")
            || sig == "reading the message"
            || sig == "speaking"
        {
            return None;
        }

        if self.last_signature.as_deref() == Some(sig) {
            self.consecutive_count += 1;
        } else {
            self.last_signature = Some(sig.to_string());
            self.consecutive_count = 1;
        }

        if self.consecutive_count >= self.threshold {
            Some(format!(
                "LoopWatchdog auto-diagnostic: detected {} consecutive identical operations (`{}`). \
                 Aborting repetitive loop to prevent energy exhaustion.",
                self.consecutive_count, sig
            ))
        } else {
            None
        }
    }

    pub fn consecutive_count(&self) -> usize {
        self.consecutive_count
    }

    /// Extract a canonical tool-invocation signature from a streaming CLI JSON line
    /// (`agy --output-format stream-json` or Claude/OpenAI NDJSON tool step).
    /// Returns `Some("tool_name:args")` only on active tool invocation steps so
    /// `ACTIVE -> DONE` transitions for the same step are never double-counted.
    pub fn extract_stream_tool_signature(line: &str) -> Option<String> {
        let val: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        if let Some(step) = val.get("step_update") {
            let state = step.get("state").and_then(|v| v.as_str()).unwrap_or("ACTIVE");
            if state != "ACTIVE" {
                return None;
            }
            let tool_name = step.get("tool_name").and_then(|v| v.as_str())?;
            let args = step
                .get("tool_input")
                .or_else(|| step.get("arguments"))
                .or_else(|| step.get("command"))
                .map(|v| v.to_string())
                .unwrap_or_default();
            if args.is_empty() {
                return None;
            }
            return Some(format!("{tool_name}:{args}"));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_watchdog_triggers_on_repeated_identical_tool_signatures() {
        let mut watchdog = LoopWatchdog::default();
        assert!(watchdog.observe("read_file:{\"path\":\"src/lib.rs\"}").is_none());
        assert!(watchdog.observe("read_file:{\"path\":\"src/lib.rs\"}").is_none());
        assert!(watchdog.observe("read_file:{\"path\":\"src/lib.rs\"}").is_none());
        let diag = watchdog
            .observe("read_file:{\"path\":\"src/lib.rs\"}")
            .expect("4th identical signature should trigger diagnostic");
        assert!(diag.contains("LoopWatchdog auto-diagnostic"));
        assert!(diag.contains("4 consecutive identical operations"));
    }

    #[test]
    fn loop_watchdog_resets_when_signature_changes_and_ignores_thinking() {
        let mut watchdog = LoopWatchdog::default();
        assert!(watchdog.observe("exec:{\"cmd\":\"cargo check\"}").is_none());
        assert!(watchdog.observe("exec:{\"cmd\":\"cargo check\"}").is_none());
        assert!(watchdog.observe("thinking").is_none());
        assert_eq!(watchdog.consecutive_count(), 2);
        assert!(watchdog.observe("edit_file:{\"path\":\"src/main.rs\"}").is_none());
        assert_eq!(watchdog.consecutive_count(), 1);
    }
}
