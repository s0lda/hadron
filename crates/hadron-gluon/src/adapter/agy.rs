use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

use crate::adapter::runner::{reply_to_outcome, CliInvocation, CliRunner};
use crate::quark::Quark;

/// A quark backed by the Antigravity CLI (`agy`). One-shot per turn in v1:
/// Antigravity cannot emit structured JSON, so the Markdown reply on stdout IS
/// the message. Each turn is self-contained (the prompt already carries the
/// recent field + diff), so no session state is threaded in v1.
pub struct AgyQuark<R: CliRunner> {
    id: QuarkId,
    flavor: Flavor,
    /// The model to run, e.g. "gemini-3-pro". Empty → the CLI's default.
    model: String,
    runner: R,
}

impl<R: CliRunner> AgyQuark<R> {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, runner: R) -> Self {
        AgyQuark { id, flavor, model: model.into(), runner }
    }

    /// `agy --print` (one-shot headless), `--model <model>` when set, prompt on
    /// stdin, Markdown on stdout.
    ///
    /// NOTE: exact flag must be verified against the installed CLI version.
    fn invocation(&self, prompt: String) -> CliInvocation {
        let mut args = vec!["--print".to_string()];
        if !self.model.is_empty() {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }
        CliInvocation { program: "agy".to_string(), args, stdin: prompt }
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
        let prompt = crate::adapter::prompt::build(&turn);
        let result = self.runner.run(self.invocation(prompt)).await?;
        Ok(reply_to_outcome(&result))
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
            mode: hadron_lattice::Mode::default(),
        }
    }

    #[tokio::test]
    async fn agy_runs_print_mode_and_maps_reply() {
        let runner = FakeRunner::with_stdout(vec!["UI complete. @claude back to you."]);
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "gemini-3-pro", runner);

        let o = q.excite(projection("build the UI")).await.unwrap();
        assert_eq!(o.message.as_deref(), Some("UI complete. @claude back to you."));

        let recorded = q.runner.recorded.lock().unwrap();
        assert_eq!(recorded[0].program, "agy");
        assert!(recorded[0].args.iter().any(|a| a == "--print"));
        assert!(recorded[0].args.iter().any(|a| a == "--model"));
        assert!(recorded[0].args.iter().any(|a| a == "gemini-3-pro"));
        // The prompt (with the handoff reminder) reached stdin.
        assert!(recorded[0].stdin.contains("# How to respond"));
    }
}
