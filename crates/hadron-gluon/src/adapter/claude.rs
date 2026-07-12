use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

use crate::adapter::runner::{CliInvocation, CliRunner};
use crate::quark::Quark;

/// A quark backed by the Claude Code CLI (`claude`). Carries a resumable session
/// so multi-turn context persists: the first turn starts a session and captures
/// its id from the CLI's JSON envelope; later turns pass `--resume <id>`.
pub struct ClaudeQuark<R: CliRunner> {
    id: QuarkId,
    flavor: Flavor,
    /// The model to run, e.g. "opus-4.8". Empty → let the CLI pick its default.
    model: String,
    runner: R,
    session: Option<String>,
}

impl<R: CliRunner> ClaudeQuark<R> {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, runner: R) -> Self {
        ClaudeQuark { id, flavor, model: model.into(), runner, session: None }
    }

    /// Build this turn's invocation. `claude -p --output-format json` (headless,
    /// JSON envelope so we can recover the session id), `--model <model>` when a
    /// model is set, adding `--resume <id>` once a session exists. Prompt on stdin.
    ///
    /// NOTE: exact flags must be verified against the installed CLI version — this
    /// is the intended point of adjustment (see Plan 3).
    fn invocation(&self, prompt: String) -> CliInvocation {
        let mut args = vec!["-p".to_string(), "--output-format".to_string(), "json".to_string()];
        if !self.model.is_empty() {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }
        if let Some(sid) = &self.session {
            args.push("--resume".to_string());
            args.push(sid.clone());
        }
        CliInvocation { program: "claude".to_string(), args, stdin: prompt }
    }
}

#[async_trait]
impl<R: CliRunner> Quark for ClaudeQuark<R> {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        self.flavor.clone()
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }

    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let prompt = crate::adapter::prompt::build(&turn);
        let result = self.runner.run(self.invocation(prompt)).await?;

        // Parse the JSON envelope: capture the session id, extract the reply text.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result.stdout) {
            if let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) {
                self.session = Some(sid.to_string());
            }

            // Extract usage if available
            let used_tokens = v.get("usage")
                .and_then(|u| u.get("total_tokens"))
                .and_then(|t| t.as_u64())
                .map(|t| t as u32)
                .unwrap_or(0);

            if let Some(text) = v.get("result").and_then(|s| s.as_str()) {
                let t = text.trim();
                return Ok(TurnOutcome {
                    message: if t.is_empty() { None } else { Some(t.to_string()) },
                    used_tokens,
                    permission: None,
                });
            }
        }

        // Fallback: the CLI did not return the expected JSON — treat stdout as raw.
        Ok(crate::adapter::runner::reply_to_outcome(&result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::runner::FakeRunner;

    fn projection(task: &str) -> Projection {
        Projection {
            task: task.into(),
            invariants: String::new(),
            available_invariants: vec![],
            nucleus_digest: String::new(),
            roster: vec![],
            field_window: vec![],
            git_diff: String::new(),
        }
    }

    #[tokio::test]
    async fn first_turn_starts_session_then_resumes() {
        let runner = FakeRunner::with_stdout(vec![
            r#"{"session_id":"sess-1","result":"hello @worker","usage":{"total_tokens":42}}"#,
            r#"{"session_id":"sess-1","result":"all done","usage":{"total_tokens":12}}"#,
        ]);
        let mut q = ClaudeQuark::new(QuarkId::new("claude"), Flavor::Orchestrator, "opus-4.8", runner);

        let o1 = q.excite(projection("start")).await.unwrap();
        assert_eq!(o1.message.as_deref(), Some("hello @worker"));
        assert_eq!(o1.used_tokens, 42);

        let o2 = q.excite(projection("continue")).await.unwrap();
        assert_eq!(o2.message.as_deref(), Some("all done"));
        assert_eq!(o2.used_tokens, 12);

        // Turn 1 had no --resume; turn 2 resumed the captured session. Both carry
        // the model flag.
        let recorded = q.runner.recorded.lock().unwrap();
        assert!(recorded[0].args.iter().any(|a| a == "--model"));
        assert!(recorded[0].args.iter().any(|a| a == "opus-4.8"));
        assert!(!recorded[0].args.iter().any(|a| a == "--resume"));
        assert!(recorded[1].args.iter().any(|a| a == "--resume"));
        assert!(recorded[1].args.iter().any(|a| a == "sess-1"));
    }

    #[tokio::test]
    async fn non_json_stdout_falls_back_to_raw() {
        let runner = FakeRunner::with_stdout(vec!["plain markdown reply"]);
        let mut q = ClaudeQuark::new(QuarkId::new("claude"), Flavor::Worker, "", runner);
        let o = q.excite(projection("x")).await.unwrap();
        assert_eq!(o.message.as_deref(), Some("plain markdown reply"));
        assert_eq!(q.session, None); // no session id to capture
    }
}
