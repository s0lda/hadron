use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Mode, Projection, QuarkId, TurnOutcome};

use crate::adapter::runner::{CliInvocation, CliRunner};
use crate::quark::Quark;

/// Translate the resolved permission mode into `claude` CLI posture flags.
///
/// The mapping is probed against `claude 2.1.207` (headless `-p`), where tools
/// are binary per posture and there is no mid-turn approval hook:
/// - **Ask** → `plan`: read-only; the quark proposes and executes nothing.
/// - **Write / Auto** → `acceptEdits` with Bash removed: file edits auto-apply,
///   but no ungated shell. Auto's per-command trust-on-first-use list is *not*
///   expressible against this CLI (no deny signal; `--allowedTools` is additive,
///   not restrictive), so Auto deliberately degrades to Write's safe posture —
///   never `acceptEdits`-with-all-bash. True per-command TOFU needs the Agent
///   SDK `canUseTool` path (see the design doc), out of scope here.
/// - **Bypass** → `bypassPermissions`: everything runs (the orchestrator's
///   standing authority).
fn posture_args(mode: Mode) -> Vec<String> {
    match mode {
        Mode::Ask => vec!["--permission-mode".into(), "plan".into()],
        Mode::Write | Mode::Auto => vec![
            "--permission-mode".into(),
            "acceptEdits".into(),
            "--disallowedTools".into(),
            "Bash".into(),
        ],
        Mode::Bypass => vec!["--permission-mode".into(), "bypassPermissions".into()],
    }
}

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
    /// model is set, the permission posture from the turn's `mode`, adding
    /// `--resume <id>` once a session exists. Prompt on stdin.
    fn invocation(&self, prompt: String, mode: Mode) -> CliInvocation {
        let mut args = vec!["-p".to_string(), "--output-format".to_string(), "json".to_string()];
        if !self.model.is_empty() {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }
        args.extend(posture_args(mode));
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
        let mode = turn.mode;
        let prompt = crate::adapter::prompt::build(&turn);
        let result = self.runner.run(self.invocation(prompt, mode)).await?;

        // Parse the JSON envelope: capture the session id, extract the reply text.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result.stdout) {
            if let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) {
                self.session = Some(sid.to_string());
            }

            // Usage: the envelope reports `input_tokens`/`output_tokens` (there is
            // no `total_tokens`); sum them for the energy ledger.
            let used_tokens = v.get("usage")
                .map(|u| {
                    let field = |k| u.get(k).and_then(|t| t.as_u64()).unwrap_or(0);
                    (field("input_tokens") + field("output_tokens")) as u32
                })
                .unwrap_or(0);

            // `permission_denials` (top-level array) is where a mid-turn denial
            // would surface — but headless `-p` never populates it (every posture
            // either runs the tool or lacks it entirely). It is the hook the Agent
            // SDK `canUseTool` upgrade will use to drive real per-command gating.

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
        projection_mode(task, Mode::default())
    }

    fn projection_mode(task: &str, mode: Mode) -> Projection {
        Projection {
            task: task.into(),
            invariants: String::new(),
            available_invariants: vec![],
            nucleus_digest: String::new(),
            roster: vec![],
            field_window: vec![],
            git_diff: String::new(),
            mode,
        }
    }

    #[test]
    fn posture_maps_each_mode() {
        assert_eq!(posture_args(Mode::Ask), vec!["--permission-mode", "plan"]);
        let edits_no_bash =
            vec!["--permission-mode", "acceptEdits", "--disallowedTools", "Bash"];
        assert_eq!(posture_args(Mode::Write), edits_no_bash);
        assert_eq!(posture_args(Mode::Auto), edits_no_bash, "Auto degrades to Write's safe posture");
        assert_eq!(posture_args(Mode::Bypass), vec!["--permission-mode", "bypassPermissions"]);
    }

    #[tokio::test]
    async fn invocation_carries_the_turn_posture() {
        let runner = FakeRunner::with_stdout(vec![
            r#"{"session_id":"s","result":"proposed","usage":{}}"#,
            r#"{"session_id":"s","result":"done","usage":{}}"#,
        ]);
        let mut q = ClaudeQuark::new(QuarkId::new("claude"), Flavor::Orchestrator, "", runner);
        q.excite(projection_mode("plan it", Mode::Ask)).await.unwrap();
        q.excite(projection_mode("do it", Mode::Bypass)).await.unwrap();
        let recorded = q.runner.recorded.lock().unwrap();
        // Ask ran read-only; Bypass ran full-auto.
        let has = |i: usize, seq: &[&str]| recorded[i].args.windows(seq.len()).any(|w| w == seq);
        assert!(has(0, &["--permission-mode", "plan"]), "Ask → plan");
        assert!(has(1, &["--permission-mode", "bypassPermissions"]), "Bypass → bypassPermissions");
    }

    #[tokio::test]
    async fn first_turn_starts_session_then_resumes() {
        let runner = FakeRunner::with_stdout(vec![
            r#"{"session_id":"sess-1","result":"hello @worker","usage":{"input_tokens":40,"output_tokens":2}}"#,
            r#"{"session_id":"sess-1","result":"all done","usage":{"input_tokens":10,"output_tokens":2}}"#,
        ]);
        let mut q = ClaudeQuark::new(QuarkId::new("claude"), Flavor::Orchestrator, "opus-4.8", runner);

        let o1 = q.excite(projection("start")).await.unwrap();
        assert_eq!(o1.message.as_deref(), Some("hello @worker"));
        assert_eq!(o1.used_tokens, 42, "input+output tokens summed");

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
