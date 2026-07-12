use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Mode, Projection, QuarkId, TurnOutcome};

use crate::adapter::runner::{reply_to_outcome, CliInvocation, CliRunner};
use crate::quark::Quark;

/// Translate the resolved permission mode into `agy` CLI posture flags, mirroring
/// the claude mapping: Ask→read-only `plan`, Write/Auto→`accept-edits` (edits
/// auto, no raw shell-bypass), Bypass→`--dangerously-skip-permissions`.
///
/// NEEDS LIVE VALIDATION: unlike claude, agy's headless flag parsing and its
/// display-name model ids (`agy models`) were not confirmed against a real turn
/// this session — a naive `--mode plan` invocation confused the parser. The
/// mapping is unit-tested as argv, but verify the live invocation shape before
/// trusting agy gating (see the design doc's "out of scope").
fn posture_args(mode: Mode) -> Vec<String> {
    match mode {
        Mode::Ask => vec!["--mode".into(), "plan".into()],
        Mode::Write | Mode::Auto => vec!["--mode".into(), "accept-edits".into()],
        Mode::Bypass => vec!["--dangerously-skip-permissions".into()],
    }
}

/// A quark backed by the Antigravity CLI (`agy`). One-shot per turn in v1:
/// Antigravity cannot emit structured JSON, so the Markdown reply on stdout IS
/// the message. Each turn is self-contained (the prompt already carries the
/// recent field + diff), so no session state is threaded in v1.
pub struct AgyQuark<R: CliRunner> {
    id: QuarkId,
    flavor: Flavor,
    /// The model to run, a display name as `agy models` prints it, e.g.
    /// "Gemini 3.1 Pro (High)". Empty → the CLI's default.
    model: String,
    runner: R,
}

impl<R: CliRunner> AgyQuark<R> {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, runner: R) -> Self {
        AgyQuark { id, flavor, model: model.into(), runner }
    }

    /// `agy --print <prompt>` (one-shot headless — the prompt is the **argument**
    /// to `--print`; agy ignores stdin in print mode), `--model <model>` when set,
    /// and the permission posture from the turn's `mode`. Markdown on stdout.
    ///
    /// Verified live against `agy 1.1.1`: prompt-on-stdin is silently ignored
    /// (the model answers a default prompt); the prompt must ride as an arg.
    fn invocation(&self, prompt: String, mode: Mode) -> CliInvocation {
        let mut args = vec!["--print".to_string(), prompt];
        if !self.model.is_empty() {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }
        args.extend(posture_args(mode));
        CliInvocation { program: "agy".to_string(), args, stdin: String::new() }
    }
}

#[async_trait]
impl<R: CliRunner> Quark for AgyQuark<R> {
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
        Ok(reply_to_outcome(&result))
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
        assert_eq!(posture_args(Mode::Ask), vec!["--mode", "plan"]);
        assert_eq!(posture_args(Mode::Write), vec!["--mode", "accept-edits"]);
        assert_eq!(posture_args(Mode::Auto), vec!["--mode", "accept-edits"]);
        assert_eq!(posture_args(Mode::Bypass), vec!["--dangerously-skip-permissions"]);
    }

    #[tokio::test]
    async fn agy_runs_print_mode_and_maps_reply() {
        let runner = FakeRunner::with_stdout(vec!["UI complete. @claude back to you."]);
        // Display-name model id, as `agy models` reports them.
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "Gemini 3.1 Pro (High)", runner);

        let o = q.excite(projection("build the UI")).await.unwrap();
        assert_eq!(o.message.as_deref(), Some("UI complete. @claude back to you."));

        let recorded = q.runner.recorded.lock().unwrap();
        assert_eq!(recorded[0].program, "agy");
        assert!(recorded[0].args.iter().any(|a| a == "--print"));
        assert!(recorded[0].args.iter().any(|a| a == "--model"));
        assert!(recorded[0].args.iter().any(|a| a == "Gemini 3.1 Pro (High)"));
        // The prompt (with the handoff reminder) rides as the --print argument,
        // not stdin (agy ignores stdin in print mode). It is args[1], right after
        // "--print", and stdin is empty.
        assert_eq!(recorded[0].args[0], "--print");
        assert!(recorded[0].args[1].contains("# How to respond"));
        assert!(recorded[0].stdin.is_empty());
    }

    #[tokio::test]
    async fn invocation_carries_the_turn_posture() {
        let runner = FakeRunner::with_stdout(vec!["proposed", "done"]);
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "", runner);
        q.excite(projection_mode("plan it", Mode::Ask)).await.unwrap();
        q.excite(projection_mode("do it", Mode::Bypass)).await.unwrap();
        let recorded = q.runner.recorded.lock().unwrap();
        let has = |i: usize, seq: &[&str]| recorded[i].args.windows(seq.len()).any(|w| w == seq);
        assert!(has(0, &["--mode", "plan"]), "Ask → plan");
        assert!(recorded[1].args.iter().any(|a| a == "--dangerously-skip-permissions"), "Bypass → skip");
    }
}
