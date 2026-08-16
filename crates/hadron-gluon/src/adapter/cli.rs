use agent_client_protocol::schema::v1::SessionConfigId;
use async_trait::async_trait;
use hadron_lattice::live::{self, Activity, Doing};
use hadron_lattice::{
    Actor, CliSpec, EnergyState, Event, Flavor, Kind, Mode, Projection, PromptChannel, QuarkId,
    ResumeMode, SeatCommands, StreamSpec, TurnOutcome, Usage,
};
use std::path::PathBuf;
use std::time::Instant;

use crate::adapter::acp::{AcpModel, ModelSelector};
use crate::adapter::cli_stream::StreamAccumulator;
use crate::adapter::local::PUBLISH_THROTTLE;
use crate::adapter::runner::{reply_to_outcome, CliInvocation, CliRunner, RedactedEnv};
use crate::quark::Quark;

/// Probe available models from a CLI binary by executing its `model_probe` spec.
pub fn probe_cli_models(spec: &CliSpec) -> anyhow::Result<ModelSelector> {
    let probe_spec = spec
        .model_probe
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no model_probe configured"))?;
    let output = std::process::Command::new(&spec.program)
        .args(&probe_spec.args)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("model probe process exited with status {}", output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut available = Vec::new();
    for line in stdout.lines() {
        let clean = line.trim();
        if clean.is_empty() || clean.contains("Fetching available models") {
            continue;
        }
        let parts: Vec<&str> = clean.split_whitespace().collect();
        if !parts.is_empty() {
            let value = parts[0].to_string();
            let label = if parts.len() > 1 {
                parts[1..].join(" ")
            } else {
                value.clone()
            };
            available.push(AcpModel { value, label });
        }
    }
    Ok(ModelSelector {
        config_id: SessionConfigId::from("model"),
        current: "".to_string(),
        available,
    })
}

/// Linux's hard cap on a **single** argv element (`MAX_ARG_STRLEN` = 32 pages =
/// 128 KiB, `include/uapi/linux/binfmts.h`). It is NOT the same as `ARG_MAX`, and
/// it cannot be raised with `ulimit`. `execve` rejects an over-long element with
/// E2BIG *before the program starts*.
///
/// On Windows, `CreateProcessW` caps the entire command-line string at 32,767 characters
/// (`UNICODE_STRING` buffer limit in the Windows kernel). Exceeding it returns
/// `ERROR_FILENAME_EXCED_RANGE` (os error 206, "The filename or extension is too long.").
///
/// This matters here and nowhere else: `agy` has no stdin in print mode and no
/// `--prompt-file`, so the whole prompt must ride as one argv element. (`claude`
/// takes its prompt on stdin, which has no such limit — which is exactly why agy
/// was the only quark that broke.)
#[cfg(windows)]
pub const MAX_ARG_STRLEN: usize = 32 * 1024;
#[cfg(not(windows))]
pub const MAX_ARG_STRLEN: usize = 128 * 1024;

/// The budget the adapter actually enforces: three quarters of the OS's hard
/// limit (96 KiB on Linux/Unix, 24 KiB on Windows), leaving headroom for the other argv elements, the environment
/// block, and multi-byte slack. A prompt over this is truncated rather than handed
/// to a doomed `execve` or `CreateProcessW`. Derived from [`MAX_ARG_STRLEN`] on purpose — the headroom
/// should stay visibly tied to the limit it is hedging against.
pub const SAFE_ARG_BYTES: usize = MAX_ARG_STRLEN / 4 * 3;

/// Inserted where the transcript was cut, so a truncated quark knows its context is
/// incomplete instead of confabulating over the gap.
pub const TRUNCATION_MARKER: &str = "[transcript truncated: older field events were dropped to fit the CLI's argument limit]";

/// Last-resort guard on the prompt handed to a CLI whose [`CliSpec::argv_guard`] is
/// set or whose [`PromptChannel`] is `Arg` (e.g. `agy --print`).
///
/// The projection already caps its field window ([`crate::engine::FIELD_WINDOW_BUDGET_BYTES`]),
/// but that is *policy* — it can be raised, and a single pathological message or a
/// large `git_diff` can still overshoot. This is the *safety net*: it guarantees the
/// argv element we are about to hand `execve` or `CreateProcessW` is one the OS will accept.
///
/// It truncates by dropping the **oldest** field-window events and re-rendering, so
/// the identity / task / authority / handoff sections — the quark's actual
/// instructions — survive by construction. If dropping the entire field window is
/// still not enough (a colossal `git_diff` or task), the diff goes too; a prompt with
/// no instructions is worse than a prompt with no transcript.
pub fn fit_prompt(projection: &Projection, self_id: &QuarkId) -> String {
    fit_prompt_to_budget(projection, self_id, SAFE_ARG_BYTES)
}

pub fn fit_prompt_to_budget(projection: &Projection, self_id: &QuarkId, max_bytes: usize) -> String {
    let render = |p: &Projection| crate::adapter::prompt::build(p, self_id);

    let prompt = render(projection);
    if prompt.len() <= max_bytes {
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
        if out.len() <= max_bytes {
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
    if out.len() <= max_bytes {
        return out;
    }

    // Nothing left to drop — the *task itself* is enormous. Hard-cut on a char
    // boundary rather than hand `execve`/`CreateProcessW` an argument it will reject outright.
    let mut cut = max_bytes.saturating_sub(TRUNCATION_MARKER.len() + 1);
    while cut > 0 && !out.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n{TRUNCATION_MARKER}", &out[..cut])
}

/// Strip the transcript from a projection bound for a resident conversation.
///
/// The identity, task, authority and diff sections stay: they are *this turn's*
/// instruction, and they change every turn. Only the field window goes, because the
/// resumed conversation already contains it.
fn without_field_window(mut projection: Projection) -> Projection {
    projection.field_window = Vec::new();
    projection
}

/// A quark backed by an arbitrary CLI subprocess, driven by a [`CliSpec`]. Folds
/// `agy.rs`'s hardened, incident-hardened behaviours (the E2BIG `fit_prompt` argv
/// guard, `--continue` resident resume with field-window stripping, per-CLI
/// timeout override, posture flags) onto a config-driven shape instead of a
/// bespoke adapter per vendor — see `docs/superpowers/specs/2026-07-17-custom-cli-transport-design.md` §4.4.
///
/// The session is **resident** exactly as agy's was: once a turn has actually run,
/// `resident` flips true and (if `spec.resume` supports it) every later turn
/// resumes rather than re-sending the whole field.
pub struct CliQuark<R: CliRunner> {
    id: QuarkId,
    flavor: Flavor,
    /// The `@mention` name (see [`Quark::display_name`]); `None` = id-only.
    display_name: Option<String>,
    /// The model to run, in whatever form the CLI's `--model`-equivalent expects.
    /// Empty → the CLI's default (never passed).
    model: String,
    spec: CliSpec,
    /// This quark's `@role` roles (see [`Quark::roles`]); empty = no roles.
    roles: Vec<String>,
    /// Whether this quark is scoped only to its roles (see [`Quark::exclusive`]).
    exclusive: bool,
    /// This quark's per-seat command allow/deny lists (see [`Quark::commands`]).
    commands: SeatCommands,
    /// This seat's resolved secret env — `(name, value)` pairs from
    /// `Seat::resolve_env`. Carried onto every [`CliInvocation`] and NOWHERE else
    /// (never argv, never a log line, never the field/prompt).
    env: RedactedEnv,
    /// Whether this quark already has a conversation for `spec.resume` to resume.
    ///
    /// In-memory on purpose: a daemon restart resets it, and the next turn re-sends
    /// the full projection. Re-sending context is merely expensive; resuming a
    /// conversation that does not exist would silently answer the wrong question.
    resident: bool,
    runner: R,
    energy_limit: Option<u32>,
    deny_skills: Vec<String>,
    /// Where to publish mid-turn draft activity for a `spec.stream` seat. `None` =
    /// nobody is watching (tests, and any quark this daemon is not watching) —
    /// same shape as `adapter::local::LocalQuark::live_dir`. A `spec.stream: None`
    /// seat never reads this: there is nothing to publish until the process exits.
    live_dir: Option<PathBuf>,
}

impl<R: CliRunner> CliQuark<R> {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, spec: CliSpec, runner: R) -> Self {
        CliQuark {
            id,
            flavor,
            display_name: None,
            model: model.into(),
            spec,
            resident: false,
            runner,
            roles: Vec::new(),
            exclusive: false,
            commands: SeatCommands::default(),
            env: RedactedEnv::default(),
            energy_limit: None,
            deny_skills: Vec::new(),
            live_dir: None,
        }
    }

    /// Stream this quark's mid-turn draft into `dir` (see `hadron_lattice::live`).
    /// Only matters for a `spec.stream: Some(_)` seat — a non-streaming CLI seat
    /// has nothing to publish until its process exits either way.
    pub fn watching(mut self, dir: PathBuf) -> Self {
        self.live_dir = Some(dir);
        self
    }

    pub fn with_energy_limit(mut self, limit: Option<u32>) -> Self {
        self.energy_limit = limit;
        self
    }

    /// Set the skill locks.
    pub fn with_deny_skills(mut self, deny_skills: Vec<String>) -> Self {
        self.deny_skills = deny_skills;
        self
    }

    /// Set the `@mention` display name (from the resolved team config).
    pub fn with_display_name(mut self, name: Option<String>) -> Self {
        self.display_name = name;
        self
    }

    /// Set the `@role` roles and exclusivity (from the resolved seat).
    pub fn with_roles(mut self, roles: Vec<String>, exclusive: bool) -> Self {
        self.roles = roles;
        self.exclusive = exclusive;
        self
    }

    /// Set the per-seat command allow/deny lists (from the resolved seat).
    pub fn with_commands(mut self, commands: SeatCommands) -> Self {
        self.commands = commands;
        self
    }

    /// Set this seat's resolved secret env (from `Seat::resolve_env`), carried onto
    /// every invocation's `CliInvocation.env`.
    pub fn with_env(mut self, env: impl Into<RedactedEnv>) -> Self {
        self.env = env.into();
        self
    }

    /// Build the subprocess invocation for one turn: static leading args, a resume
    /// flag once resident, the prompt on whichever channel `spec.prompt` names,
    /// the CLI's own timeout override, the model flag when set and non-empty, and
    /// the posture flags for this turn's `mode`.
    fn invocation(&self, prompt: String, mode: Mode, cwd: PathBuf) -> CliInvocation {
        let mut args = self.spec.args.clone();
        if let Some(stream) = &self.spec.stream {
            args.extend(stream.flags.iter().cloned());
        }
        if self.resident {
            if let ResumeMode::Continue { flag } = &self.spec.resume {
                args.push(flag.clone());
            }
        }

        let mut stdin = String::new();
        match &self.spec.prompt {
            PromptChannel::Stdin => stdin = prompt,
            PromptChannel::Arg { flag: Some(flag) } => {
                args.push(flag.clone());
                args.push(prompt);
            }
            PromptChannel::Arg { flag: None } => args.push(prompt),
        }

        if let Some(timeout) = &self.spec.timeout {
            args.push(timeout.flag.clone());
            args.push(timeout.value.clone());
        }
        if let Some(model_flag) = &self.spec.model_flag {
            if !self.model.is_empty() {
                args.push(model_flag.clone());
                args.push(self.model.clone());
            }
        }
        args.extend(self.spec.posture.for_mode(mode).iter().cloned());

        // The seat's resolved secrets, plus the shared build env so a `cargo` the
        // quark runs itself lands in the main checkout's warm `target/` instead of
        // growing a 37 GB one per worktree (`worktree::shared_build_env`). Seat env
        // is applied last: a seat that deliberately sets one of these wins.
        let mut env = crate::worktree::shared_build_env(&cwd);
        env.extend(self.env.iter().cloned());

        CliInvocation {
            program: self.spec.program.clone(),
            args,
            stdin,
            cwd,
            env: env.into(),
            stream: self.spec.stream.clone(),
        }
    }

    /// The streaming counterpart of a plain `runner.run(...)`: parses stdout line
    /// by line per `stream.format`, throttle-publishing the accumulated draft into
    /// `live_dir` exactly the way `adapter::local::LocalQuark::excite` does for an
    /// HTTP seat (same `PUBLISH_THROTTLE`), and turns the final parsed
    /// `(message, TokenSpend)` into a `TurnOutcome` once the subprocess exits.
    async fn run_streaming(&mut self, inv: CliInvocation, stream: StreamSpec) -> anyhow::Result<TurnOutcome> {
        let dir = self.live_dir.clone();
        let quark_id = self.id.clone();
        let mut acc = StreamAccumulator::new(stream.format);
        let mut last_publish: Option<Instant> = None;
        let mut on_line = |line: &str| {
            if !acc.on_line(line) {
                return;
            }
            let Some(dir) = &dir else { return };
            let due = last_publish.is_none_or(|t: Instant| t.elapsed() >= PUBLISH_THROTTLE);
            if due {
                last_publish = Some(Instant::now());
                // Text once there is any; until then the step/tool the stream last
                // named, so a CLI that reports its work before it reports its words
                // (agy does — see `cli_stream::on_agy_line`) still shows a moving
                // Live card instead of a motionless `working…`.
                let activity = if acc.draft().is_empty() {
                    Activity::new(quark_id.clone(), Doing::Working, acc.status())
                } else {
                    Activity::speaking(quark_id.clone(), acc.draft())
                };
                let _ = live::publish(dir, &activity);
            }
        };
        let run_result = self.runner.run_streaming(inv, &mut on_line).await;
        if let Some(dir) = &self.live_dir {
            let _ = live::clear(dir, &self.id);
        }
        run_result?;
        let (message, spend) = acc.finish();
        Ok(TurnOutcome { message, usage: Usage { spend, ..Default::default() }, ..Default::default() })
    }
}

#[async_trait]
impl<R: CliRunner> Quark for CliQuark<R> {
    /// A chat lane is made stateless by clearing `spec.resume`, not by a parallel flag.
    /// `resident` and `spec.resume` are already read together at both sites that matter —
    /// the resume flag in [`CliQuark::invocation`] and the field-window drop in
    /// [`Quark::excite`] — so `ResumeMode::None` is correct at both through logic that is
    /// already there, and a site added later cannot forget to consult a second bool.
    fn become_chat_lane(&mut self) {
        self.spec.resume = ResumeMode::None;
    }
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        self.flavor.clone()
    }
    fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }
    fn roles(&self) -> Vec<String> {
        self.roles.clone()
    }
    fn exclusive(&self) -> bool {
        self.exclusive
    }
    fn deny_skills(&self) -> Vec<String> {
        self.deny_skills.clone()
    }
    fn commands(&self) -> &SeatCommands {
        &self.commands
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    fn energy_limit(&self) -> Option<u32> {
        self.energy_limit
    }
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let mode = turn.mode;
        let cwd = turn.cwd.clone();
        // A resumed conversation already holds everything we sent before, so re-sending
        // the field window would pay for the same history twice — the whole point of
        // going resident. Send the new instruction only; the CLI still has the rest.
        // Only true when this CLI actually supports resuming (`spec.resume` is not
        // `None`) — a generic CLI with no resume flag never gets to drop context it
        // has no way to recall.
        let resuming = self.resident && !matches!(self.spec.resume, ResumeMode::None);
        let turn = if resuming { without_field_window(turn) } else { turn };
        // The argv guard only applies when the spec says the prompt rides as one
        // argv element with no stdin fallback (`execve` on Linux rejects an over-long one
        // with E2BIG; `CreateProcessW` on Windows returns error 206 before the CLI ever starts).
        // Otherwise build the prompt normally — `fit_prompt`'s truncation is a tax a stdin-fed CLI never needs.
        let prompt = if self.spec.argv_guard || matches!(self.spec.prompt, PromptChannel::Arg { .. }) {
            fit_prompt(&turn, &self.id)
        } else {
            crate::adapter::prompt::build(&turn, &self.id)
        };
        let invocation = self.invocation(prompt, mode, cwd);
        let outcome = if let Some(stream) = self.spec.stream.clone() {
            self.run_streaming(invocation, stream).await?
        } else {
            let result = self.runner.run(invocation).await?;
            reply_to_outcome(&result)
        };
        // Only a turn that actually ran leaves a conversation behind to resume.
        self.resident = true;
        Ok(outcome)
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
            nucleus_index: String::new(),
            nucleus_index_path: std::path::PathBuf::new(),
            nucleus_index_truncated: false,
            nucleus_index_budget_bytes: hadron_lattice::DEFAULT_NUCLEUS_INDEX_BUDGET_BYTES,
            nucleus_notes_dir: std::path::PathBuf::new(),
            git_diff: String::new(),
            cwd,
            mode,
            role_body: None,
            active_skill: None,
            named_specifically: true,
            has_forge_tools: false,
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
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "Gemini 3.1 Pro (High)", CliSpec::agy(), runner);

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
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "", spec, runner);

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
        assert!(prompt.contains("# Response Format & Output Strictness"), "the handoff reminder survives");
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
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "", spec, runner);
        let p = huge_projection(3, 100);
        let want = crate::adapter::prompt::build(&p, &QuarkId::new("agy"));

        q.excite(p).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        assert_eq!(&recorded[0].args[1], &want, "passed through untouched");
        assert!(!recorded[0].args[1].contains(TRUNCATION_MARKER));
    }

    #[tokio::test]
    async fn fit_prompt_to_budget_respects_windows_command_line_limit() {
        let p = huge_projection(200, 2000);
        let windows_safe_bytes = 24 * 1024;
        let fitted = fit_prompt_to_budget(&p, &QuarkId::new("agy"), windows_safe_bytes);
        assert!(
            fitted.len() <= windows_safe_bytes,
            "fitted prompt {} bytes must fit Windows safe limit {windows_safe_bytes}",
            fitted.len()
        );
        assert!(fitted.contains(TRUNCATION_MARKER));
        assert!(fitted.contains("# Who you are"));
    }

    /// Layer 3b of the cwd chain, agy side.
    #[tokio::test]
    async fn agy_invocation_carries_the_projection_cwd() {
        let runner = FakeRunner::with_stdout(vec!["ok"]);
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "", CliSpec::agy(), runner);
        let wt = PathBuf::from("/repo/.hadron/trees/agy");
        q.excite(projection_in("go", Mode::Write, wt.clone())).await.unwrap();
        assert_eq!(q.runner.recorded.lock().unwrap()[0].cwd, wt);
    }

    #[test]
    fn posture_maps_each_mode() {
        let spec = CliSpec::agy();
        assert_eq!(spec.posture.for_mode(Mode::Ask), vec!["--mode", "plan"]);
        assert_eq!(spec.posture.for_mode(Mode::Write), vec!["--mode", "accept-edits"]);
        assert_eq!(spec.posture.for_mode(Mode::Auto), vec!["--mode", "accept-edits"]);
        assert_eq!(spec.posture.for_mode(Mode::Bypass), vec!["--dangerously-skip-permissions"]);
    }

    #[tokio::test]
    async fn agy_runs_print_mode_and_maps_reply() {
        let runner = FakeRunner::with_stdout(vec!["UI complete. @claude back to you."]);
        // Display-name model id, as `agy models` reports them.
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut q = CliQuark::new(
            QuarkId::new("agy"),
            Flavor::Worker,
            "Gemini 3.1 Pro (High)",
            spec,
            runner,
        );

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
        assert!(recorded[0].args[1].contains("# Response Format & Output Strictness"));
        assert!(recorded[0].stdin.is_empty());
    }

    #[tokio::test]
    async fn invocation_carries_the_turn_posture() {
        let runner = FakeRunner::with_stdout(vec!["proposed", "done"]);
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "", CliSpec::agy(), runner);
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
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "", CliSpec::agy(), runner);

        q.excite(projection_mode("work", Mode::Bypass)).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        assert!(
            recorded[0].args.windows(2).any(|w| w == ["--print-timeout", "29m"]),
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
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "", spec.clone(), runner);

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

        // A chat lane instance on the same seat never resumes or sets resident.
        let chat_runner = FakeRunner::with_stdout(vec!["chat 1", "chat 2"]);
        let mut chat_q =
            CliQuark::new(QuarkId::new("agy"), Flavor::Orchestrator, "", spec.clone(), chat_runner);
        chat_q.become_chat_lane();

        let mut chat_turn = projection_mode("chat task", Mode::Bypass);
        chat_turn.field_window = vec![Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "CHAT-TRANSCRIPT-LINE".into() },
        )];

        chat_q.excite(chat_turn.clone()).await.unwrap();
        chat_q.excite(chat_turn).await.unwrap();

        let chat_recorded = chat_q.runner.recorded.lock().unwrap();
        assert!(
            !chat_recorded[0].args.iter().any(|a| a == "--continue"),
            "chat lane turn 1 does not resume"
        );
        assert!(
            !chat_recorded[1].args.iter().any(|a| a == "--continue"),
            "chat lane turn 2 also does not resume"
        );
        assert!(
            chat_recorded[1].args[1].contains("CHAT-TRANSCRIPT-LINE"),
            "a stateless chat lane must keep re-sending the field it cannot recall"
        );
    }

    /// **The chat lane must not steal the work lane's conversation.** An orchestrator
    /// seat runs two `CliQuark` instances (`Engine`'s `Lanes`). A `ResumeMode::Continue`
    /// CLI — which is what `agy` is — resumes ONE conversation, scoped per working
    /// directory, so if both lanes passed `--continue` they would interleave into it and
    /// each would read the other's turns as its own history. The chat lane is therefore
    /// stateless by construction: no resume flag, ever, and the full field window every
    /// turn (which is correct for it — its turns are short by design).
    ///
    /// The inverse of `a_resumed_turn_continues_the_session_and_stops_resending_the_field`
    /// above, same fixture, one call different.
    #[tokio::test]
    async fn a_chat_lane_never_resumes_the_work_lanes_conversation() {
        let runner = FakeRunner::with_stdout(vec!["one", "two"]);
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut q =
            CliQuark::new(QuarkId::new("agy"), Flavor::Orchestrator, "", spec, runner);
        q.become_chat_lane();

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
        assert!(
            !recorded.iter().any(|inv| inv.args.iter().any(|a| a == "--continue")),
            "a chat lane resumed a conversation it does not own"
        );
        assert!(
            recorded[1].args[1].contains("MEMORABLE-TRANSCRIPT-LINE"),
            "a stateless lane must keep re-sending the field it has no way to recall"
        );
    }

    /// **The CLI carry path, end to end.** A seat's `secret_env` names resolve
    /// through a `SecretStore` to `(name, value)` pairs (`Seat::resolve_env`, from a
    /// prior task) and must land on the `CliInvocation` the runner actually receives
    /// — never anywhere else. Drives the real `MemoryStore` → `resolve_env` →
    /// `with_env` → `CliInvocation.env` chain; never a real keychain, never a real
    /// spawn (`FakeRunner`).
    #[tokio::test]
    async fn cli_invocation_carries_resolved_secret_env() {
        use hadron_lattice::secrets::{MemoryStore, SecretStore};
        use hadron_lattice::Seat;

        let mut seat = Seat::cli(QuarkId::new("cli-agy"), "agy", "", Flavor::Worker);
        seat.secret_env = vec!["GEMINI_API_KEY".to_string()];

        let store = MemoryStore::new();
        store.set(&seat.id, "GEMINI_API_KEY", "k").unwrap();

        let runner = FakeRunner::with_stdout(vec!["ok"]);
        let mut q = CliQuark::new(seat.id.clone(), Flavor::Worker, "", CliSpec::agy(), runner)
            .with_env(seat.resolve_env(&store));

        q.excite(projection("go")).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        // Containment, not equality: the invocation also carries the shared build env
        // (`worktree::shared_build_env`). What this test guards is that the *secret*
        // reaches `env` and nothing else — so assert on the secret, and separately
        // that no other secret-shaped value snuck in.
        assert!(
            recorded[0].env.0.contains(&("GEMINI_API_KEY".to_string(), "k".to_string())),
            "the resolved secret must reach CliInvocation.env, got {:?}",
            recorded[0].env,
        );
        assert!(
            recorded[0].env.0.iter().all(|(k, _)| k == "GEMINI_API_KEY" || k.starts_with("CARGO_")),
            "only the seat's secret and the shared build env belong here, got {:?}",
            recorded[0].env,
        );
    }

    // --- Generic CLI tests: a spec with no argv guard, no resume, no posture, no
    // model flag — the "pipe prompt in, read reply out" default that a custom CLI
    // gets when it isn't `agy`. ---

    /// A generic CLI takes its prompt on stdin (never argv) and its raw stdout IS
    /// the message — no JSON envelope, no truncation, no special flags.
    #[tokio::test]
    async fn generic_cli_pipes_prompt_on_stdin_and_reads_raw_stdout() {
        let runner = FakeRunner::with_stdout(vec!["reply"]);
        let spec = CliSpec::generic("cat".to_string(), vec![]);
        let mut q = CliQuark::new(QuarkId::new("cat-quark"), Flavor::Worker, "", spec, runner);

        let p = projection("say hello");
        let want = crate::adapter::prompt::build(&p, &QuarkId::new("cat-quark"));

        let outcome = q.excite(p).await.unwrap();
        assert_eq!(outcome.message.as_deref(), Some("reply"));

        let recorded = q.runner.recorded.lock().unwrap();
        assert_eq!(recorded[0].program, "cat");
        assert_eq!(recorded[0].stdin, want, "the prompt rides on stdin, unguarded");
        assert!(recorded[0].args.is_empty(), "no argv args for a bare generic spec");
    }

    /// A generic spec has no `model_flag`, empty `posture`, `resume: None`, and
    /// `timeout: None` — none of those should ever appear on the invocation, no
    /// matter the mode or model set on the quark.
    #[tokio::test]
    async fn generic_cli_passes_no_model_or_posture_flags() {
        let runner = FakeRunner::with_stdout(vec!["one", "two"]);
        let spec = CliSpec::generic("cat".to_string(), vec![]);
        let mut q = CliQuark::new(QuarkId::new("cat-quark"), Flavor::Worker, "some-model", spec, runner);

        q.excite(projection_mode("first", Mode::Bypass)).await.unwrap();
        q.excite(projection_mode("second", Mode::Bypass)).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        for inv in recorded.iter() {
            assert!(inv.args.is_empty(), "generic spec has no argv flags at all: {:?}", inv.args);
        }
        // And no resume flag, even on the second turn — `resume: None` on the spec, so
        // `--continue` (an argv token on agy, never a stdin string) never appears at all.
        assert!(!recorded[1].args.iter().any(|a| a == "--continue"));
    }

    /// **The inverse of `a_resumed_turn_...stops_resending_the_field`.** `resident`
    /// alone must NOT trigger `without_field_window`: a generic CLI with
    /// `resume: None` has no `--continue`-equivalent, so it has no way to recall
    /// history it didn't just receive. Stripping the field window on turn two would
    /// silently throw away context the CLI can never get back — pinning that
    /// `excite`'s guard is `resident && spec.resume != None`, not `resident` alone.
    #[tokio::test]
    async fn generic_cli_does_not_strip_field_window_across_turns() {
        let runner = FakeRunner::with_stdout(vec!["one", "two"]);
        let spec = CliSpec::generic("cat".to_string(), vec![]);
        let mut q = CliQuark::new(QuarkId::new("cat-quark"), Flavor::Worker, "", spec, runner);

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
        // Stdin channel (generic spec), not argv — the prompt lands in `.stdin`.
        assert!(recorded[0].stdin.contains("MEMORABLE-TRANSCRIPT-LINE"), "first turn carries the field");
        let second_prompt = &recorded[1].stdin;
        assert!(second_prompt.contains("second task"), "the new instruction rides");
        assert!(
            second_prompt.contains("MEMORABLE-TRANSCRIPT-LINE"),
            "a no-resume CLI must keep re-sending context it has no way to recall"
        );
    }

    // --- Streaming (`spec.stream: Some(_)`) tests: a CliQuark whose invocation
    // gains the format's flags, whose reply is the parsed final text (not raw
    // stdout), whose usage lands on TurnOutcome, and whose deltas reach the live
    // feed exactly like `LocalQuark`'s. ---

    fn agy_stream_spec() -> hadron_lattice::StreamSpec {
        hadron_lattice::StreamSpec {
            flags: vec!["--output-format".to_string(), "stream-json".to_string()],
            format: hadron_lattice::StreamFormat::AgyStreamJson,
        }
    }

    /// A `spec.stream` seat must append the format's flags to the invocation, and
    /// its reply must be the STREAM's parsed final text, not the raw joined stdout
    /// (which is NDJSON, not a chat reply).
    #[tokio::test]
    async fn a_streaming_seat_appends_stream_flags_and_parses_the_final_reply() {
        let lines = vec![vec![
            r#"{"event":"init","conversation_id":"abc"}"#.to_string(),
            r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"hi"}}"#.to_string(),
            r#"{"event":"result","result":{"response":"hi","usage":{"input_tokens":10,"output_tokens":2,"thinking_tokens":1,"cache_read_tokens":3,"total_tokens":12}}}"#.to_string(),
        ]];
        let runner = FakeRunner::with_stream_lines(lines);
        let mut spec = CliSpec::agy();
        spec.stream = Some(agy_stream_spec());
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "", spec, runner);

        let outcome = q.excite(projection("say hi")).await.unwrap();
        assert_eq!(outcome.message.as_deref(), Some("hi"), "the reply is the parsed final text, not raw NDJSON");
        assert_eq!(outcome.usage.spend.input, Some(10));
        assert_eq!(outcome.usage.spend.output, Some(2));
        assert_eq!(outcome.usage.spend.cache_read, Some(3));

        let recorded = q.runner.recorded.lock().unwrap();
        // Exactly ONCE, not merely present: `invocation` appended `stream.flags`
        // twice — before the prompt and again before the posture flags — so every
        // real `agy` turn was spawned with `--output-format stream-json` duplicated.
        // `any()` could not see it.
        assert_eq!(
            recorded[0].args.windows(2).filter(|w| *w == ["--output-format", "stream-json"]).count(),
            1,
            "stream.flags must ride on the invocation exactly once: {:?}",
            recorded[0].args
        );
    }

    /// `spec.stream: None` (every built-in preset today) must be entirely
    /// unaffected: no stream flags, raw stdout is still the reply. The inverse of
    /// the test above, guarding rule 4 (branch beside the old path, don't touch it).
    #[tokio::test]
    async fn a_non_streaming_seat_is_unaffected_by_the_stream_field_existing() {
        let runner = FakeRunner::with_stdout(vec!["plain reply"]);
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut q = CliQuark::new(QuarkId::new("agy"), Flavor::Worker, "", spec, runner);

        let outcome = q.excite(projection("say hi")).await.unwrap();
        assert_eq!(outcome.message.as_deref(), Some("plain reply"));
        assert!(outcome.usage.is_empty(), "a non-streaming CLI reports no usage, same as before");

        let recorded = q.runner.recorded.lock().unwrap();
        assert!(!recorded[0].args.iter().any(|a| a == "--output-format"));
    }

    /// **The live-preview proof for a streaming CLI seat.** A `.watching(dir)`
    /// quark must publish its draft mid-turn (not just at the end) and leave the
    /// live dir clear once the turn is over — the same contract
    /// `AcpQuark`/`LocalQuark` already prove, now true for a CLI seat too.
    #[tokio::test]
    async fn a_watched_streaming_seat_publishes_deltas_and_clears_on_finish() {
        let dir = std::env::temp_dir().join(format!("hadron-cli-stream-test-{}", ulid::Ulid::new()));
        let id = QuarkId::new("agy");
        let lines = vec![vec![
            r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"partial"}}"#.to_string(),
            r#"{"event":"result","result":{"response":"partial done","usage":{"input_tokens":1,"output_tokens":1,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":2}}}"#.to_string(),
        ]];
        let runner = FakeRunner::with_stream_lines(lines);
        let mut spec = CliSpec::agy();
        spec.stream = Some(agy_stream_spec());
        let mut q = CliQuark::new(id.clone(), Flavor::Worker, "", spec, runner).watching(dir.clone());

        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished_clone = finished.clone();
        let (watch_dir, watch_id) = (dir.clone(), id.clone());
        let seen = tokio::spawn(async move {
            let mut saw_it = false;
            for _ in 0..200 {
                if finished_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if live::read(&watch_dir, &watch_id, chrono::Utc::now()).is_some() {
                    saw_it = true;
                    break;
                }
                tokio::task::yield_now().await;
            }
            saw_it
        });

        let outcome = q.excite(projection("say hi")).await.unwrap();
        finished.store(true, std::sync::atomic::Ordering::Relaxed);
        let saw_a_publish = seen.await.unwrap_or(false);

        assert!(saw_a_publish, "the delta must reach live::publish mid-turn, not only after excite returns");
        assert_eq!(outcome.message.as_deref(), Some("partial done"));
        assert_eq!(
            live::read(&dir, &id, chrono::Utc::now()),
            None,
            "a finished streaming turn must leave no activity behind, same as ACP/HTTP"
        );
    }

    #[test]
    fn probe_cli_models_fails_without_model_probe() {
        let spec = CliSpec::generic("echo".to_string(), vec![]);
        assert!(probe_cli_models(&spec).is_err());
    }

    #[test]
    fn probe_cli_models_runs_echo_probe() {
        let spec = CliSpec {
            program: "echo".to_string(),
            args: vec![],
            prompt: PromptChannel::Stdin,
            model_flag: None,
            temperature_flag: None,
            top_p_flag: None,
            max_tokens_flag: None,
            model_probe: Some(hadron_lattice::CliProbeSpec {
                args: vec!["gemini-3.6-flash Gemini 3.6 Flash\ngemini-3.6-pro Gemini 3.6 Pro".to_string()],
            }),
            resume: ResumeMode::None,
            timeout: None,
            posture: hadron_lattice::PostureMap::default(),
            argv_guard: false,
            stream: None,
        };
        let selector = probe_cli_models(&spec).unwrap();
        assert_eq!(selector.config_id, SessionConfigId::from("model"));
        assert_eq!(selector.available.len(), 2);
        assert_eq!(selector.available[0].value, "gemini-3.6-flash");
        assert_eq!(selector.available[0].label, "Gemini 3.6 Flash");
        assert_eq!(selector.available[1].value, "gemini-3.6-pro");
        assert_eq!(selector.available[1].label, "Gemini 3.6 Pro");
    }
}
