use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hadron_lattice::{
    live, Activity, ContextUsage, Doing, Mode, Projection, QuarkId, QuotaBucket, TurnOutcome, Usage,
};

use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, McpServer, McpServerStdio, NewSessionRequest, PermissionOptionKind, PlanEntryStatus,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, SetSessionConfigOptionRequest,
    StopReason, TextContent, ToolCall, ToolCallContent, ToolCallUpdate, ToolKind, Usage as AcpUsage,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};

use crate::adapter::registry::AcpTarget;

use super::model::{effort_selector, mode_selector, model_selector, permission_choice, resolve_model};
use super::spend::turn_spend;

/// Build the ACP JSON stdio descriptor `AcpAgent::from_str` parses (see the crate
/// doc example on `AcpAgent`), with a resolved secret env baked into its `env`
/// array. `agent_client_protocol::AcpAgent::spawn_process` applies each entry via
/// `cmd.env(name, value)` before spawning (`agent-client-protocol` 1.2.0,
/// `acp_agent.rs:185-187`) — this is how a secret VALUE reaches the ACP transport
/// without ever riding on argv or a bare command string.
///
/// Pure and side-effect-free on purpose: no process is spawned here, so it is
/// unit-testable without touching a real agent.
pub(super) fn acp_stdio_descriptor(program: &str, args: &[String], env: &[(String, String)]) -> String {
    let env_json: Vec<serde_json::Value> = env
        .iter()
        .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
        .collect();
    serde_json::json!({
        "type": "stdio",
        // The descriptor requires a human-readable name; the program itself is a
        // fine one and carries no secret.
        "name": program,
        "command": program,
        "args": args,
        "env": env_json,
    })
    .to_string()
}

/// Parse the vendor-namespaced Claude rate-limit payload `claude-agent-acp` attaches
/// to a `usage_update`'s `_meta` (`_claude/rateLimit`), mapped straight from Claude
/// Code's own `rate_limit_event`. `utilization` is the provider's **own** figure —
/// exactly the number Option A (recomputing locally from `ledger.db`) could never
/// match, since server-side limits move with load
/// (`.hadron/docs/plans/2026-07-24-claude-plan-limits-in-stats.md`). Absent or
/// malformed `_meta` returns `None` — absent is not zero (see `Usage::quota`'s
/// honesty rule: empty means the provider reported nothing, not that it is full).
///
/// `rateLimitType` is optional in the schema; when absent, key falls back to
/// `claude-limit`. `utilization` is strictly required: a bucket with no fraction has
/// no "% left" to show, and inventing one breaks `Usage::quota`'s honesty rule.
pub(super) fn parse_claude_rate_limit(meta: &serde_json::Map<String, serde_json::Value>) -> Option<QuotaBucket> {
    let rl = meta.get("_claude/rateLimit")?.as_object()?;
    let rate_limit_type = rl.get("rateLimitType").and_then(|v| v.as_str()).unwrap_or("limit");
    let utilization = rl.get("utilization")?.as_f64()?;
    let reset_time = rl
        .get("resetsAt")
        .and_then(|v| v.as_i64())
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0));
    Some(QuotaBucket {
        key: format!("claude-{rate_limit_type}"),
        remaining_fraction: (1.0 - utilization).clamp(0.0, 1.0),
        reset_time,
    })
}

/// What a fresh session's quota accumulator starts from: whatever was last
/// persisted for this quark (see `hadron_lattice::quota`), so a resident agent
/// that re-boots — or a `/clear`, which drops the session outright — does not
/// start blind to a still-valid window. `dir` is `None` for a caller with no
/// field on disk (tests, and any quark this daemon is not watching), matching
/// `LiveFeed`'s existing convention; that yields an empty accumulator, same as
/// a quark that has never reported quota.
pub(super) fn seed_quota(dir: Option<&std::path::Path>, quark: &QuarkId) -> Vec<QuotaBucket> {
    dir.map(|d| hadron_lattice::quota::load(d, quark)).unwrap_or_default()
}

/// Merge a freshly-seen bucket into the accumulator — refining an existing
/// bucket of the same key rather than duplicating it, same rule the pump
/// already followed — and persist the result so the *next* boot can see it.
/// Best-effort: a failed write must never fail a turn, which is why this
/// returns nothing to check.
pub(super) fn merge_and_persist_bucket(
    quota: &mut Vec<QuotaBucket>,
    bucket: QuotaBucket,
    dir: Option<&std::path::Path>,
    quark: &QuarkId,
) {
    match quota.iter_mut().find(|b| b.key == bucket.key) {
        Some(existing) => *existing = bucket,
        None => quota.push(bucket),
    }
    if let Some(d) = dir {
        let _ = hadron_lattice::quota::save(d, quark, quota);
    }
}

/// The one-line "what is it doing" a human reads mid-turn. An agent's bare
/// `title` is often just the tool's name ("Terminal", "Write"), so enrich it:
/// the kind's verb plus the file being touched, or the actual command line for
/// a shell call, falling back to the title when the call carries nothing richer.
pub(super) fn tool_call_detail(call: &ToolCall) -> String {
    let verb = match call.kind {
        ToolKind::Read => Some("Reading"),
        ToolKind::Edit => Some("Editing"),
        ToolKind::Delete => Some("Deleting"),
        ToolKind::Move => Some("Moving"),
        ToolKind::Search => Some("Searching"),
        ToolKind::Execute => Some("Running"),
        ToolKind::Think => Some("Thinking"),
        ToolKind::Fetch => Some("Fetching"),
        _ => None,
    };
    // The file a follow-along client would jump to: the call's first location.
    let target = call.locations.first().map(|loc| {
        loc.path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| loc.path.display().to_string())
    });
    // A shell call's command line beats its generic "Terminal" title.
    let command = matches!(call.kind, ToolKind::Execute)
        .then(|| call.raw_input.as_ref())
        .flatten()
        .and_then(|input| input.get("command"))
        .and_then(|cmd| cmd.as_str())
        .map(first_line_truncated);
    match (verb, target, command) {
        (Some(verb), Some(target), _) => format!("{verb} {target}"),
        (Some(verb), None, Some(command)) => format!("{verb} `{command}`"),
        _ => call.title.clone(),
    }
}

/// What a tool-call **update** adds beyond its opening title: the actual output
/// as it streams in (a shell command's stdout, a file diff) beats a static verb
/// that was already published when the call started. `None` means this update
/// carries nothing new to show (a bare status flip with no content or title) —
/// the caller should leave the previous detail on screen rather than blank it.
pub(super) fn tool_call_update_detail(update: &ToolCallUpdate) -> Option<String> {
    if let Some(last) = update.fields.content.as_ref().and_then(|c| c.last()) {
        match last {
            ToolCallContent::Content(c) => {
                if let ContentBlock::Text(t) = &c.content {
                    return Some(t.text.clone());
                }
            }
            ToolCallContent::Diff(d) => {
                let name = d
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| d.path.display().to_string());
                return Some(format!("Edited {name}"));
            }
            // `Terminal` (an embedded terminal reference) and any future
            // variant carry nothing renderable here without a further lookup.
            _ => {}
        }
    }
    update.fields.title.clone()
}

/// First line only, cut at 80 **chars** (never bytes — a mid-character byte cut
/// panics the chamber's label rendering).
fn first_line_truncated(s: &str) -> String {
    const MAX: usize = 80;
    let line = s.lines().next().unwrap_or("");
    if line.chars().count() > MAX {
        let cut: String = line.chars().take(MAX).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

/// plan." then, later, the final "@orchestrator Task 2 complete..." report).
/// The old code `push_str`'d every notification straight onto the last with no
/// separator, so a report's leading `@mention` landed mid-line, glued onto the
/// previous notification's final word (`"Committing now.@orchestrator ..."`).
/// `parse_all_addressees`/`parse_addressee` (router/mod.rs) only recognize a
/// mention that **starts a line** — by design, so a quark quoting another
/// quark's handle in prose doesn't spuriously excite it (see
/// `orchestrator_alias_does_not_name_an_unrelated_card` and friends) — so a
/// glued-on mention silently routed to nobody and the orchestrator was never
/// dispatched. A blank line between notifications restores the line-start
/// Appends a streaming message chunk to an ACP session's transcript.
///
/// Ensures that if a chunk starts with an `@` mention, it begins on a new line
/// so `parse_all_addressees` line-start mention routing succeeds, while
/// preserving continuous streaming text without inserting spurious mid-sentence
/// or mid-word newlines.
pub(super) fn append_message_chunk(transcript: &mut String, chunk: &str) {
    if !transcript.is_empty() && chunk.starts_with('@') && !transcript.ends_with('\n') {
        transcript.push('\n');
    }
    transcript.push_str(chunk);
}

/// One turn, handed to the resident pump.
pub(super) struct TurnRequest {
    prompt: String,
    reply: tokio::sync::oneshot::Sender<anyhow::Result<TurnReply>>,
}

/// What the pump got back from one `session/prompt`.
pub(super) struct TurnReply {
    text: String,
    /// The end-turn token usage, when the agent implements the (still unstable)
    /// `unstable_end_turn_token_usage` extension. `None` if it does not.
    usage: Option<AcpUsage>,
    /// The last `usage_update` seen during the turn: context used / window size.
    context: Option<(u64, u64)>,
    /// Every Claude rate-limit bucket seen during the turn (vendor `_meta`, keyed by
    /// `rateLimitType` so a later `usage_update` refines its own bucket rather than
    /// appending a duplicate). Empty for every non-Claude ACP agent.
    quota: Vec<QuotaBucket>,
    stop: StopReason,
}

/// The live connection: a handle onto the pump thread. Dropping it drops the
/// channel, which ends the pump's loop, which tears down the connection and reaps
/// the agent subprocess.
pub(super) struct AcpSession {
    pub(super) turns: tokio::sync::mpsc::UnboundedSender<TurnRequest>,
    /// The permission posture the pump should apply, swapped in before each turn.
    /// Shared because ACP's `session/request_permission` arrives on the *connection*,
    /// not on the turn, so the handler needs a way to see the current turn's mode.
    pub(super) mode: Arc<Mutex<Mode>>,
    /// The model the agent is **actually** running, as the agent itself reported it —
    /// not the one the seat asked for. `None` means the agent advertised no selector,
    /// so we genuinely do not know. Absent is not "the default"; it is unknown.
    pub(super) model: Arc<Mutex<Option<String>>>,
}

/// A quark backed by a resident ACP agent.
/// Publishes what this quark is doing, mid-turn, for the chamber to render.
///
/// Cheap to clone (it is moved into the ACP notification handler, which the SDK
/// drives on its own thread) and **throttled**: an agent emits a thought chunk
/// every few tokens, and a file write per token would be a hot loop that helps
/// nobody read faster.
#[derive(Clone)]
pub(super) struct LiveFeed {
    pub(super) dir: PathBuf,
    pub(super) quark: QuarkId,
    pub(super) last: Arc<Mutex<Option<Instant>>>,
    pub(super) active: Arc<std::sync::atomic::AtomicBool>,
}

impl LiveFeed {
    /// The minimum gap between two published activities. A tool call ignores it —
    /// it is the one update the human is actually reading.
    const THROTTLE: std::time::Duration = std::time::Duration::from_millis(200);

    pub(super) fn set_active(&self, active: bool) {
        self.active.store(active, std::sync::atomic::Ordering::Relaxed);
    }

    fn publish(&self, doing: Doing, detail: &str) {
        if !self.active.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let forced = matches!(doing, Doing::Working | Doing::Planning);
        {
            let mut last = self.last.lock().unwrap();
            let now = Instant::now();
            match *last {
                Some(t) if !forced && now.duration_since(t) < Self::THROTTLE => return,
                _ => *last = Some(now),
            }
        }
        // A failed publish must never kill a turn: this is a view, not the record.
        let _ = live::publish(&self.dir, &Activity::new(self.quark.clone(), doing, detail));
    }

    pub(super) fn clear(&self) {
        let _ = live::clear(&self.dir, &self.quark);
    }
}

impl super::AcpQuark {
    /// Boot the agent and open one session in `cwd`. Blocks until the agent has
    /// answered `initialize` and `session/new`, so a boot failure (missing `npx`, a
    /// dead adapter, an unauthenticated CLI) surfaces as a failed turn rather than a
    /// silent hang.
    ///
    /// `cwd` is the quark's own worktree, straight off the `Projection` — ACP's
    /// `session/new` takes a required `cwd`, so hadron's existing cwd chain lands on
    /// the protocol with no adaptation.
    pub(super) fn boot(
        quark: QuarkId,
        target: &AcpTarget,
        cwd: PathBuf,
        want_model: String,
        want_effort: Option<String>,
        want_mode: Option<String>,
        live: Option<LiveFeed>,
        quota_dir: Option<PathBuf>,
        env: Vec<(String, String)>,
    ) -> anyhow::Result<AcpSession> {
        let (turns_tx, mut turns_rx) = tokio::sync::mpsc::unbounded_channel::<TurnRequest>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();

        let mode = Arc::new(Mutex::new(Mode::default()));
        let handler_mode = Arc::clone(&mode);

        // What the agent says it is running, written once at boot by the pump.
        let model = Arc::new(Mutex::new(None::<String>));
        let pump_model = Arc::clone(&model);

        // `display_command` is what ever appears in a diagnostic — the bare command
        // line, no secrets. `agent_source` is the JSON stdio descriptor actually
        // handed to `AcpAgent::from_str`, which is the only place `env`'s resolved
        // VALUES go: they must never end up in an error string, a log line, or
        // anything else that could surface them. An empty `env` produces a
        // `{"env": []}` descriptor, which `AcpAgent`'s stdio spawn treats identically
        // to the old bare-command path (see `acp_stdio_descriptor_no_env_matches_bare_command`).
        let display_command = target.command_line();
        // The shared build env goes in ahead of the seat's own: an ACP agent runs
        // `cargo` in its worktree exactly like a CLI quark does, and without this it
        // grows a duplicate 37 GB `target/` there (`worktree::shared_build_env`).
        // Seat env last, so a seat that sets one of these deliberately still wins.
        let mut spawn_env = crate::worktree::shared_build_env(&cwd);
        spawn_env.extend(env.iter().cloned());
        let agent_source = acp_stdio_descriptor(&target.program, &target.args, &spawn_env);
        // The reply accumulator and the context watermark are written by the
        // notification handler (which the SDK drives on the connection) and read by
        // the turn pump. Hence the Arcs.
        let transcript = Arc::new(Mutex::new(String::new()));
        let context = Arc::new(Mutex::new(None::<(u64, u64)>));
        let quota = Arc::new(Mutex::new(seed_quota(quota_dir.as_deref(), &quark)));
        let pump_transcript = Arc::clone(&transcript);
        let pump_context = Arc::clone(&context);
        let pump_quota = Arc::clone(&quota);

        // One handle for the happy path (moved into the pump, fired once the session
        // opens) and one for the failure path (kept out here, fired if the pump dies
        // before it ever gets that far).
        let boot_tx = ready_tx.clone();

        std::thread::Builder::new()
            .name("hadron-acp".to_string())
            .spawn(move || {
                let outcome: anyhow::Result<()> = futures::executor::block_on(async move {
                    // NEVER format `agent_source` into an error: it carries the
                    // resolved secret values. `display_command` is the safe stand-in.
                    let agent = AcpAgent::from_str(&agent_source)
                        .map_err(|e| anyhow::anyhow!("bad ACP command {display_command:?}: {e}"))?;

                    let in_turn = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let pump_in_turn = Arc::clone(&in_turn);

                    let connect = agent_client_protocol::Client
                        .builder()
                        .name("hadron")
                        .on_receive_notification(
                            {
                                let in_turn = Arc::clone(&in_turn);
                                async move |n: SessionNotification, _cx| {
                                    if !in_turn.load(std::sync::atomic::Ordering::Relaxed) {
                                        return Ok(());
                                    }
                                    match n.update {
                                        // The reply text. `PromptResponse` carries none,
                                        // so this is the only place a message exists.
                                        SessionUpdate::AgentMessageChunk(chunk) => {
                                            if let ContentBlock::Text(t) = chunk.content {
                                                append_message_chunk(
                                                    &mut transcript.lock().unwrap(),
                                                    &t.text,
                                                );
                                            }
                                        }
                                        // Real context numbers, including the window SIZE
                                        // — which the claude CLI never reports.
                                        SessionUpdate::UsageUpdate(u) => {
                                            *context.lock().unwrap() = Some((u.used, u.size));
                                            if let Some(bucket) =
                                                u.meta.as_ref().and_then(parse_claude_rate_limit)
                                            {
                                                merge_and_persist_bucket(
                                                    &mut quota.lock().unwrap(),
                                                    bucket,
                                                    quota_dir.as_deref(),
                                                    &quark,
                                                );
                                            }
                                        }
                                        // The agent's reasoning, streamed. Volatile: it
                                        // is published for the chamber to render live and
                                        // never written to the field.
                                        SessionUpdate::AgentThoughtChunk(chunk) => {
                                            if let (Some(feed), ContentBlock::Text(t)) =
                                                (&live, chunk.content)
                                            {
                                                feed.publish(Doing::Thinking, &t.text);
                                            }
                                        }
                                        // A tool call is the update the human is actually
                                        // reading — "what is it DOING" — so it is never
                                        // throttled away.
                                        SessionUpdate::ToolCall(call) => {
                                            if let Some(feed) = &live {
                                                feed.publish(Doing::Working, &tool_call_detail(&call));
                                            }
                                        }
                                        // A tool call's own output as it runs — the tail of a
                                        // shell command's stdout, a diff summary — is richer
                                        // than the static title `ToolCall` published at start.
                                        SessionUpdate::ToolCallUpdate(update) => {
                                            if let (Some(feed), Some(detail)) =
                                                (&live, tool_call_update_detail(&update))
                                            {
                                                feed.publish(Doing::Working, &detail);
                                            }
                                        }
                                        SessionUpdate::Plan(plan) => {
                                            if let Some(feed) = &live {
                                                let step = plan
                                                    .entries
                                                    .iter()
                                                    .find(|e| {
                                                        e.status == PlanEntryStatus::InProgress
                                                    })
                                                    .or_else(|| plan.entries.first());
                                                if let Some(step) = step {
                                                    feed.publish(Doing::Planning, &step.content);
                                                }
                                            }
                                        }
                                        // Everything else (user echoes, tool-call updates,
                                        // command lists, mode changes) is protocol
                                        // bookkeeping with nothing for a human to read.
                                        _ => {}
                                    }
                                    Ok(())
                                }
                            },
                            agent_client_protocol::on_receive_notification!(),
                        )
                        .on_receive_request(
                            async move |req: RequestPermissionRequest, responder, _cx| {
                                let chosen = if is_native_edit_request(&req) {
                                    req.options
                                        .iter()
                                        .find(|o| o.kind == PermissionOptionKind::RejectOnce)
                                        .map(|o| o.option_id.clone())
                                } else {
                                    let want = permission_choice(*handler_mode.lock().unwrap());
                                    req.options
                                        .iter()
                                        .find(|o| o.kind == want)
                                        .or_else(|| {
                                            req.options
                                                .iter()
                                                .find(|o| o.kind == PermissionOptionKind::RejectOnce)
                                        })
                                        .map(|o| o.option_id.clone())
                                };

                                match chosen {
                                    Some(id) => responder.respond(RequestPermissionResponse::new(
                                        RequestPermissionOutcome::Selected(
                                            SelectedPermissionOutcome::new(id),
                                        ),
                                    )),
                                    None => responder.respond(RequestPermissionResponse::new(
                                        RequestPermissionOutcome::Cancelled,
                                    )),
                                }
                            },
                            agent_client_protocol::on_receive_request!(),
                        )
                        .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
                            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                                .block_task()
                                .await?;

                            let forge_exe = std::env::current_exe()
                                .ok()
                                .and_then(|p| p.parent().map(|d| d.join("hadron-forge-mcp")))
                                .unwrap_or_else(|| PathBuf::from("hadron-forge-mcp"));

                            let mcp_servers = vec![
                                McpServer::Stdio(
                                    McpServerStdio::new("hadron-forge-mcp", forge_exe)
                                        .args(vec![cwd.to_string_lossy().to_string()]),
                                ),
                                McpServer::Stdio(
                                    McpServerStdio::new("context7", "npx")
                                        .args(vec!["-y".to_string(), "@context7/mcp".to_string()]),
                                ),
                            ];

                            let session = cx
                                .send_request(NewSessionRequest::new(cwd).mcp_servers(mcp_servers))
                                .block_task()
                                .await?;
                            let sid = session.session_id;

                            // THE MODEL PICKER. The agent volunteers its models here, in
                            // `config_options`, and Hadron used to drop them on the floor.
                            // Ask for the seat's model if the agent offers one that matches.
                            //
                            // A model we cannot find is NOT fatal: the agent has its own
                            // default and a turn on the wrong model beats no turn at all.
                            // But it must be loud, or the roster will claim a model that
                            // never ran.
                            let offered = session.config_options.unwrap_or_default();
                            match model_selector(&offered) {
                                Some(selector) => {
                                    match resolve_model(&selector, &want_model) {
                                        Some(value) => {
                                            let set = cx
                                                .send_request(SetSessionConfigOptionRequest::new(
                                                    sid.clone(),
                                                    selector.config_id.clone(),
                                                    value.as_str(),
                                                ))
                                                .block_task()
                                                .await;
                                            match set {
                                                Ok(_) => {
                                                    *pump_model.lock().unwrap() = Some(value.clone());
                                                    eprintln!(
                                                        "[acp] model set to {value:?} (asked for {want_model:?})"
                                                    );
                                                }
                                                Err(e) => eprintln!(
                                                    "[acp] the agent refused model {value:?}: {e} — \
                                                     staying on its default {:?}",
                                                    selector.current
                                                ),
                                            }
                                        }
                                        None => {
                                            // Either the seat asked for nothing, or it asked
                                            // for the model already running, or it asked for
                                            // one this agent does not have.
                                            *pump_model.lock().unwrap() =
                                                Some(selector.current.clone());
                                            if !want_model.trim().is_empty()
                                                && !selector
                                                    .available
                                                    .iter()
                                                    .any(|m| m.value == selector.current
                                                        && (m.value.eq_ignore_ascii_case(&want_model)
                                                            || m.label
                                                                .eq_ignore_ascii_case(&want_model)))
                                            {
                                                eprintln!(
                                                    "[acp] seat asked for model {want_model:?}; \
                                                     agent runs {:?} and offers {:?}",
                                                    selector.current,
                                                    selector
                                                        .available
                                                        .iter()
                                                        .map(|m| &m.value)
                                                        .collect::<Vec<_>>()
                                                );
                                            }
                                        }
                                    }
                                }
                                None => {
                                    if !want_model.trim().is_empty() {
                                        eprintln!(
                                            "[acp] seat asked for model {want_model:?} but this \
                                             agent advertises no model selector"
                                        );
                                    }
                                }
                            }

                            if let Some(want_eff) = want_effort {
                                if let Some(selector) = effort_selector(&offered) {
                                    if let Some(value) = resolve_model(&selector, &want_eff) {
                                        let _ = cx.send_request(SetSessionConfigOptionRequest::new(
                                            sid.clone(),
                                            selector.config_id,
                                            value.as_str(),
                                        )).block_task().await;
                                        eprintln!("[acp] effort set to {:?}", value);
                                    }
                                }
                            }

                            if let Some(want_m) = want_mode {
                                if let Some(selector) = mode_selector(&offered) {
                                    if let Some(value) = resolve_model(&selector, &want_m) {
                                        let _ = cx.send_request(SetSessionConfigOptionRequest::new(
                                            sid.clone(),
                                            selector.config_id,
                                            value.as_str(),
                                        )).block_task().await;
                                        eprintln!("[acp] mode set to {:?}", value);
                                    }
                                }
                            }

                            // The agent is up and has a session. Unblock `boot`.
                            let _ = ready_tx.send(Ok(()));

                            // THE PUMP. This is what makes the session resident: the
                            // connection stays open across turns, and each turn is
                            // just another `session/prompt` on the same `sid`.
                            while let Some(turn) = turns_rx.recv().await {
                                pump_transcript.lock().unwrap().clear();
                                *pump_context.lock().unwrap() = None;
                                pump_in_turn.store(true, std::sync::atomic::Ordering::Relaxed);

                                let sent = cx
                                    .send_request(PromptRequest::new(
                                        sid.clone(),
                                        vec![ContentBlock::Text(TextContent::new(turn.prompt))],
                                    ))
                                    .block_task()
                                    .await;

                                pump_in_turn.store(false, std::sync::atomic::Ordering::Relaxed);

                                let reply = match sent {
                                    Ok(resp) => Ok(TurnReply {
                                        text: pump_transcript.lock().unwrap().clone(),
                                        usage: resp.usage.clone(),
                                        context: *pump_context.lock().unwrap(),
                                        quota: pump_quota.lock().unwrap().clone(),
                                        stop: resp.stop_reason,
                                    }),
                                    Err(e) => Err(anyhow::anyhow!("session/prompt failed: {e}")),
                                };
                                // A dropped receiver means the engine gave up on this
                                // turn. Not fatal — keep the session for the next one.
                                let _ = turn.reply.send(reply);
                            }
                            Ok(())
                        });

                    connect
                        .await
                        .map_err(|e| anyhow::anyhow!("ACP connection failed: {e}"))
                });

                // If we died before `session/new` landed, `boot` is still parked on
                // `ready_rx`. Hand it the reason instead of letting it block forever.
                if let Err(e) = outcome {
                    let _ = boot_tx.send(Err(e));
                }
            })
            .map_err(|e| anyhow::anyhow!("failed to spawn the ACP pump thread: {e}"))?;

        // `recv` errors only if the thread died without reporting — surface that as a
        // boot failure rather than hanging.
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(AcpSession { turns: turns_tx, mode, model }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(anyhow::anyhow!(
                "the ACP agent ({}) exited before opening a session",
                target.command_line()
            )),
        }
    }

    pub(super) async fn run_turn(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let mode = turn.mode;
        let prompt = crate::adapter::prompt::build(&turn, &self.id);
        // Best-effort, same as quota's persistence right below: a failed write must
        // never fail a turn.
        if let Some(dir) = &self.prompt_cost_dir {
            let breakdown = crate::adapter::prompt::measure(&turn, &self.id);
            let _ = hadron_lattice::prompt_cost::save(dir, &self.id, &breakdown);
        }

        // If the chat history has been cleared/reset, discard the resident session so the agent boots fresh.
        if turn.field_window.is_empty() {
            self.session = None;
        }

        // Boot on the first turn, in the quark's own worktree, and keep it.
        if self.session.is_none() {
            self.session = Some(Self::boot(
                self.id.clone(),
                &self.target,
                turn.cwd.clone(),
                self.model.clone(),
                self.effort.clone(),
                self.mode_config.clone(),
                self.live.clone(),
                self.quota_dir.clone(),
                self.env.0.clone(),
            )?);
        }
        let session = self.session.as_ref().expect("just booted");

        // The posture the permission handler will apply for this turn.
        *session.mode.lock().unwrap() = mode;

        // A resident agent that dies must not wedge the quark forever. Both failure
        // paths below mean the pump is gone, so drop the session: the NEXT turn then
        // finds `None` and boots a fresh agent, instead of every later `excite`
        // erroring on a dead channel for the life of the daemon. The conversation is
        // lost (it lived in the agent), but the quark recovers — which is exactly
        // what the one-shot CLI path gets for free by spawning per turn.
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if session.turns.send(TurnRequest { prompt, reply: reply_tx }).is_err() {
            self.session = None;
            anyhow::bail!("the ACP agent's session is gone (it will re-boot on the next turn)");
        }

        let reply = match reply_rx.await {
            Ok(reply) => reply,
            Err(_) => {
                self.session = None;
                anyhow::bail!("the ACP agent died mid-turn (it will re-boot on the next turn)");
            }
        };
        // If session/prompt failed (e.g. WebSocket connection closed or agent exception),
        // drop the session so the next turn re-boots a fresh session instead of stranding the seat.
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                self.session = None;
                return Err(e);
            }
        };

        // Per-turn spend, by component, from the cumulative counters.
        let (spend, new_watermark) = turn_spend(self.last_spend, reply.usage.as_ref());
        self.last_spend = new_watermark;

        // Context, when the agent sent a `usage_update`. NOTE the honesty rule this
        // codebase already enforces (see `telemetry.rs`): most ACP agents have **no
        // quota concept at all**, so `quota` stays empty rather than claiming a full
        // budget. Claude's is the one exception — `reply.quota` carries whatever
        // vendor `_meta` buckets `parse_claude_rate_limit` found this turn, still
        // empty for every other agent. `used_percentage` is computed here only
        // because ACP — unlike agy — does not send one; `size` is the agent's own
        // reported window, never invented.
        let usage = Usage {
            spend,
            context: reply.context.map(|(used, size)| ContextUsage {
                used_tokens: used.min(u32::MAX as u64) as u32,
                context_window_size: size.min(u32::MAX as u64) as u32,
                used_percentage: if size > 0 { (used as f64 / size as f64) * 100.0 } else { 0.0 },
            }),
            model: self.session.as_ref().and_then(|s| s.model.lock().unwrap().clone()),
            quota: reply.quota,
        };

        // A refusal or a token wall is a real thing the field should see; a plain
        // `end_turn` with no text is just a silent turn.
        let text = reply.text.trim();
        let message = match reply.stop {
            StopReason::Cancelled => None,
            StopReason::EndTurn if text.is_empty() => None,
            StopReason::EndTurn => Some(text.to_string()),
            other => {
                let note = format!("[acp: stopped on {other:?}]");
                Some(if text.is_empty() { note } else { format!("{text}\n\n{note}") })
            }
        };

        Ok(TurnOutcome { message, permission: None, usage })
    }
}

pub fn is_native_edit_request(req: &RequestPermissionRequest) -> bool {
    let fields = &req.tool_call.fields;
    if matches!(fields.kind, Some(ToolKind::Edit)) {
        return true;
    }
    if let Some(title) = &fields.title {
        let name = title.trim();
        if matches!(
            name,
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "fs/write_text_file" | "write_file"
        ) {
            return true;
        }
    }
    false
}
