use async_trait::async_trait;
use hadron_lattice::{
    Actor, EnergyState, Event, Flavor, Kind, Mode, Projection, QuarkId, TurnOutcome,
};
use std::path::PathBuf;

use crate::adapter::runner::{reply_to_outcome, CliInvocation, CliRunner};
use crate::quark::Quark;

/// Linux's hard cap on a **single** argv element (`MAX_ARG_STRLEN` = 32 pages =
/// 128 KiB, `include/uapi/linux/binfmts.h`). It is NOT the same as `ARG_MAX`, and
/// it cannot be raised with `ulimit`. `execve` rejects an over-long element with
/// E2BIG *before the program starts*.
///
/// This matters here and nowhere else: `agy` has no stdin in print mode and no
/// `--prompt-file`, so the whole prompt must ride as one argv element. (`claude`
/// takes its prompt on stdin, which has no such limit — which is exactly why agy
/// was the only quark that broke.)
const MAX_ARG_STRLEN: usize = 128 * 1024;

/// The budget the adapter actually enforces: three quarters of the kernel's hard
/// limit (96 KiB), leaving headroom for the other argv elements, the environment
/// block, and multi-byte slack. A prompt over this is truncated rather than handed
/// to a doomed `execve`. Derived from [`MAX_ARG_STRLEN`] on purpose — the headroom
/// should stay visibly tied to the limit it is hedging against.
const SAFE_ARG_BYTES: usize = MAX_ARG_STRLEN / 4 * 3;

/// Inserted where the transcript was cut, so a truncated quark knows its context is
/// incomplete instead of confabulating over the gap.
const TRUNCATION_MARKER: &str = "[transcript truncated: older field events were dropped to fit the CLI's argument limit]";

/// Last-resort guard on the prompt handed to `agy --print`.
///
/// The projection already caps its field window ([`crate::engine::FIELD_WINDOW_BUDGET_BYTES`]),
/// but that is *policy* — it can be raised, and a single pathological message or a
/// large `git_diff` can still overshoot. This is the *safety net*: it guarantees the
/// argv element we are about to hand `execve` is one `execve` will accept.
///
/// It truncates by dropping the **oldest** field-window events and re-rendering, so
/// the identity / task / authority / handoff sections — the quark's actual
/// instructions — survive by construction. If dropping the entire field window is
/// still not enough (a colossal `git_diff` or task), the diff goes too; a prompt with
/// no instructions is worse than a prompt with no transcript.
fn fit_prompt(projection: &Projection, self_id: &QuarkId) -> String {
    let render = |p: &Projection| crate::adapter::prompt::build(p, self_id);

    let prompt = render(projection);
    if prompt.len() <= SAFE_ARG_BYTES {
        return prompt;
    }

    let mut p = projection.clone();
    // Drop the oldest events one at a time until it fits. The marker rides as a
    // synthetic leading event so it renders inside the transcript, right where the
    // cut happened.
    while !p.field_window.is_empty() {
        p.field_window.remove(0);
        let mut probe = p.clone();
        probe.field_window.insert(
            0,
            Event::new(Actor::Gluon, None, Kind::Message { body: TRUNCATION_MARKER.to_string() }),
        );
        let out = render(&probe);
        if out.len() <= SAFE_ARG_BYTES {
            return out;
        }
    }

    // The field window is gone and it still does not fit: the diff is the only other
    // unbounded section. Drop it too, and say so.
    p.git_diff = String::new();
    p.field_window = vec![Event::new(
        Actor::Gluon,
        None,
        Kind::Message { body: TRUNCATION_MARKER.to_string() },
    )];
    let out = render(&p);
    if out.len() <= SAFE_ARG_BYTES {
        return out;
    }

    // Nothing left to drop — the *task itself* is enormous. Hard-cut on a char
    // boundary rather than hand `execve` an argument it will reject outright.
    let mut cut = SAFE_ARG_BYTES.saturating_sub(TRUNCATION_MARKER.len() + 1);
    while cut > 0 && !out.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n{TRUNCATION_MARKER}", &out[..cut])
}

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

/// `agy --print` gives up on its own turn after **5 minutes** by default
/// (`--print-timeout`, default `5m0s`, per `agy --help`). That default is what put
/// agy in `error` after exactly 5m02s on a real turn: not a crash, not a quota wall
/// — the CLI abandoned a turn that was still working.
///
/// A quark's turn is routinely longer than that, so the timeout is raised to sit
/// just inside the engine's own 30-minute turn deadline. The engine stays the outer
/// bound: agy should give up *because Hadron said so*, not on a default nobody chose.
const PRINT_TIMEOUT: &str = "29m";

/// Strip the transcript from a projection bound for a resident conversation.
///
/// The identity, task, authority and diff sections stay: they are *this turn's*
/// instruction, and they change every turn. Only the field window goes, because the
/// resumed conversation already contains it.
fn without_field_window(mut projection: Projection) -> Projection {
    projection.field_window = Vec::new();
    projection
}

/// A quark backed by the Antigravity CLI (`agy`). Antigravity cannot emit structured
/// JSON, so the Markdown reply on stdout IS the message.
///
/// The session is **resident**: the first turn opens a conversation, every turn after
/// resumes it with `--continue` and sends only the new instruction rather than the
/// whole field again. Verified live — a second `--continue` turn recalled a codeword
/// from the first with no history re-sent. This is what stops cost growing
/// quadratically with the conversation, and it is why we no longer have to throw away
/// the human's transcript to fit an argv limit.
pub struct AgyQuark<R: CliRunner> {
    id: QuarkId,
    flavor: Flavor,
    /// The `@mention` name (see [`Quark::display_name`]); `None` = id-only.
    display_name: Option<String>,
    /// The model to run, a display name as `agy models` prints it, e.g.
    /// "Gemini 3.1 Pro (High)". Empty → the CLI's default.
    model: String,
    /// Whether this quark already has a conversation for `--continue` to resume.
    ///
    /// In-memory on purpose: a daemon restart resets it, and the next turn re-sends
    /// the full projection. Re-sending context is merely expensive; resuming a
    /// conversation that does not exist would silently answer the wrong question.
    resident: bool,
    runner: R,
}

impl<R: CliRunner> AgyQuark<R> {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, runner: R) -> Self {
        AgyQuark { id, flavor, display_name: None, model: model.into(), resident: false, runner }
    }

    /// Set the `@mention` display name (from the resolved team config).
    pub fn with_display_name(mut self, name: Option<String>) -> Self {
        self.display_name = name;
        self
    }

    /// `agy --print <prompt>` (one-shot headless — the prompt is the **argument**
    /// to `--print`; agy ignores stdin in print mode), `--model <model>` when set,
    /// and the permission posture from the turn's `mode`. Markdown on stdout.
    ///
    /// Verified live against `agy 1.1.1`: prompt-on-stdin is silently ignored
    /// (the model answers a default prompt); the prompt must ride as an arg.
    ///
    /// `agy` takes no directory flag either, so the working directory rides on the
    /// invocation and `ProcessRunner` applies it — no new argv surface.
    fn invocation(&self, prompt: String, mode: Mode, cwd: PathBuf) -> CliInvocation {
        let mut args = Vec::new();
        // `--continue` resumes the most recent conversation *in this working directory*,
        // which is why it can only be trusted once the quark has one of its own.
        if self.resident {
            args.push("--continue".to_string());
        }
        // The prompt is the **value** of `--print`, not a positional. Get this wrong and
        // agy answers the next flag as if it were the question — observed live: it
        // replied "I'm not sure what you mean by --dangerously-skip-permissions".
        args.push("--print".to_string());
        args.push(prompt);
        args.push("--print-timeout".to_string());
        args.push(PRINT_TIMEOUT.to_string());
        if !self.model.is_empty() {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }
        args.extend(posture_args(mode));
        CliInvocation { program: "agy".to_string(), args, stdin: String::new(), cwd }
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
    fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }

    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let mode = turn.mode;
        let cwd = turn.cwd.clone();
        // A resumed conversation already holds everything we sent before, so re-sending
        // the field window would pay for the same history twice — the whole point of
        // going resident. Send the new instruction only; agy still has the rest.
        let turn = if self.resident { without_field_window(turn) } else { turn };
        // NOT `prompt::build` directly: the prompt is one argv element, and `execve`
        // rejects an over-long one with E2BIG before agy ever starts. `fit_prompt` is
        // the guard that makes the invocation executable no matter how big the field.
        let prompt = fit_prompt(&turn, &self.id);
        let result = self.runner.run(self.invocation(prompt, mode, cwd)).await?;
        // Only a turn that actually ran leaves a conversation behind for `--continue`.
        self.resident = true;
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
        projection_in(task, mode, PathBuf::from("/tmp/hadron-test-cwd"))
    }

    fn projection_in(task: &str, mode: Mode, cwd: PathBuf) -> Projection {
        Projection {
            isolated: true,
            task: task.into(),
            invariants: String::new(),
            available_invariants: vec![],
            nucleus_digest: String::new(),
            live_activities: vec![], roster: vec![],
            field_window: vec![],
            field_truncated: false,
            memory: String::new(),
            memory_path: std::path::PathBuf::new(),
            memory_truncated: false,
            memory_notes_dir: std::path::PathBuf::new(),
            git_diff: String::new(),
            cwd,
            mode,
        }
    }

    /// A projection whose field window is `n` messages of `body_bytes` each — a
    /// long-lived swarm's field, which is what actually blew up in production.
    fn huge_projection(n: usize, body_bytes: usize) -> Projection {
        let mut p = projection_mode("summarise the work so far", Mode::Bypass);
        p.field_window = (0..n)
            .map(|i| {
                Event::new(
                    Actor::Human,
                    None,
                    Kind::Message { body: format!("event{i} {}", "x".repeat(body_bytes)) },
                )
            })
            .collect();
        p
    }

    /// **THE discriminating test.** `agy` has no stdin and no `--prompt-file`: the
    /// prompt is one argv element, and Linux caps a single argv element at
    /// `MAX_ARG_STRLEN` = 128 KiB. Hand the adapter an oversized projection and the
    /// invocation must STILL be executable — otherwise `execve` returns E2BIG and
    /// the turn dies in under a millisecond with no subprocess ever spawned.
    #[tokio::test]
    async fn agy_never_builds_an_argv_element_that_execve_would_reject() {
        let runner = FakeRunner::with_stdout(vec!["ok"]);
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "Gemini 3.1 Pro (High)", runner);

        // ~400 KB of field — over 3× the kernel's hard limit for one argv element.
        let p = huge_projection(200, 2000);
        assert!(
            crate::adapter::prompt::build(&p, &QuarkId::new("agy")).len() > MAX_ARG_STRLEN,
            "precondition: the un-guarded prompt really would be rejected by execve"
        );

        q.excite(p).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        for (i, arg) in recorded[0].args.iter().enumerate() {
            assert!(
                arg.len() <= SAFE_ARG_BYTES,
                "argv[{i}] is {} bytes, over the {SAFE_ARG_BYTES}-byte safe budget \
                 (kernel hard limit {MAX_ARG_STRLEN})",
                arg.len()
            );
        }
    }

    /// Truncation must be *honest* and must cut the OLDEST context. The identity,
    /// task, authority and handoff sections are the quark's instructions — losing
    /// them silently turns a truncated turn into a confabulated one.
    #[tokio::test]
    async fn truncation_drops_the_oldest_field_and_says_so() {
        let runner = FakeRunner::with_stdout(vec!["ok"]);
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "", runner);

        let mut p = huge_projection(200, 2000);
        // Bookend the window: the oldest event must go, the newest must survive.
        p.field_window.first_mut().unwrap().kind =
            Kind::Message { body: format!("OLDEST-CANARY {}", "x".repeat(2000)) };
        p.field_window.last_mut().unwrap().kind =
            Kind::Message { body: "NEWEST-CANARY the thing I just asked for".into() };

        q.excite(p).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        let prompt = &recorded[0].args[1];
        assert!(prompt.contains("# Who you are"), "identity survives");
        assert!(prompt.contains("summarise the work so far"), "the task survives");
        assert!(prompt.contains("# Your authority this turn"), "authority survives");
        assert!(prompt.contains("# How to respond"), "the handoff reminder survives");
        assert!(prompt.contains("NEWEST-CANARY"), "the most recent field survives");
        assert!(!prompt.contains("OLDEST-CANARY"), "the oldest field is what gets dropped");
        assert!(
            prompt.contains(TRUNCATION_MARKER),
            "and the quark is TOLD its transcript was cut, so it does not confabulate the gap"
        );
    }

    /// The guard is a safety net, not a tax: a normal-sized prompt is passed through
    /// byte-for-byte, with no marker and nothing dropped.
    #[tokio::test]
    async fn a_normal_prompt_is_not_touched() {
        let runner = FakeRunner::with_stdout(vec!["ok"]);
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "", runner);
        let p = huge_projection(3, 100);
        let want = crate::adapter::prompt::build(&p, &QuarkId::new("agy"));

        q.excite(p).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        assert_eq!(&recorded[0].args[1], &want, "passed through untouched");
        assert!(!recorded[0].args[1].contains(TRUNCATION_MARKER));
    }

    /// Layer 3b of the cwd chain, agy side.
    #[tokio::test]
    async fn agy_invocation_carries_the_projection_cwd() {
        let runner = FakeRunner::with_stdout(vec!["ok"]);
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "", runner);
        let wt = PathBuf::from("/repo/.hadron/trees/agy");
        q.excite(projection_in("go", Mode::Write, wt.clone())).await.unwrap();
        assert_eq!(q.runner.recorded.lock().unwrap()[0].cwd, wt);
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

    /// `agy --print` abandons its own turn after 5 minutes by default, which is how a
    /// perfectly healthy quark landed in `error` after 5m02s. Every invocation must
    /// carry an explicit timeout, or that default silently comes back.
    #[tokio::test]
    async fn every_turn_overrides_agys_five_minute_print_timeout() {
        let runner = FakeRunner::with_stdout(vec!["ok"]);
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "", runner);

        q.excite(projection_mode("work", Mode::Bypass)).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        assert!(
            recorded[0].args.windows(2).any(|w| w == ["--print-timeout", PRINT_TIMEOUT]),
            "no --print-timeout: agy will give up after 5 minutes. args: {:?}",
            recorded[0].args
        );
    }

    /// **The discriminating test for resident sessions.** The first turn opens a
    /// conversation and carries the transcript; every turn after resumes it and must
    /// send only the new instruction. Re-sending the field into a resumed conversation
    /// pays for the same history twice — which is the entire cost problem going
    /// resident exists to solve.
    #[tokio::test]
    async fn a_resumed_turn_continues_the_session_and_stops_resending_the_field() {
        let runner = FakeRunner::with_stdout(vec!["one", "two"]);
        let mut q = AgyQuark::new(QuarkId::new("agy"), Flavor::Worker, "", runner);

        let mut first = projection_mode("first task", Mode::Bypass);
        first.field_window = vec![Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "MEMORABLE-TRANSCRIPT-LINE".into() },
        )];
        let mut second = first.clone();
        second.task = "second task".into();

        q.excite(first).await.unwrap();
        q.excite(second).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        assert!(!recorded[0].args.iter().any(|a| a == "--continue"), "first turn opens the session");
        assert!(recorded[0].args[1].contains("MEMORABLE-TRANSCRIPT-LINE"), "first turn carries the field");

        assert!(recorded[1].args.iter().any(|a| a == "--continue"), "second turn resumes it");
        let resumed_prompt = &recorded[1].args[2];
        assert!(resumed_prompt.contains("second task"), "the new instruction still rides");
        assert!(
            !resumed_prompt.contains("MEMORABLE-TRANSCRIPT-LINE"),
            "a resumed turn re-sent the transcript agy already has"
        );
    }
}
