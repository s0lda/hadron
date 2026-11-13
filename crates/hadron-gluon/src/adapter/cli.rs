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
    let mut host_cmd = std::process::Command::new(&spec.program);
    host_cmd.args(&probe_spec.args);
    // Run model probe in the host temp directory so Windows host binaries (like `agy.exe`)
    // never inherit a WSL UNC workspace directory (`\\wsl.localhost\...`), which would cause
    // `agy.exe` to switch to WSL file-based token storage and fail authentication on Windows.
    host_cmd.current_dir(std::env::temp_dir());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        host_cmd.creation_flags(0x08000000);
    }
    let output = match host_cmd.output() {
        Ok(out) if out.status.success() => out,
        primary_res => {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let script = "NVM_BIN=$(ls -d \"$HOME\"/.nvm/versions/node/*/bin 2>/dev/null | sort -V | tail -n 1); export PATH=\"${NVM_BIN:+$NVM_BIN:}$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.gemini/antigravity-cli/bin:$HOME/.grok/bin:$HOME/.bun/bin:$HOME/.deno/bin:$HOME/.volta/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin:$PATH\"; exec \"$@\"";
                let mut cmd = std::process::Command::new("wsl.exe");
                cmd.args(["--exec", "sh", "-lc", script, "sh", &spec.program]);
                cmd.args(&probe_spec.args);
                cmd.creation_flags(0x08000000);
                if let Ok(wsl_out) = cmd.output() {
                    if wsl_out.status.success() {
                        wsl_out
                    } else {
                        primary_res?
                    }
                } else {
                    primary_res?
                }
            }
            #[cfg(not(windows))]
            {
                primary_res?
            }
        }
    };
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

/// The prompt budget for transports that do NOT ride argv — ACP sessions and
/// `PromptChannel::Stdin` CLIs. A pipe has no OS length limit, so holding these to
/// [`SAFE_ARG_BYTES`] (24 KiB on Windows) starved them of the nucleus index and
/// field history for no reason. This is a context-cost ceiling, not an OS guard.
pub const SAFE_PIPE_PROMPT_BYTES: usize = 256 * 1024;

/// When a prompt is over budget, the nucleus index is re-ranked down to this many
/// bytes before it is dropped outright — pinned and task-relevant lessons survive a
/// tight (Windows 24 KiB) argv budget instead of vanishing all at once.
pub const FIT_NUCLEUS_INDEX_BYTES: usize = 2048;

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
pub fn fit_prompt_to_budget(projection: &Projection, self_id: &QuarkId, max_bytes: usize) -> String {
    fit_projection_to_budget(projection, self_id, max_bytes).1
}

pub fn fit_projection_to_budget(
    projection: &Projection,
    self_id: &QuarkId,
    max_bytes: usize,
) -> (Projection, String) {
    let render = |p: &Projection| crate::adapter::prompt::build(p, self_id);

    let prompt = render(projection);
    if prompt.len() <= max_bytes {
        return (projection.clone(), prompt);
    }

    let mut p = projection.clone();
    let marker_event = || {
        Event::new(
            Actor::Gluon,
            None,
            Kind::Message {
                body: TRUNCATION_MARKER.to_string(),
            },
        )
    };

    // Binary-search the minimum number of oldest field events to drop so the rendered
    // prompt fits `max_bytes`, keeping at least the 2 most recent field events while
    // reference sections (`nucleus_index`, `nucleus_digest`, `git_diff`) can still be
    // shed first. Fast-checks the max-drop bound (`len - 2`) first so we spend 1 render
    // instead of N-2 renders when reference sections alone exceed `max_bytes`.
    if projection.field_window.len() > 2 {
        let max_drop = projection.field_window.len() - 2;
        p.field_window.clear();
        p.field_window.push(marker_event());
        p.field_window
            .extend_from_slice(&projection.field_window[max_drop..]);
        let min_window_out = render(&p);
        if min_window_out.len() <= max_bytes {
            let mut lo = 1usize;
            let mut hi = max_drop;
            let mut best = min_window_out;
            let mut best_p = p.clone();
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                p.field_window.clear();
                p.field_window.push(marker_event());
                p.field_window
                    .extend_from_slice(&projection.field_window[mid..]);
                let candidate = render(&p);
                if candidate.len() <= max_bytes {
                    best = candidate;
                    best_p = p.clone();
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            return (best_p, best);
        }
        // Keep the last 2 field events in `p.field_window` (without marker yet; added below).
        p.field_window.remove(0);
    }

    // Still over budget: drop the working diff and bulk reference sections
    // (`nucleus_digest`, tool catalogue, and excess `invariants`) first so that
    // the most recent field messages, `active_skill`, and ranked `nucleus_index`
    // lessons survive within tight command-line budgets (such as Windows 24 KiB).
    p.git_diff = String::new();
    p.field_window.insert(
        0,
        Event::new(
            Actor::Gluon,
            None,
            Kind::Message { body: TRUNCATION_MARKER.to_string() },
        ),
    );
    let mut out = render(&p);
    if out.len() <= max_bytes {
        return (p, out);
    }

    if !p.nucleus_digest.is_empty() {
        p.nucleus_digest.clear();
        out = render(&p);
        if out.len() <= max_bytes {
            return (p, out);
        }
    }
    if p.has_forge_tools {
        p.has_forge_tools = false;
        out = render(&p);
        if out.len() <= max_bytes {
            return (p, out);
        }
    }
    // Invariants are concatenated Standard Model first, repo `always.md` after, so a
    // cut from the end on a line boundary sheds trailing repo rules before any of the
    // Standard Model — and never leaves half a rule behind.
    let inv_soft_cap = max_bytes / 2;
    if p.invariants.len() > inv_soft_cap && inv_soft_cap > 0 {
        truncate_at_line(&mut p.invariants, inv_soft_cap);
        out = render(&p);
        if out.len() <= max_bytes {
            return (p, out);
        }
    }

    if !p.nucleus_index.is_empty() {
        // Re-rank to a small slice first: `rank_lessons` keeps pinned lessons and the
        // ones that match this task. It emits `budget / 8` bytes, hence the `* 8`.
        // The configured `nucleus_index_budget_bytes` is left alone — the prompt
        // quotes it to the quark as the index's real budget.
        let shrunk = crate::engine::nucleus::rank_lessons(
            &p.nucleus_index,
            &p.nucleus_notes_dir,
            &p.task,
            FIT_NUCLEUS_INDEX_BYTES * 8,
        );
        let kept_a_lesson = shrunk.lines().any(|l| l.starts_with("- "));
        if kept_a_lesson && shrunk.len() < p.nucleus_index.len() {
            p.nucleus_index = shrunk;
            p.nucleus_index_truncated = true;
            out = render(&p);
            if out.len() <= max_bytes {
                return (p, out);
            }
        }
        p.nucleus_index.clear();
        // Empty + truncated = "omitted for budget", which the prompt renders as such
        // instead of claiming nothing has ever been recorded.
        p.nucleus_index_truncated = true;
        out = render(&p);
        if out.len() <= max_bytes {
            return (p, out);
        }
    }
    if p.active_skill.is_some() {
        p.active_skill = None;
        out = render(&p);
        if out.len() <= max_bytes {
            return (p, out);
        }
    }
    while p.field_window.len() > 1 {
        p.field_window.remove(1);
        out = render(&p);
        if out.len() <= max_bytes {
            return (p, out);
        }
    }
    if !p.invariants.is_empty() {
        let mut without_inv = p.clone();
        without_inv.invariants.clear();
        let base_len = render(&without_inv).len();
        let inv_budget = max_bytes.saturating_sub(base_len + 40);
        if inv_budget > 0 && inv_budget < p.invariants.len() {
            truncate_at_line(&mut p.invariants, inv_budget);
            out = render(&p);
            if out.len() <= max_bytes {
                return (p, out);
            }
        }
        p.invariants.clear();
        out = render(&p);
        if out.len() <= max_bytes {
            return (p, out);
        }
    }

    // Nothing left to drop — the *task itself* is enormous. Hard-cut on a char
    // boundary rather than hand `execve`/`CreateProcessW` an argument it will reject outright.
    let mut cut = max_bytes.saturating_sub(TRUNCATION_MARKER.len() + 1);
    while cut > 0 && !out.is_char_boundary(cut) {
        cut -= 1;
    }
    if cut < p.task.len() {
        let mut task_cut = cut;
        while task_cut > 0 && !p.task.is_char_boundary(task_cut) {
            task_cut -= 1;
        }
        p.task.truncate(task_cut);
    }
    (p, format!("{}\n{TRUNCATION_MARKER}", &out[..cut]))
}

/// Atomically write an argv-overflow prompt to `<hadron_dir>/prompts/<id>.md` (temp +
/// rename, same shape as `prompt_cost::save`). `.hadron/` is untracked, so the spill
/// never dirties a worktree's git status.
pub fn spill_prompt(hadron_dir: &std::path::Path, id: &QuarkId, prompt: &str) -> std::io::Result<PathBuf> {
    let dir = hadron_dir.join("prompts");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", id.as_str()));
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, prompt)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// The pointer prepended to an argv prompt whose full version was spilled to `path`.
fn spill_header(path: &std::path::Path) -> String {
    format!(
        "# Read your full prompt first\n\
         This prompt was cut to fit the OS command-line limit. The complete prompt — full \
         transcript, nucleus index and instructions — is at `{}`. Read that file in full \
         before acting; where it differs from what follows, the file wins.\n\n",
        path.display()
    )
}

/// Truncate `s` to at most `max` bytes, ending on a line boundary (the newline is
/// kept) so no rule is left cut mid-sentence. With no newline under `max`, falls back
/// to the nearest char boundary rather than dropping everything.
fn truncate_at_line(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    if let Some(nl) = s[..cut].rfind('\n') {
        cut = nl + 1;
    }
    s.truncate(cut);
}

/// Returns true if an error string from a CLI subprocess execution represents
/// a transient/retryable network error (e.g. gRPC EOF, connection drops, 429 TPM, 503).
pub fn is_retryable_cli_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("\"retryable\":true")
        || lower.contains("\"retryable\": true")
        || lower.contains("there was a network issue connecting to the server")
        || (lower.contains("request failed: post") && lower.contains("eof"))
        || (lower.contains("streamgeneratecontent") && lower.contains("eof"))
        || lower.contains("silence deadline")
        || lower.contains("429 too many requests")
        || lower.contains("resource_exhausted")
        || lower.contains("503 service unavailable")
        || lower.contains("connection reset by peer")
        || lower.contains("broken pipe")
        || lower.contains("the stream was interrupted")
}

/// Fit a prompt into budget, using SlidingContextPruner first to preserve recent turns and
/// summarized older context, falling back to hard truncation if still over budget.
pub fn fit_prompt_with_sliding_pruner(
    projection: &Projection,
    self_id: &QuarkId,
    max_bytes: usize,
    config: &crate::sliding_pruner::SlidingPrunerConfig,
) -> String {
    fit_projection_with_sliding_pruner(projection, self_id, max_bytes, config).1
}

pub fn fit_projection_with_sliding_pruner(
    projection: &Projection,
    self_id: &QuarkId,
    max_bytes: usize,
    config: &crate::sliding_pruner::SlidingPrunerConfig,
) -> (Projection, String) {
    let render = |p: &Projection| crate::adapter::prompt::build(p, self_id);
    let mut p = projection.clone();
    // Always govern raw terminal/compiler dumps and `<SYSTEM_MESSAGE>` blocks in `task`
    // and `field_window` even when the overall prompt is under `max_bytes`.
    let _ = crate::sliding_pruner::SlidingContextPruner::govern_projection_tool_dumps(&mut p);
    let prompt = render(&p);
    if prompt.len() <= max_bytes {
        return (p, prompt);
    }

    if crate::sliding_pruner::SlidingContextPruner::prune_projection(&mut p, config) {
        let compacted_prompt = render(&p);
        if compacted_prompt.len() <= max_bytes {
            return (p, compacted_prompt);
        }
    }

    fit_projection_to_budget(&p, self_id, max_bytes)
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
    /// Optional reasoning effort level, passed via `spec.effort_flag` when both are set.
    effort: Option<String>,
    spec: CliSpec,
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
    sandbox: hadron_gatekeeper::SandboxMode,
    /// Where to publish mid-turn draft activity for a `spec.stream` seat. `None` =
    /// nobody is watching (tests, and any quark this daemon is not watching) —
    /// same shape as `adapter::local::LocalQuark::live_dir`. A `spec.stream: None`
    /// seat never reads this: there is nothing to publish until the process exits.
    live_dir: Option<PathBuf>,
    /// Where to persist the latest prompt cost breakdown (`<hadron-dir>/prompt-cost`).
    prompt_cost_dir: Option<PathBuf>,
    /// Shared cancel slot populated for the duration of `excite` so the dispatch
    /// loop can interrupt an in-flight CLI subprocess in `<1ms` without waiting
    /// for `CANCEL_DEADLINE` or resetting `resident`.
    cancel_slot: Option<crate::quark::CancelSlot>,
}

impl<R: CliRunner> CliQuark<R> {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, spec: CliSpec, runner: R) -> Self {
        CliQuark {
            id,
            flavor,
            display_name: None,
            model: model.into(),
            effort: None,
            spec,
            resident: false,
            runner,
            commands: SeatCommands::default(),
            env: RedactedEnv::default(),
            energy_limit: None,
            deny_skills: Vec::new(),
            sandbox: hadron_gatekeeper::SandboxMode::default(),
            live_dir: None,
            prompt_cost_dir: None,
            cancel_slot: None,
        }
    }

    pub fn with_sandbox(mut self, sandbox: hadron_gatekeeper::SandboxMode) -> Self {
        self.sandbox = sandbox;
        self
    }

    pub fn with_effort(mut self, effort: Option<String>) -> Self {
        self.effort = effort;
        self
    }

    /// Stream this quark's mid-turn draft into `dir` (see `hadron_lattice::live`).
    /// Only matters for a `spec.stream: Some(_)` seat — a non-streaming CLI seat
    /// has nothing to publish until its process exits either way.
    pub fn watching(mut self, dir: PathBuf) -> Self {
        self.prompt_cost_dir = dir.parent().map(|p| p.join("prompt-cost"));
        self.live_dir = Some(dir);
        self
    }

    pub fn with_prompt_cost_dir(mut self, dir: PathBuf) -> Self {
        self.prompt_cost_dir = Some(dir);
        self
    }

    fn hadron_dir(&self) -> Option<PathBuf> {
        self.prompt_cost_dir
            .as_deref()
            .and_then(std::path::Path::parent)
            .or_else(|| self.live_dir.as_deref().and_then(std::path::Path::parent))
            .map(std::path::Path::to_path_buf)
    }

    fn is_agy_seat(&self) -> bool {
        self.spec.is_agy_cli()
    }

    fn prepare_agy_statusline(
        &self,
        cwd: &std::path::Path,
        inv: &mut CliInvocation,
    ) -> Option<(PathBuf, PathBuf)> {
        if !self.is_agy_seat() {
            return None;
        }
        let dir = self.hadron_dir()?;
        let out_path = dir.join("statusline").join(format!("{}.json", self.id.as_str()));
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = std::fs::remove_file(&out_path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "gluon: warning: failed to clear prior statusline capture {}: {err}",
                    out_path.display()
                );
                return None;
            }
        }
        let child_out_path = translate_statusline_out_for_child(&out_path, cwd);
        let cmd = resolve_statusline_command(cwd);
        let settings_json = build_statusline_settings_json(&cmd);
        inv.args.push("--settings".to_string());
        inv.args.push(settings_json);
        inv.env
            .0
            .push((crate::statusline::OUT_ENV.to_string(), child_out_path));
        Some((dir, out_path))
    }

    fn apply_agy_statusline_telemetry(
        &self,
        outcome: &mut TurnOutcome,
        dir: &std::path::Path,
        out_path: &std::path::Path,
        cwd: &std::path::Path,
        turn_started: std::time::SystemTime,
    ) {
        let Some(tel) = crate::statusline::read_turn_capture(out_path, cwd, turn_started) else {
            return;
        };
        if let Some(tel_ctx) = tel.usage.context {
            if tel_ctx.context_window_size > 0 {
                let used = outcome
                    .usage
                    .context
                    .as_ref()
                    .map(|c| c.used_tokens)
                    .filter(|&u| u > 0)
                    .unwrap_or(tel_ctx.used_tokens);
                let size = tel_ctx.context_window_size;
                outcome.usage.context = Some(hadron_lattice::ContextUsage {
                    used_tokens: used,
                    context_window_size: size,
                    used_percentage: (used as f64 / size as f64) * 100.0,
                });
            }
        }
        if outcome.usage.model.is_none() && tel.model.is_some() {
            outcome.usage.model = tel.model;
        }
        if !tel.usage.quota.is_empty() {
            outcome.usage.quota = tel.usage.quota.clone();
            let _ = hadron_lattice::quota::save(&dir.join("quota"), &self.id, &tel.usage.quota);
        }
    }

    /// Fit a prompt that must ride as one argv element.
    ///
    /// The argv budget ([`SAFE_ARG_BYTES`], 24 KiB on Windows) sheds the transcript,
    /// nucleus index and more. When that happens and a `.hadron` state dir exists, the
    /// pipe-budget prompt is spilled to `<hadron-dir>/prompts/<id>.md` and the argv
    /// prompt opens with a pointer to it — so nothing shed is out of the quark's reach,
    /// while the argv prompt still carries everything that fits (a model that skips
    /// the file is no worse off than before).
    fn fit_argv_prompt(&self, turn: &Projection) -> (Projection, String) {
        let full = fit_projection_to_budget(turn, &self.id, SAFE_PIPE_PROMPT_BYTES);
        if full.1.len() <= SAFE_ARG_BYTES {
            return full;
        }
        let hadron_dir = self.hadron_dir().filter(|d| d.is_dir());
        let Some(hadron_dir) = hadron_dir.as_deref() else {
            return fit_projection_to_budget(turn, &self.id, SAFE_ARG_BYTES);
        };
        match spill_prompt(hadron_dir, &self.id, &full.1) {
            Ok(path) => {
                let header = spill_header(&path);
                let budget = SAFE_ARG_BYTES.saturating_sub(header.len());
                let (fitted, prompt) = fit_projection_to_budget(turn, &self.id, budget);
                (fitted, format!("{header}{prompt}"))
            }
            Err(e) => {
                eprintln!(
                    "CliQuark {}: could not spill the over-length prompt to {}: {e}",
                    self.id.as_str(),
                    hadron_dir.display()
                );
                fit_projection_to_budget(turn, &self.id, SAFE_ARG_BYTES)
            }
        }
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
        if let (Some(effort_flag), Some(effort)) = (&self.spec.effort_flag, &self.effort) {
            let trimmed = effort.trim();
            if !trimmed.is_empty()
                && !self.spec.is_agy_cli()
                && !hadron_lattice::model_encodes_effort(&self.model)
            {
                args.push(effort_flag.clone());
                args.push(trimmed.to_string());
            }
        }
        args.extend(self.spec.posture.for_mode(mode).iter().cloned());

        // The seat's resolved secrets, plus the shared build env so a `cargo` the
        // quark runs itself lands in the main checkout's warm `target/` instead of
        // growing a 37 GB one per worktree (`worktree::shared_build_env`). Seat env
        // is applied last: a seat that deliberately sets one of these wins.
        let mut env = crate::worktree::shared_build_env_for_quark(&cwd, self.id.as_str());
        env.extend(self.env.iter().cloned());

        CliInvocation {
            program: self.spec.program.clone(),
            args,
            stdin,
            cwd,
            env: env.into(),
            stream: self.spec.stream.clone(),
            sandbox: self.sandbox,
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
        let turn_started = chrono::Utc::now();
        let mut acc = StreamAccumulator::new(stream.format).with_work_dir(inv.cwd.clone());
        let mut loop_watchdog = crate::watchdog::LoopWatchdog::default();
        let mut watchdog_diag: Option<String> = None;
        let mut last_publish: Option<Instant> = None;
        let mut last_heartbeat: Option<Instant> = None;
        let in_error_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let in_error_for_line = std::sync::Arc::clone(&in_error_flag);
        let heartbeat_task = dir.as_ref().map(|d| {
            let dir_bg = d.clone();
            let qid_bg = quark_id.clone();
            let err_bg = std::sync::Arc::clone(&in_error_flag);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    if !err_bg.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = live::touch_heartbeat(&dir_bg, &qid_bg);
                    }
                }
            })
        });
        struct AbortOnDrop(Option<tokio::task::JoinHandle<()>>);
        impl Drop for AbortOnDrop {
            fn drop(&mut self) {
                if let Some(h) = self.0.take() {
                    h.abort();
                }
            }
        }
        let _heartbeat_guard = AbortOnDrop(heartbeat_task);
        let mut on_line = |line: &str| -> bool {
            let changed = acc.on_line(line);
            let status_is_error = acc.status() == "error message" || acc.status() == "error";
            in_error_for_line.store(
                acc.in_error_state() || status_is_error,
                std::sync::atomic::Ordering::Relaxed,
            );
            if acc.error_step_count() >= 3 {
                return false;
            }
            if let Some(sig) = crate::watchdog::LoopWatchdog::extract_stream_tool_signature(line) {
                if let Some(diag) = loop_watchdog.observe(&sig) {
                    watchdog_diag = Some(diag);
                    return false;
                }
            }
            let Some(dir) = &dir else { return true };
            if !changed {
                if !line.trim().is_empty()
                    && !acc.in_error_state()
                    && !status_is_error
                    && last_heartbeat.is_none_or(|t: Instant| t.elapsed() >= PUBLISH_THROTTLE)
                {
                    last_heartbeat = Some(Instant::now());
                    let _ = live::touch_heartbeat(dir, &quark_id);
                }
                return true;
            }
            // Status transitions arrive in bursts (e.g. `agent_response` followed <1ms
            // later by `tool_name: run_command`) and must never be dropped by the text
            // delta throttle, even after an earlier step has already appended to `draft`.
            let is_status_update = acc.is_status_update() || acc.draft().is_empty();
            let due = is_status_update
                || last_publish.is_none_or(|t: Instant| t.elapsed() >= PUBLISH_THROTTLE);
            if due {
                let now = Instant::now();
                last_publish = Some(now);
                if !acc.in_error_state() && !status_is_error {
                    last_heartbeat = Some(now);
                }
                let mut activity = if is_status_update {
                    let status = acc.status();
                    let doing = if status == "thinking"
                        || status.starts_with("thinking:")
                        || status == "reading the message"
                    {
                        Doing::Thinking
                    } else {
                        Doing::Working
                    };
                    let mut a = Activity::new(quark_id.clone(), doing, status);
                    if !acc.draft().is_empty() {
                        a.full = Some(acc.draft().to_string());
                    }
                    a
                } else {
                    Activity::speaking(quark_id.clone(), acc.draft())
                };
                activity.started = Some(turn_started);
                crate::adapter::telemetry::TurnTelemetrySink::publish_activity(dir, &mut activity);
            }
            true
        };
        let run_result = self.runner.run_streaming(inv, &mut on_line).await;
        if let Some(dir) = &self.live_dir {
            crate::adapter::telemetry::TurnTelemetrySink::clear_live(dir, &self.id);
        }
        let error_step_exhausted = acc.error_step_count() >= 3;
        let in_error = acc.in_error_state();
        let latest_err_msg = acc.latest_error_message();
        let res = acc.finish_result();
        if let Some(diag) = watchdog_diag {
            anyhow::bail!("{diag}");
        }
        if error_step_exhausted || (in_error && res.message.is_none()) {
            let err_msg = latest_err_msg
                .unwrap_or_else(|| "quark stream entered error state".to_string());
            anyhow::bail!("quark stream failed with error steps: {err_msg}");
        }
        run_result?;
        let model = res
            .model
            .or_else(|| (!self.model.trim().is_empty()).then(|| self.model.clone()));
        let context_tokens = res.context_used.or_else(|| {
            if !self.model.trim().is_empty() && !res.spend.is_empty() {
                // Never fold cumulative `res.spend.cache_read` into context window occupancy:
                // on multi-step CLI turns (e.g. `agy --output-format stream-json`), `cache_read`
                // sums cached prompt reads across every tool step (often 5M–15M tokens).
                res.spend.input.filter(|&n| n > 0)
            } else {
                None
            }
        });
        let is_agy = self.is_agy_seat();
        let context = context_tokens.map(|used| {
            let size = res
                .context_window
                .unwrap_or_else(|| default_cli_context_window_size(is_agy, model.as_deref()));
            hadron_lattice::ContextUsage {
                used_tokens: used,
                context_window_size: size,
                used_percentage: if size > 0 {
                    (used as f64 / size as f64) * 100.0
                } else {
                    0.0
                },
            }
        });
        Ok(TurnOutcome {
            message: res.message,
            usage: Usage {
                spend: res.spend,
                model,
                context,
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

fn translate_statusline_out_for_child(out_path: &std::path::Path, cwd: &std::path::Path) -> String {
    if hadron_lattice::wsl::is_wsl_unc_path(cwd) {
        if let Some((_, linux_out)) = hadron_lattice::wsl::parse_wsl_unc_path(out_path) {
            return linux_out.to_string_lossy().replace('\\', "/");
        }
        if let Some((distro, _)) = hadron_lattice::wsl::parse_wsl_unc_path(cwd) {
            return hadron_lattice::wsl::translate_arg_for_wsl(&distro, &out_path.to_string_lossy());
        }
    }
    out_path.to_string_lossy().to_string()
}

fn sibling_gluon_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let bin_name = if cfg!(windows) {
        "hadron-gluon.exe"
    } else {
        "hadron-gluon"
    };
    let candidate = dir.join(bin_name);
    candidate.is_file().then_some(candidate)
}

fn resolve_statusline_command(cwd: &std::path::Path) -> String {
    if !hadron_lattice::wsl::is_wsl_unc_path(cwd) {
        if let Some(sibling) = sibling_gluon_exe() {
            return format!("\"{}\" --statusline", sibling.display());
        }
    }
    r#"sh -c 'p="$HADRON_STATUSLINE_OUT"; if [ -n "$p" ]; then (cat > "$p") 2>/dev/null || { d="$HOME/.hadron/statusline"; mkdir -p "$d" 2>/dev/null && cat > "$d/${p##*/}" 2>/dev/null; }; fi; echo hadron'"#.to_string()
}

fn build_statusline_settings_json(command: &str) -> String {
    serde_json::json!({
        "statusLine": {
            "type": "command",
            "command": command,
            "enabled": true,
        }
    })
    .to_string()
}

/// Default context window size when a streaming CLI (`agy --output-format stream-json`,
/// `claude --output-format stream-json`) omits `context_window_size` on stdout.
///
/// Google's `agy` CLI (`BuildStatusLineData` / `Store.ModelMaxTokens`) reports a 1M
/// (`1_048_576` = $2^{20}$) context window for both Gemini 3.x/4.x and Claude Opus/Sonnet 5.5
/// seats; non-`agy` Claude CLI models default to `1_000_000` for 1M variants (`claude-opus-5*`,
/// `claude-sonnet-5*`, `claude-opus-4-6`/`4-7`/`4-8`, or `1m`) and `200_000` otherwise.
fn default_cli_context_window_size(is_agy: bool, model: Option<&str>) -> u32 {
    if is_agy {
        return 1_048_576;
    }
    if let Some(m) = model {
        let lower = m.to_ascii_lowercase();
        let is_1m_claude = [
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-opus-4-6",
            "claude-opus-4-7",
            "claude-opus-4-8",
            "opus-5",
            "sonnet-5",
            "opus-4-6",
            "opus-4-7",
            "opus-4-8",
        ]
        .iter()
        .any(|pat| lower.contains(pat));
        if is_1m_claude {
            return 1_000_000;
        }
        if lower.contains("claude") {
            return if lower.contains("1m") { 1_000_000 } else { 200_000 };
        }
    }
    1_048_576
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
    fn attach_cancel_slot(&mut self, slot: crate::quark::CancelSlot) {
        self.cancel_slot = Some(slot);
    }
    fn reset_session(&mut self) {
        self.resident = false;
    }
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let mode = turn.mode;
        let cwd = turn.cwd.clone();

        let cancel_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let cancelled_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Some(slot) = &self.cancel_slot {
            let notify = cancel_notify.clone();
            let flag = cancelled_flag.clone();
            slot.set(Some(std::sync::Arc::new(move || {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                notify.notify_waiters();
                true
            })));
        }
        struct ClearCancelSlot(Option<crate::quark::CancelSlot>);
        impl Drop for ClearCancelSlot {
            fn drop(&mut self) {
                if let Some(slot) = &self.0 {
                    slot.set(None);
                }
            }
        }
        let _cancel_guard = ClearCancelSlot(self.cancel_slot.clone());

        let max_attempts = 3;
        let mut last_err = None;

        for attempt in 0..max_attempts {
            if cancelled_flag.load(std::sync::atomic::Ordering::SeqCst) {
                if let Some(dir) = &self.live_dir {
                    crate::adapter::telemetry::TurnTelemetrySink::clear_live(dir, &self.id);
                }
                return Ok(TurnOutcome {
                    message: None,
                    cancelled: true,
                    ..Default::default()
                });
            }
            if attempt > 0 {
                // On a retry, reset resident state so we don't try --continue on a dead/corrupted session
                self.resident = false;
                if let Some(dir) = &self.live_dir {
                    let mut retry_act = Activity::new(
                        self.id.clone(),
                        Doing::Thinking,
                        &format!("retrying turn in fresh session (attempt {}/{max_attempts})…", attempt + 1),
                    );
                    crate::adapter::telemetry::TurnTelemetrySink::publish_activity(dir, &mut retry_act);
                }
                tokio::select! {
                    _ = cancel_notify.notified() => {
                        if let Some(dir) = &self.live_dir {
                            crate::adapter::telemetry::TurnTelemetrySink::clear_live(dir, &self.id);
                        }
                        return Ok(TurnOutcome {
                            message: None,
                            cancelled: true,
                            ..Default::default()
                        });
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500 * (attempt as u64))) => {}
                }
            }

            // A resumed conversation already holds everything we sent before, so re-sending
            // the field window and static cache prefix would pay for the same history twice —
            // the whole point of going resident. Send the new instruction only; the CLI still has the rest.
            // Only true when this CLI actually supports resuming (`spec.resume` is not
            // `None`) — a generic CLI with no resume flag never gets to drop context it
            // has no way to recall.
            let resuming = self.resident && !matches!(self.spec.resume, ResumeMode::None);
            let mut prepared_turn = if resuming {
                crate::adapter::prompt::without_static_prefix(without_field_window(turn.clone()))
            } else {
                turn.clone()
            };
            if attempt > 0 && !turn.cwd.as_os_str().is_empty() && turn.cwd.join(".git").exists() {
                if let Ok(Some(live_diff)) = crate::snapshot::git_ok(&turn.cwd, &["diff", "HEAD"]) {
                    if !live_diff.trim().is_empty() {
                        prepared_turn.git_diff = live_diff;
                    }
                }
            }

            // The argv guard only applies when the spec says the prompt rides as one
            // argv element with no stdin fallback (`execve` on Linux rejects an over-long one
            // with E2BIG; `CreateProcessW` on Windows returns error 206 before the CLI ever starts).
            // Otherwise the prompt rides a pipe: fit it through `SlidingContextPruner` to the
            // pipe budget, which has no OS limit behind it.
            let (fitted_turn, prompt) =
                if self.spec.argv_guard || matches!(self.spec.prompt, PromptChannel::Arg { .. }) {
                    self.fit_argv_prompt(&prepared_turn)
                } else {
                    fit_projection_with_sliding_pruner(
                        &prepared_turn,
                        &self.id,
                        SAFE_PIPE_PROMPT_BYTES,
                        &crate::sliding_pruner::SlidingPrunerConfig::default(),
                    )
                };

            if attempt == 0 {
                if let Some(dir) = &self.prompt_cost_dir {
                    let breakdown = crate::adapter::prompt::measure(&fitted_turn, &self.id);
                    let _ = hadron_lattice::prompt_cost::save(dir, &self.id, &breakdown);
                }
            }

            let mut invocation = self.invocation(prompt, mode, cwd.clone());
            let statusline_capture = self.prepare_agy_statusline(&cwd, &mut invocation);
            let turn_started = std::time::SystemTime::now();
            let run_opt = {
                let run_fut = async {
                    if let Some(stream) = self.spec.stream.clone() {
                        self.run_streaming(invocation, stream).await
                    } else {
                        let result = self.runner.run(invocation).await;
                        result.map(|r| reply_to_outcome(&r))
                    }
                };
                tokio::select! {
                    biased;
                    _ = cancel_notify.notified() => None,
                    res = run_fut => Some(res),
                }
            };

            let Some(res) = run_opt else {
                // Graceful cancel: dropping `run_fut` drops `ProcessRunner::run`'s
                // `ChildProcessGuard`, killing the subprocess group in <1ms while
                // preserving `self.resident` so the resumed turn uses `--continue`.
                if let Some(dir) = &self.live_dir {
                    crate::adapter::telemetry::TurnTelemetrySink::clear_live(dir, &self.id);
                }
                return Ok(TurnOutcome {
                    message: None,
                    cancelled: true,
                    ..Default::default()
                });
            };
            if cancelled_flag.load(std::sync::atomic::Ordering::SeqCst) {
                if let Some(dir) = &self.live_dir {
                    crate::adapter::telemetry::TurnTelemetrySink::clear_live(dir, &self.id);
                }
                return Ok(TurnOutcome {
                    message: None,
                    cancelled: true,
                    ..Default::default()
                });
            }

            match res {
                Ok(mut outcome) => {
                    if let Some((ref dir, ref out_path)) = statusline_capture {
                        self.apply_agy_statusline_telemetry(
                            &mut outcome,
                            dir,
                            out_path,
                            &cwd,
                            turn_started,
                        );
                    }
                    self.resident = true;
                    return Ok(outcome);
                }
                Err(err) => {
                    let err_msg = err.to_string();
                    let already_exhausted_stream =
                        err_msg.contains("quark stream failed with error steps");
                    if !already_exhausted_stream
                        && is_retryable_cli_error(&err_msg)
                        && attempt + 1 < max_attempts
                    {
                        eprintln!(
                            "CliQuark {} turn attempt {} failed with retryable error, retrying: {err_msg}",
                            self.id.as_str(),
                            attempt + 1
                        );
                        last_err = Some(err);
                        continue;
                    } else {
                        // Unretryable error or retries exhausted: reset resident state so future turns start fresh
                        self.resident = false;
                        return Err(err);
                    }
                }
            }
        }

        self.resident = false;
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("CliQuark turn execution failed")))
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
            active_skill: None,
            named_specifically: true,
            from_human_directly: false,
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
        let mut p = huge_projection(200, 2000);
        p.invariants = format!("# The Standard Model\n{}", "rule ".repeat(6000));
        p.nucleus_digest = "feature map ".repeat(400);
        p.nucleus_index = "- [lesson](notes/lesson.md) — hook\n".repeat(200);
        p.has_forge_tools = true;
        let windows_safe_bytes = 24 * 1024;
        let fitted = fit_prompt_to_budget(&p, &QuarkId::new("agy"), windows_safe_bytes);
        assert!(
            fitted.len() <= windows_safe_bytes,
            "fitted prompt {} bytes must fit Windows safe limit {windows_safe_bytes}",
            fitted.len()
        );
        assert!(fitted.contains(TRUNCATION_MARKER));
        assert!(fitted.contains("# Who you are"));
        assert!(
            fitted.contains("# Your task\nsummarise the work so far"),
            "task section must survive Windows 24 KiB command-line truncation even when invariants exceed 24 KiB"
        );
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
            recorded[0].env.0.iter().all(|(k, _)| k == "GEMINI_API_KEY" || k.starts_with("CARGO_") || k.starts_with("HADRON_PORT_")),
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
            effort_flag: None,
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

    #[test]
    fn test_fit_prompt_with_sliding_pruner_compacts_field() {
        let p = huge_projection(12, 100);
        let id = QuarkId::new("agy");
        let config = crate::sliding_pruner::SlidingPrunerConfig {
            max_recent_events: 3,
            max_summary_tokens: 100,
            compaction_threshold_tokens: 50,
        };
        let initial_len = crate::adapter::prompt::build(&p, &id).len();
        let prompt = fit_prompt_with_sliding_pruner(&p, &id, initial_len - 10, &config);
        assert!(prompt.contains("Sliding Context Pruner: compacted"));
    }

    #[tokio::test]
    async fn streaming_context_fallback_never_adds_cumulative_cache_read_tokens() {
        let lines = vec![vec![
            r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"done"}}"#.to_string(),
            r#"{"event":"result","result":{"response":"done","usage":{"input_tokens":443232,"output_tokens":79850,"thinking_tokens":12000,"cache_read_tokens":14410669,"total_tokens":523082}}}"#.to_string(),
        ]];
        let runner = FakeRunner::with_stream_lines(lines);
        let mut spec = CliSpec::agy();
        spec.stream = Some(agy_stream_spec());
        let mut q = CliQuark::new(
            QuarkId::new("cli-agy"),
            Flavor::Orchestrator,
            "gemini-4-argon-medium",
            spec,
            runner,
        );
        let outcome = q.excite(projection("check context")).await.unwrap();
        assert_eq!(outcome.usage.spend.input, Some(443_232));
        assert_eq!(outcome.usage.spend.cache_read, Some(14_410_669));
        let ctx = outcome.usage.context.expect("context should be populated");
        assert_eq!(
            ctx.used_tokens, 443_232,
            "cumulative cache_read_tokens (14.4M) must never be folded into context.used_tokens"
        );
        assert_eq!(
            ctx.context_window_size, 1_048_576,
            "agy / Gemini models must default to a 1M (1_048_576) context window matching agy statusLine"
        );
    }

    #[tokio::test]
    async fn a_resumed_turn_strips_static_prefix_and_persists_prompt_cost() {
        let dir = std::env::temp_dir().join(format!("hadron-cli-cost-test-{}", ulid::Ulid::new()));
        let live_dir = dir.join("live");
        let cost_dir = dir.join("prompt-cost");
        let id = QuarkId::new("cli-agy");
        let runner = FakeRunner::with_stdout(vec!["first reply", "second reply"]);
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut q = CliQuark::new(id.clone(), Flavor::Worker, "", spec, runner).watching(live_dir);

        let mut first = projection_mode("first task", Mode::Bypass);
        first.invariants = "STATIC-INVARIANTS-CANARY".to_string();
        first.nucleus_digest = "STATIC-DIGEST-CANARY".to_string();
        first.has_forge_tools = true;
        first.field_window = vec![Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "FIRST-FIELD-LINE".into() },
        )];

        let mut second = first.clone();
        second.task = "second task".into();

        q.excite(first).await.unwrap();
        let first_cost = hadron_lattice::prompt_cost::load(&cost_dir, &id).expect("first turn prompt_cost saved");
        assert!(first_cost.invariants > 0);
        assert!(first_cost.nucleus_digest > 0);

        q.excite(second).await.unwrap();
        let second_cost = hadron_lattice::prompt_cost::load(&cost_dir, &id).expect("second turn prompt_cost saved");
        assert_eq!(second_cost.invariants, 0, "resumed turn strips already-cached invariants");
        assert_eq!(second_cost.nucleus_digest, 0, "resumed turn strips already-cached nucleus_digest");

        let recorded = q.runner.recorded.lock().unwrap();
        assert!(recorded[0].args[1].contains("STATIC-INVARIANTS-CANARY"));
        let resumed_prompt = &recorded[1].args[2];
        assert!(resumed_prompt.contains("second task"));
        assert!(!resumed_prompt.contains("STATIC-INVARIANTS-CANARY"));
        assert!(!resumed_prompt.contains("STATIC-DIGEST-CANARY"));
        assert!(!resumed_prompt.contains("# Available Hadron Forge Tools"));
    }

    #[tokio::test]
    async fn a_stdin_cli_seat_compacts_oversized_field_via_sliding_pruner() {
        let runner = FakeRunner::with_stdout(vec!["ok"]);
        let spec = CliSpec::generic("cat".to_string(), vec![]);
        let mut q = CliQuark::new(QuarkId::new("cat-quark"), Flavor::Worker, "", spec, runner);

        // 300 x 1.2 KB is over the 256 KiB `SAFE_PIPE_PROMPT_BYTES` stdin budget.
        let p = huge_projection(300, 1200);
        q.excite(p).await.unwrap();

        let recorded = q.runner.recorded.lock().unwrap();
        assert!(
            recorded[0].stdin.contains("Sliding Context Pruner: compacted"),
            "oversized stdin prompt must compact older field events via SlidingContextPruner"
        );
    }

    #[tokio::test]
    async fn cli_quark_passes_effort_flag_only_when_spec_and_seat_define_it() {
        let runner_claude = FakeRunner::with_stdout(vec!["ok"]);
        let claude_spec = CliSpec::claude();
        let mut q_claude = CliQuark::new(
            QuarkId::new("cli-claude"),
            Flavor::Worker,
            "claude-opus-4-6",
            claude_spec,
            runner_claude,
        )
        .with_effort(Some("high".to_string()));
        q_claude.excite(projection("do task")).await.unwrap();

        let runner_agy = FakeRunner::with_stdout(vec!["ok"]);
        let agy_spec = CliSpec::agy();
        let mut q_agy = CliQuark::new(
            QuarkId::new("cli-agy"),
            Flavor::Orchestrator,
            "gemini-4-carbon-medium",
            agy_spec,
            runner_agy,
        )
        .with_effort(Some("high".to_string()));
        q_agy.excite(projection("do task")).await.unwrap();

        // Defence-in-depth control: even a custom non-agy CLI spec with `effort_flag`
        // must omit `--effort` when the model identifier already encodes effort.
        let runner_baked = FakeRunner::with_stdout(vec!["ok"]);
        let mut q_baked = CliQuark::new(
            QuarkId::new("cli-claude-baked"),
            Flavor::Worker,
            "claude-opus-5-5-high",
            CliSpec::claude(),
            runner_baked,
        )
        .with_effort(Some("low".to_string()));
        q_baked.excite(projection("do task")).await.unwrap();

        // Negative control: a spec with no effort flag must never pass one, even
        // when the seat sets an effort.
        let runner_none = FakeRunner::with_stdout(vec!["ok"]);
        let none_spec = CliSpec { effort_flag: None, ..CliSpec::claude() };
        let mut q_none = CliQuark::new(
            QuarkId::new("cli-none"),
            Flavor::Worker,
            "some-model",
            none_spec,
            runner_none,
        )
        .with_effort(Some("high".to_string()));
        q_none.excite(projection("do task")).await.unwrap();

        let recorded_claude = q_claude.runner.recorded.lock().unwrap();
        assert!(
            recorded_claude[0]
                .args
                .windows(2)
                .any(|w| w[0] == "--effort" && w[1] == "high"),
            "claude CLI spec with effort_flag must pass --effort high, got: {:?}",
            recorded_claude[0].args
        );
        let recorded_agy = q_agy.runner.recorded.lock().unwrap();
        assert!(
            !recorded_agy[0].args.iter().any(|a| a == "--effort"),
            "agy CLI encodes effort in the model id and must never pass --effort, got: {:?}",
            recorded_agy[0].args
        );
        let recorded_baked = q_baked.runner.recorded.lock().unwrap();
        assert!(
            !recorded_baked[0].args.iter().any(|a| a == "--effort"),
            "effort-encoded model id must never emit a conflicting --effort flag, got: {:?}",
            recorded_baked[0].args
        );
        let recorded_none = q_none.runner.recorded.lock().unwrap();
        assert!(
            !recorded_none[0].args.iter().any(|a| a == "--effort"),
            "a CLI spec with effort_flag: None must never pass --effort, got: {:?}",
            recorded_none[0].args
        );
    }

    #[test]
    fn fit_prompt_to_budget_binary_search_preserves_maximum_recent_events() {
        let mut p = huge_projection(50, 500);
        for (i, ev) in p.field_window.iter_mut().enumerate() {
            ev.kind = Kind::Message {
                body: format!("EVENT-{i:03} {}", "y".repeat(500)),
            };
        }
        // Budget large enough to hold roughly the newest 10 events plus static chrome.
        let budget = 8 * 1024;
        let fitted = fit_prompt_to_budget(&p, &QuarkId::new("agy"), budget);
        assert!(fitted.len() <= budget);
        assert!(fitted.contains(TRUNCATION_MARKER));
        assert!(fitted.contains("EVENT-049"), "newest event must survive");
        assert!(fitted.contains("EVENT-045"), "recent events fitting budget must survive");
        assert!(!fitted.contains("EVENT-000"), "oldest events must be dropped");
    }

    #[test]
    fn detects_retryable_cli_errors() {
        let agy_err = "Quark cli-agy turn errored: agy exited with 3: error: There was a network issue connecting to the server, please try again.\nAGY_ERROR: {\"short_error\":\"agent executor error: generating and executing: request failed: Post \\\"https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse\\\": EOF\",\"status\":\"UNKNOWN\",\"error_code\":2,\"code_kind\":\"grpc\",\"retryable\":true,\"error_id\":\"2c0ddb97-6a2c-4e84-be79-08dded829bf7-77\"}";
        assert!(is_retryable_cli_error(agy_err));

        assert!(is_retryable_cli_error("429 Too Many Requests"));
        assert!(is_retryable_cli_error("503 Service Unavailable"));
        assert!(is_retryable_cli_error("connection reset by peer"));
        assert!(is_retryable_cli_error("Error: The stream was interrupted. Please continue the task you were working on."));
        assert!(!is_retryable_cli_error("command not found: foobar"));
    }

    #[tokio::test]
    async fn cli_quark_retries_transient_network_error_and_resets_resident_state() {
        use crate::adapter::runner::CliResult;

        struct RetryableFakeRunner {
            attempts: std::sync::atomic::AtomicUsize,
        }

        #[async_trait]
        impl CliRunner for RetryableFakeRunner {
            async fn run(&self, _inv: CliInvocation) -> anyhow::Result<CliResult> {
                let attempt = self.attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if attempt == 0 {
                    anyhow::bail!("agy exited with 3: error: There was a network issue connecting to the server, please try again.\nAGY_ERROR: {{\"retryable\":true}}");
                }
                Ok(CliResult { stdout: "Success after retry".to_string(), exit: 0, usage: None })
            }
            async fn run_streaming(&self, inv: CliInvocation, _on_line: &mut (dyn for<'a> FnMut(&'a str) -> bool + Send)) -> anyhow::Result<CliResult> {
                self.run(inv).await
            }
        }

        let runner = RetryableFakeRunner { attempts: std::sync::atomic::AtomicUsize::new(0) };
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut quark = CliQuark::new(
            QuarkId::new("cli-agy"),
            Flavor::Worker,
            "gemini-3.6-flash",
            spec,
            runner,
        );

        // Turn 1 fails on attempt 0 with retryable error, then retries and succeeds!
        let outcome = quark.excite(projection("do task")).await.unwrap();
        assert_eq!(outcome.message.as_deref(), Some("Success after retry"));
        assert_eq!(quark.runner.attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cli_quark_streaming_aborts_immediately_on_consecutive_error_steps() {
        let stream_lines = vec![vec![
            r#"{"event":"step_update","step_update":{"step_type":"error_message","message":"Error 1"}}"#.to_string(),
            r#"{"event":"step_update","step_update":{"step_type":"error_message","message":"Error 2"}}"#.to_string(),
            r#"{"event":"step_update","step_update":{"step_type":"error_message","message":"Error 3"}}"#.to_string(),
            r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"Never reached"}}"#.to_string(),
        ]];
        let runner = FakeRunner::with_stream_lines(stream_lines);
        let mut spec = CliSpec::agy();
        spec.stream = Some(agy_stream_spec());
        let mut quark = CliQuark::new(
            QuarkId::new("cli-agy"),
            Flavor::Worker,
            "gemini-3.6-flash",
            spec,
            runner,
        );

        let err = quark.excite(projection("do task")).await.unwrap_err();
        assert!(
            err.to_string().contains("quark stream failed with error steps"),
            "streaming execution must abort immediately when consecutive error steps >= 3, got: {err}"
        );
    }

    #[test]
    fn fit_prompt_to_budget_preserves_nucleus_index_and_recent_field_when_invariants_is_large() {
        let mut p = projection_mode("fix bug", Mode::Bypass);
        p.invariants = format!("# The Standard Model\n{}", "rule ".repeat(3500)); // ~17.5 KiB
        p.nucleus_digest = "feature map ".repeat(300); // ~3.6 KiB
        p.nucleus_index =
            "## How we get things wrong\n- [bug-canary](notes/bug-canary.md) — critical lesson\n"
                .to_string();
        p.nucleus_index_path = PathBuf::from("/repo/.hadron/nucleus/index.md");
        p.field_window = vec![
            Event::new(Actor::Human, None, Kind::Message { body: "OLDER-MSG".into() }),
            Event::new(Actor::Human, None, Kind::Message { body: "RECENT-MSG-1".into() }),
            Event::new(Actor::Human, None, Kind::Message { body: "RECENT-MSG-2".into() }),
        ];

        let fitted = fit_prompt_to_budget(&p, &QuarkId::new("agy"), 24 * 1024);
        assert!(fitted.len() <= 24 * 1024);
        assert!(
            fitted.contains("bug-canary"),
            "ranked nucleus_index must survive Windows 24 KiB budget when invariants is ~17.5 KiB"
        );
        assert!(
            fitted.contains("RECENT-MSG-2"),
            "most recent field message must survive Windows 24 KiB budget"
        );
    }

    /// An index of `n` task-relevant lessons plus a pinned section of `pinned` lines.
    fn big_index(pinned: usize, n: usize) -> String {
        let mut s = String::from("## How we get things wrong\n\n");
        for i in 0..pinned {
            s.push_str(&format!("- [pinned-canary-{i}](notes/pinned-canary-{i}.md) — must survive\n"));
        }
        s.push_str("\n## Other\n\n");
        for i in 0..n {
            s.push_str(&format!("- [lesson-{i}](notes/lesson-{i}.md) — fix bug detail number {i}\n"));
        }
        s
    }

    /// A projection whose only big shed-able section is the nucleus index, plus the
    /// rendered length with the index omitted — so a test can place `max_bytes`
    /// exactly between "shrunk index fits" and "nothing but the notice fits".
    fn tight_index_projection(pinned: usize) -> (Projection, usize) {
        let mut p = projection_mode("fix bug", Mode::Bypass);
        p.invariants = "# The Standard Model\n".to_string() + &"- a rule line\n".repeat(400);
        p.active_skill = Some(format!("# Skill for this turn: SKILL-CANARY\n{}", "step\n".repeat(600)));
        p.nucleus_index = big_index(pinned, 400);
        p.nucleus_index_path = PathBuf::from("/repo/.hadron/nucleus/index.md");
        p.nucleus_notes_dir = PathBuf::from("/nonexistent/notes");
        let mut omitted = p.clone();
        omitted.nucleus_index.clear();
        omitted.nucleus_index_truncated = true;
        omitted.field_window.insert(
            0,
            Event::new(Actor::Gluon, None, Kind::Message { body: TRUNCATION_MARKER.to_string() }),
        );
        let base = crate::adapter::prompt::build(&omitted, &QuarkId::new("agy"));
        (p, base.len())
    }

    /// The regression: a 24 KiB Windows argv budget used to `clear()` the index and
    /// render "_Empty — nothing has been recorded here yet._". It must re-rank to a
    /// small slice that keeps the pinned lessons first.
    #[test]
    fn a_tight_budget_re_ranks_the_index_instead_of_dropping_it() {
        let (p, base) = tight_index_projection(1);
        let max = base + 3000;
        assert!(crate::adapter::prompt::build(&p, &QuarkId::new("agy")).len() > max, "precondition: over budget");

        let (fitted, prompt) = fit_projection_to_budget(&p, &QuarkId::new("agy"), max);
        assert!(prompt.len() <= max);
        assert!(prompt.contains("pinned-canary-0"), "pinned lesson must survive:\n{prompt}");
        assert!(fitted.nucleus_index_truncated, "a re-ranked index is reported as cut");
        let kept = prompt.matches("](notes/lesson-").count();
        assert!(kept > 0 && kept < 40, "index shrunk to ~{FIT_NUCLEUS_INDEX_BYTES} B, kept {kept}");
        assert!(!prompt.contains("nothing has been recorded here yet"));
        assert!(prompt.contains("SKILL-CANARY"), "the skill outlives the index");
    }

    /// When even the shrunk index cannot fit, the prompt must say it was OMITTED —
    /// never that nothing was ever recorded — and the active skill still survives.
    #[test]
    fn an_index_shed_for_budget_is_reported_as_omitted_not_empty() {
        let (p, base) = tight_index_projection(60);
        let max = base + 200;

        let (fitted, prompt) = fit_projection_to_budget(&p, &QuarkId::new("agy"), max);
        assert!(prompt.len() <= max);
        assert!(fitted.nucleus_index.is_empty());
        assert!(
            prompt.contains("_Omitted to fit the prompt budget — open `/repo/.hadron/nucleus/index.md` on demand._"),
            "must point at the index it omitted:\n{prompt}"
        );
        assert!(!prompt.contains("nothing has been recorded here yet"));
        assert!(prompt.contains("SKILL-CANARY"), "the skill is shed only after the index");
    }

    /// Invariants are trimmed on a line boundary: no rule is left cut mid-sentence.
    #[test]
    fn truncate_at_line_never_leaves_half_a_line() {
        let mut s = "alpha rule\nbeta rule\ngamma rule\n".to_string();
        truncate_at_line(&mut s, 15);
        assert_eq!(s, "alpha rule\n");
        let mut one = "no newline at all".to_string();
        truncate_at_line(&mut one, 5);
        assert_eq!(one, "no ne", "falls back to a char boundary");
    }

    /// Pipe transports (ACP, stdin CLIs) are fitted to `SAFE_PIPE_PROMPT_BYTES`, so a
    /// realistic 37 KB index and ~17 KB of invariants arrive intact — no 24 KiB
    /// starvation where there is no argv limit.
    #[test]
    fn the_pipe_budget_keeps_a_real_sized_index_and_full_invariants() {
        let mut p = projection_mode("fix bug", Mode::Bypass);
        p.invariants = format!("# The Standard Model\n{}INVARIANTS-TAIL\n", "- rule line here\n".repeat(1000));
        p.nucleus_index = big_index(1, 600);
        assert!(p.nucleus_index.len() > 37_000);
        p.nucleus_index_path = PathBuf::from("/repo/.hadron/nucleus/index.md");
        p.nucleus_notes_dir = PathBuf::from("/nonexistent/notes");

        let (fitted, prompt) = fit_projection_with_sliding_pruner(
            &p,
            &QuarkId::new("acp-agy"),
            SAFE_PIPE_PROMPT_BYTES,
            &crate::sliding_pruner::SlidingPrunerConfig::default(),
        );
        assert!(prompt.contains("INVARIANTS-TAIL"), "invariants must arrive whole");
        assert!(prompt.contains("pinned-canary-0"));
        assert!(!fitted.nucleus_index_truncated);
        assert!(!prompt.contains("Omitted to fit the prompt budget"));
    }

    /// An argv-bound prompt over `SAFE_ARG_BYTES` is spilled to
    /// `<hadron-dir>/prompts/<id>.md` with the FULL transcript, and the argv prompt
    /// stays under the OS limit while pointing at the file.
    #[tokio::test]
    async fn an_argv_overflow_spills_the_full_prompt_to_the_hadron_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let hadron_dir = tmp.path().join(".hadron");
        std::fs::create_dir_all(&hadron_dir).unwrap();

        let mut turn = projection("do task");
        turn.field_window = (0..80)
            .map(|i| {
                Event::new(Actor::Human, None, Kind::Message { body: format!("EVENT-{i:02} {}", "x".repeat(2000)) })
            })
            .collect();

        let mut q = CliQuark::new(
            QuarkId::new("cli-agy"),
            Flavor::Worker,
            "",
            CliSpec::agy(),
            FakeRunner::with_stdout(vec!["ok"]),
        )
        .with_prompt_cost_dir(hadron_dir.join("prompt-cost"));
        q.excite(turn.clone()).await.unwrap();

        let spill = hadron_dir.join("prompts").join("cli-agy.md");
        let full = std::fs::read_to_string(&spill).expect("prompt spilled to .hadron/prompts/<id>.md");
        assert!(full.contains("EVENT-00"), "the spill carries the oldest event");
        let recorded = q.runner.recorded.lock().unwrap();
        let argv_prompt = recorded[0].args.iter().max_by_key(|a| a.len()).unwrap();
        assert!(argv_prompt.len() <= SAFE_ARG_BYTES);
        assert!(argv_prompt.starts_with("# Read your full prompt first"));
        assert!(argv_prompt.contains(&spill.display().to_string()));
        assert!(!argv_prompt.contains("EVENT-00"), "argv prompt really was cut");
        drop(recorded);

        // No `.hadron` dir → no spill, plain argv fit.
        let mut bare = CliQuark::new(
            QuarkId::new("cli-agy"),
            Flavor::Worker,
            "",
            CliSpec::agy(),
            FakeRunner::with_stdout(vec!["ok"]),
        );
        bare.excite(turn).await.unwrap();
        let recorded = bare.runner.recorded.lock().unwrap();
        let argv_prompt = recorded[0].args.iter().max_by_key(|a| a.len()).unwrap();
        assert!(argv_prompt.len() <= SAFE_ARG_BYTES);
        assert!(!argv_prompt.contains("Read your full prompt first"));
    }

    #[tokio::test]
    async fn cli_quark_streaming_aborts_on_loop_watchdog_repeated_tool_calls() {
        let repeated_tool = r#"{"event":"step_update","step_update":{"state":"ACTIVE","tool_name":"run_command","tool_input":{"CommandLine":"cargo check"}}}"#;
        let stream_lines = vec![
            vec![
                repeated_tool.to_string(),
                repeated_tool.to_string(),
                repeated_tool.to_string(),
                repeated_tool.to_string(),
            ],
            vec![
                repeated_tool.to_string(),
                repeated_tool.to_string(),
                repeated_tool.to_string(),
                repeated_tool.to_string(),
            ],
            vec![
                repeated_tool.to_string(),
                repeated_tool.to_string(),
                repeated_tool.to_string(),
                repeated_tool.to_string(),
            ],
        ];
        let runner = FakeRunner::with_stream_lines(stream_lines);
        let mut spec = CliSpec::agy();
        spec.stream = Some(agy_stream_spec());
        let mut quark = CliQuark::new(
            QuarkId::new("cli-agy"),
            Flavor::Worker,
            "gemini-3.6-flash",
            spec,
            runner,
        );

        let err = quark.excite(projection("do task")).await.unwrap_err();
        assert!(
            err.to_string().contains("LoopWatchdog auto-diagnostic"),
            "expected LoopWatchdog abort, got: {err}"
        );
    }

    #[tokio::test]
    async fn cli_quark_fills_cancel_slot_and_interrupts_in_flight_turn_preserving_resident() {
        use crate::adapter::runner::CliResult;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Arc;

        struct BlockingFakeRunner {
            started: Arc<AtomicBool>,
            calls: Arc<AtomicUsize>,
        }

        #[async_trait]
        impl CliRunner for BlockingFakeRunner {
            async fn run(&self, _inv: CliInvocation) -> anyhow::Result<CliResult> {
                let n = self.calls.fetch_add(1, Ordering::SeqCst);
                self.started.store(true, Ordering::SeqCst);
                if n == 1 {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
                Ok(CliResult {
                    stdout: format!("completed call {n}"),
                    exit: 0,
                    usage: None,
                })
            }
            async fn run_streaming(
                &self,
                inv: CliInvocation,
                _on_line: &mut (dyn for<'a> FnMut(&'a str) -> bool + Send),
            ) -> anyhow::Result<CliResult> {
                self.run(inv).await
            }
        }

        let started = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut spec = CliSpec::agy();
        spec.stream = None;
        let mut quark = CliQuark::new(
            QuarkId::new("cli-agy"),
            Flavor::Orchestrator,
            "gemini-3.6-flash",
            spec,
            BlockingFakeRunner {
                started: started.clone(),
                calls: calls.clone(),
            },
        );
        let slot = crate::quark::CancelSlot::default();
        quark.attach_cancel_slot(slot.clone());
        assert!(!slot.is_filled(), "slot is empty when no turn is in flight");

        // Turn 1 completes normally and marks the quark resident.
        let out1 = quark.excite(projection("turn 1")).await.unwrap();
        assert!(!out1.cancelled);
        assert!(quark.resident, "turn 1 must mark CliQuark resident");
        assert!(!slot.is_filled(), "slot must be cleared after turn 1 returns");

        // Turn 2 blocks in-flight; CancelSlot interrupts it in <1ms while keeping resident == true.
        started.store(false, Ordering::SeqCst);
        let slot_for_cancel = slot.clone();
        let started_for_cancel = started.clone();
        let canceller = tokio::spawn(async move {
            while !started_for_cancel.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            assert!(slot_for_cancel.is_filled(), "CliQuark must fill CancelSlot while in flight");
            assert!(slot_for_cancel.request_cancel(), "request_cancel must return true");
        });

        let t0 = std::time::Instant::now();
        let out2 = quark.excite(projection("turn 2")).await.unwrap();
        canceller.await.unwrap();
        assert!(out2.cancelled, "cancelled turn must report outcome.cancelled == true");
        assert!(out2.message.is_none(), "cancelled turn must emit no message");
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(200),
            "CancelSlot must interrupt CliQuark immediately without waiting 30s (took {:?})",
            t0.elapsed()
        );
        assert!(
            quark.resident,
            "graceful cancel via CancelSlot must preserve CliQuark.resident so resumed turn uses --continue"
        );
        assert!(!slot.is_filled(), "CancelSlot must be cleared after cancelled turn returns");

        quark.reset_session();
        assert!(
            !quark.resident,
            "Quark::reset_session on CliQuark must clear resident so /clear, /reboot, and 70%/150k context rotation start a fresh conversation"
        );
    }

    #[tokio::test]
    async fn agy_claude_5_5_and_1m_claude_models_resolve_1m_context_window_without_clamping() {
        for model_name in ["claude-opus-5-5-high", "claude-sonnet-5-5-high"] {
            let lines = vec![vec![
                r#"{"event":"step_update","step_update":{"step_type":"tool","tool_name":"view_file","usage":{"used_tokens":217196,"input_tokens":217196,"output_tokens":1400,"cache_read_tokens":210000}}}"#.to_string(),
                r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"ok"}}"#.to_string(),
                r#"{"event":"result","result":{"response":"ok","usage":{"input_tokens":322213,"output_tokens":12400,"cache_read_tokens":13561400,"total_tokens":334613}}}"#.to_string(),
            ]];
            let runner = FakeRunner::with_stream_lines(lines);
            let mut spec = CliSpec::agy();
            spec.stream = Some(agy_stream_spec());
            let mut q = CliQuark::new(
                QuarkId::new("cli-agy"),
                Flavor::Worker,
                model_name,
                spec,
                runner,
            );
            let outcome = q.excite(projection("verify 1M context")).await.unwrap();
            let ctx = outcome.usage.context.expect("context should be populated");
            assert_eq!(ctx.used_tokens, 217_196, "{model_name} peak step input_tokens preserved");
            assert_eq!(
                ctx.context_window_size, 1_048_576,
                "{model_name} on agy must use 1_048_576 (1M) window, never 200_000"
            );
            assert!(
                (ctx.used_percentage - (217_196.0 / 1_048_576.0) * 100.0).abs() < 1e-4,
                "expected ~20.71%, got {}",
                ctx.used_percentage
            );
        }

        // Non-agy CLI defaults: legacy Claude stays 200k; 1M Claude 5.x / Opus 4.6+ get 1_000_000.
        assert_eq!(default_cli_context_window_size(false, Some("claude-3-5-sonnet-20241022")), 200_000);
        assert_eq!(default_cli_context_window_size(false, Some("claude-sonnet-4-5")), 200_000);
        assert_eq!(default_cli_context_window_size(false, Some("claude-sonnet-4-5[1m]")), 1_000_000);
        assert_eq!(default_cli_context_window_size(false, Some("claude-opus-4-6")), 1_000_000);
        assert_eq!(default_cli_context_window_size(false, Some("claude-opus-4-7")), 1_000_000);
        assert_eq!(default_cli_context_window_size(false, Some("claude-opus-4-8")), 1_000_000);
        assert_eq!(default_cli_context_window_size(false, Some("claude-opus-5-5-high")), 1_000_000);
        assert_eq!(default_cli_context_window_size(false, Some("claude-sonnet-5-5-high")), 1_000_000);
        assert_eq!(default_cli_context_window_size(true, Some("claude-3-5-sonnet")), 1_048_576);
    }

    #[tokio::test]
    async fn agy_cli_quark_injects_statusline_settings_ingests_capture_and_persists_quota() {
        use crate::adapter::runner::CliResult;
        use std::sync::{Arc, Mutex};

        struct StatuslineFakeRunner {
            recorded: Arc<Mutex<Vec<CliInvocation>>>,
        }

        #[async_trait]
        impl CliRunner for StatuslineFakeRunner {
            async fn run(&self, inv: CliInvocation) -> anyhow::Result<CliResult> {
                self.recorded.lock().unwrap().push(inv);
                Ok(CliResult {
                    stdout: "ok".to_string(),
                    exit: 0,
                    usage: None,
                })
            }

            async fn run_streaming(
                &self,
                inv: CliInvocation,
                on_line: &mut (dyn for<'a> FnMut(&'a str) -> bool + Send),
            ) -> anyhow::Result<CliResult> {
                let out_env = inv
                    .env
                    .iter()
                    .find(|(k, _)| k == crate::statusline::OUT_ENV)
                    .map(|(_, v)| PathBuf::from(v))
                    .expect("HADRON_STATUSLINE_OUT must be injected for agy seat");
                let raw_statusline = r#"{"cwd":"/home/user/project","session_id":"s1","conversation_id":"c1","model":{"id":"Claude Opus 5.5 (High)"},"context_window":{"total_input_tokens":180000,"total_output_tokens":4200,"context_window_size":1048576,"used_percentage":17.166},"quota":{"3p-5h":{"remaining_fraction":0.8125,"reset_time":"2026-11-12T21:00:00Z"},"3p-weekly":{"remaining_fraction":0.625,"reset_time":"2026-11-18T00:00:00Z"},"gemini-5h":{"remaining_fraction":0.95,"reset_time":"2026-11-12T20:00:00Z"},"gemini-weekly":{"remaining_fraction":0.90,"reset_time":"2026-11-19T00:00:00Z"}},"plan_tier":"Google AI Ultra","email":"secret.user@example.com"}"#;
                std::fs::write(&out_env, raw_statusline).expect("write per-turn statusline capture");

                on_line(r#"{"event":"step_update","step_update":{"step_type":"tool","tool_name":"view_file","usage":{"used_tokens":217196,"input_tokens":217196,"output_tokens":900,"cache_read_tokens":200000}}}"#);
                on_line(r#"{"event":"step_update","step_update":{"step_type":"agent_response","text_delta":"done"}}"#);
                on_line(r#"{"event":"result","result":{"response":"done","usage":{"input_tokens":250000,"output_tokens":3000,"cache_read_tokens":900000,"total_tokens":253000}}}"#);

                self.recorded.lock().unwrap().push(inv);
                Ok(CliResult {
                    stdout: "done".to_string(),
                    exit: 0,
                    usage: None,
                })
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let hadron_dir = tmp.path().join(".hadron");
        std::fs::create_dir_all(hadron_dir.join("live")).unwrap();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let id = QuarkId::new("cli-agy");
        let mut spec = CliSpec::agy();
        spec.stream = Some(agy_stream_spec());
        let mut q = CliQuark::new(
            id.clone(),
            Flavor::Worker,
            "",
            spec,
            StatuslineFakeRunner {
                recorded: recorded.clone(),
            },
        )
        .watching(hadron_dir.join("live"));

        let mut turn = projection("run turn with statusline");
        turn.cwd = tmp.path().to_path_buf();
        let outcome = q.excite(turn).await.unwrap();

        let invs = recorded.lock().unwrap();
        assert_eq!(invs.len(), 1);
        let settings_pos = invs[0]
            .args
            .iter()
            .position(|a| a == "--settings")
            .expect("--settings flag must be appended");
        let settings_val: serde_json::Value =
            serde_json::from_str(&invs[0].args[settings_pos + 1]).expect("valid --settings JSON");
        assert_eq!(settings_val["statusLine"]["type"], "command");
        assert_eq!(settings_val["statusLine"]["enabled"], true);
        assert!(!settings_val["statusLine"]["command"].as_str().unwrap().is_empty());

        assert_eq!(outcome.usage.model.as_deref(), Some("Claude Opus 5.5 (High)"));
        let ctx = outcome.usage.context.as_ref().expect("context populated");
        assert_eq!(ctx.used_tokens, 217_196, "StreamResult peak step used_tokens preserved");
        assert_eq!(ctx.context_window_size, 1_048_576);
        assert!((ctx.used_percentage - (217_196.0 / 1_048_576.0) * 100.0).abs() < 1e-4);

        assert_eq!(outcome.usage.quota.len(), 4);
        assert!(outcome.usage.quota.iter().any(|b| b.key == "3p-5h" && (b.remaining_fraction - 0.8125).abs() < 1e-6));
        assert!(outcome.usage.quota.iter().any(|b| b.key == "gemini-5h" && (b.remaining_fraction - 0.95).abs() < 1e-6));

        let saved_quota = hadron_lattice::quota::load(&hadron_dir.join("quota"), &id);
        assert_eq!(saved_quota, outcome.usage.quota, "<hadron_dir>/quota/<id>.json persisted");

        let quota_raw = std::fs::read_to_string(hadron_dir.join("quota").join("cli-agy.json")).unwrap();
        let outcome_dbg = format!("{outcome:?}");
        for text in [&quota_raw, &outcome_dbg] {
            assert!(!text.contains("secret.user@example.com"), "privacy: email must never leak: {text}");
            assert!(!text.contains('@'), "privacy: @ must never leak: {text}");
            assert!(!text.contains("Ultra"), "privacy: plan_tier must never leak: {text}");
        }

        // Verify WSL UNC path translation and POSIX statusline command selection.
        let wsl_cwd = std::path::Path::new(r"\\wsl.localhost\Ubuntu\home\user\project\.hadron\trees\cli-agy");
        let wsl_out = std::path::Path::new(r"\\wsl.localhost\Ubuntu\home\user\project\.hadron\statusline\cli-agy.json");
        assert_eq!(
            translate_statusline_out_for_child(wsl_out, wsl_cwd),
            "/home/user/project/.hadron/statusline/cli-agy.json"
        );
        assert!(resolve_statusline_command(wsl_cwd).starts_with("sh -c "));
    }
}
