//! The ACP transport: a quark backed by a **resident** agent subprocess speaking
//! the Agent Client Protocol (JSON-RPC 2.0 over stdio).
//!
//! Hadron is the ACP **client**; the seated agent is the ACP **agent**. Where the
//! CLI adapters spawn a fresh process per turn and re-send the whole conversation
//! through argv/stdin, this boots the agent **once** and holds the connection: the
//! agent keeps the conversation, and a turn is a `session/prompt` on an existing
//! session.
//!
//! ## Why a thread, not a task
//!
//! The SDK's connection API is *scoped*: `connect_with(transport, |cx| async { … })`
//! runs the connection for exactly as long as its closure. That is a fine shape for
//! a one-shot client and the wrong shape for a `Quark`, whose `excite` is called
//! once per turn over minutes. So the closure is inverted into a **turn pump**: it
//! parks on an mpsc of turn requests and only returns when the channel is dropped,
//! which is what makes the session resident.
//!
//! The SDK is built on the `futures`/`async-process`/`blocking` stack rather than
//! tokio, so the pump gets a dedicated OS thread driven by `futures::executor::block_on`
//! instead of a `tokio::spawn`. `tokio::sync`'s channels are runtime-agnostic, so
//! they bridge the two sides cleanly. One thread per ACP quark, parked on a channel.
//!
//! ## What the agent tells us, and what we do with it
//!
//! - `session/update` → `agent_message_chunk`: the **only** place the reply text
//!   lives (`PromptResponse` carries no content), so we accumulate it. This is not
//!   the streaming feature — nothing is surfaced mid-turn; `excite` still returns
//!   once. It is simply how ACP hands over a message.
//! - `session/update` → `usage_update` `{ used, size }`: context tokens and the
//!   model's **real** window size. Straight into [`ContextUsage`].
//! - the `session/prompt` response → `usage` (feature `unstable_end_turn_token_usage`):
//!   cumulative token totals for the session. The per-turn cost is the **delta**.
//!   See [`turn_spend`].

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use hadron_lattice::{
    live, Activity, ContextUsage, Doing, EnergyState, Flavor, Mode, Projection, QuarkId,
    TokenSpend, TurnOutcome, Usage,
};

use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind, PlanEntryStatus,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionConfigId, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectOptions, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, StopReason, TextContent, Usage as AcpUsage,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};

use crate::adapter::registry::AcpTarget;
use crate::quark::Quark;

/// One turn, handed to the resident pump.
struct TurnRequest {
    prompt: String,
    reply: tokio::sync::oneshot::Sender<anyhow::Result<TurnReply>>,
}

/// What the pump got back from one `session/prompt`.
struct TurnReply {
    text: String,
    /// The end-turn token usage, when the agent implements the (still unstable)
    /// `unstable_end_turn_token_usage` extension. `None` if it does not.
    usage: Option<AcpUsage>,
    /// The last `usage_update` seen during the turn: context used / window size.
    context: Option<(u64, u64)>,
    stop: StopReason,
}

/// The live connection: a handle onto the pump thread. Dropping it drops the
/// channel, which ends the pump's loop, which tears down the connection and reaps
/// the agent subprocess.
struct AcpSession {
    turns: tokio::sync::mpsc::UnboundedSender<TurnRequest>,
    /// The permission posture the pump should apply, swapped in before each turn.
    /// Shared because ACP's `session/request_permission` arrives on the *connection*,
    /// not on the turn, so the handler needs a way to see the current turn's mode.
    mode: Arc<Mutex<Mode>>,
    /// The model the agent is **actually** running, as the agent itself reported it —
    /// not the one the seat asked for. `None` means the agent advertised no selector,
    /// so we genuinely do not know. Absent is not "the default"; it is unknown.
    model: Arc<Mutex<Option<String>>>,
}

/// The cumulative counters an ACP session has reported so far — the watermark the
/// per-turn deltas are measured against. Cumulative, so `u64`: a long session can
/// out-grow a `u32` on cache reads alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpendWatermark {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// **The per-turn spend, by component, from cumulative counters.**
///
/// ACP's end-turn `Usage` is documented as session-cumulative — `input_tokens` is
/// "total input tokens across all turns". Hadron's spend is **per-turn** (it feeds a
/// ledger that sums it), so reporting the cumulative figure would make every turn
/// re-bill the whole session and the ledger would grow quadratically.
///
/// So: keep the last cumulative reading per component, and report the difference.
///
/// **This no longer touches `total_tokens`**, and that is the point. `total_tokens`
/// is "sum of all token types" — cache reads included — so using it made an ACP quark
/// report ~200x what a CLI quark reported for the same work. The components are
/// carried separately and [`hadron_lattice::TokenSpend::fresh`] is the only thing that
/// adds any of them up.
///
/// The guard, kept per-component: if a counter goes *backwards*, the agent either
/// restarted its count or reports per-turn despite the schema saying cumulative.
/// Saturating to 0 would silently drop that turn's cost, so a backwards counter is
/// read as an absolute for that turn instead.
pub fn turn_spend(last: SpendWatermark, usage: Option<&AcpUsage>) -> (TokenSpend, SpendWatermark) {
    let Some(u) = usage else {
        // The agent does not implement end-turn usage. Absent is absent: report
        // nothing (not zero) and do not move the watermark.
        return (TokenSpend::default(), last);
    };
    let delta = |now: u64, prev: u64| -> u32 {
        let d = if now >= prev { now - prev } else { now };
        d.min(u32::MAX as u64) as u32
    };
    let spend = TokenSpend {
        input: Some(delta(u.input_tokens, last.input)),
        output: Some(delta(u.output_tokens, last.output)),
        // Absent stays absent: an agent that reports no cache columns gets `None`,
        // never `Some(0)`.
        cache_read: u.cached_read_tokens.map(|n| delta(n, last.cache_read)),
        cache_write: u.cached_write_tokens.map(|n| delta(n, last.cache_write)),
    };
    let next = SpendWatermark {
        input: u.input_tokens,
        output: u.output_tokens,
        // A component the agent did not report must not reset its watermark, or the
        // next real reading comes out as a bogus delta against zero.
        cache_read: u.cached_read_tokens.unwrap_or(last.cache_read),
        cache_write: u.cached_write_tokens.unwrap_or(last.cache_write),
    };
    (spend, next)
}

/// Translate the resolved permission mode into an answer to ACP's *blocking*
/// `session/request_permission`.
///
/// This is deliberately the **narrow** version. ACP can express real per-tool,
/// human-in-the-loop gating (the agent blocks until we answer, and the options carry
/// `AllowAlways` / `RejectAlways` — the trust-on-first-use kinds the CLI path cannot
/// express). Wiring that to hadron's field-driven grant flow is a separate piece of
/// work, because the human's answer arrives asynchronously via the field while the
/// JSON-RPC call is held open. Until then we answer from the turn's posture alone:
///
/// - **Ask / Write** → reject. The quark may talk, not act unattended.
/// - **Auto / Bypass** → allow once.
///
/// `AllowAlways` is never selected: remembering a grant is the field's job, and this
/// function has no way to record one. Erring toward `*_once` keeps the blast radius
/// of a mistake to a single tool call.
fn permission_choice(mode: Mode) -> PermissionOptionKind {
    match mode {
        Mode::Ask | Mode::Write => PermissionOptionKind::RejectOnce,
        Mode::Auto | Mode::Bypass => PermissionOptionKind::AllowOnce,
    }
}

/// One model the seated agent says it can actually run: the id that goes on the wire,
/// and the label a human should see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpModel {
    pub value: String,
    pub label: String,
}

/// The agent's **model selector**, exactly as it advertised it on `session/new`.
///
/// This is not a thing we invent — it is a thing the agent hands us and which, until
/// today, Hadron threw away. `config_options` is on `NewSessionResponse` in **v1**;
/// the model picker never needed a protocol migration, only a reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelector {
    /// What `session/set_config_option` must name to change it.
    pub config_id: SessionConfigId,
    pub current: String,
    pub available: Vec<AcpModel>,
}

/// Find the model selector among the options the agent advertised.
///
/// Selection is by `category == Model`, **not** by matching the option's id against a
/// name we guessed: an id like `"model"` is that agent's private business, and a client
/// that hard-codes it works for exactly one agent. The category is the contract.
///
/// Boolean options (`Fast mode`) and non-model selects (`Mode`, `Thought level`) are
/// not models and are ignored here.
pub fn model_selector(options: &[SessionConfigOption]) -> Option<ModelSelector> {
    config_selector(options, SessionConfigOptionCategory::Model)
}

pub fn effort_selector(options: &[SessionConfigOption]) -> Option<ModelSelector> {
    config_selector(options, SessionConfigOptionCategory::ThoughtLevel)
}

pub fn mode_selector(options: &[SessionConfigOption]) -> Option<ModelSelector> {
    config_selector(options, SessionConfigOptionCategory::Mode)
}

pub fn config_selector(options: &[SessionConfigOption], category: SessionConfigOptionCategory) -> Option<ModelSelector> {
    let opt = options
        .iter()
        .find(|o| o.category.as_ref() == Some(&category))?;

    let SessionConfigKind::Select(select) = &opt.kind else {
        // A model you cannot choose from a list is not a picker. Say nothing rather
        // than guess a shape.
        return None;
    };

    // The agent may group its models (e.g. by family). A group is a UI affordance;
    // for choosing, flatten it.
    let available: Vec<AcpModel> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(opts) => opts
            .iter()
            .map(|o| AcpModel { value: o.value.to_string(), label: o.name.clone() })
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|g| g.options.iter())
            .map(|o| AcpModel { value: o.value.to_string(), label: o.name.clone() })
            .collect(),
        // The enum is `#[non_exhaustive]`: a future ACP may add a shape we have never
        // seen. An unknown shape means we cannot enumerate the models — which is not
        // the same as the agent having none, so offer nothing rather than a wrong list.
        _ => return None,
    };

    Some(ModelSelector {
        config_id: opt.id.clone(),
        current: select.current_value.to_string(),
        available,
    })
}

/// Resolve what the **seat** asked for against what the **agent** actually offers, and
/// return the wire value to set — or `None` to leave the agent's own default alone.
///
/// Matching is deliberately forgiving, because a seat's `model` is typed by a human:
/// exact id first, then the human label, then a case-insensitive substring of either.
/// `"opus"` should find `"claude-opus-4-8"`, and `"Sonnet"` should find `"Sonnet 4.5"`.
///
/// It returns `None` when the seat asked for nothing, when the request is *already* the
/// current model, or when nothing matches. That last case is the important one: an
/// unmatched model is **not** an error that should kill the turn — the agent has a
/// perfectly good default — but it must be visible, so the caller warns.
pub fn resolve_model(selector: &ModelSelector, wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return None;
    }
    let lower = wanted.to_lowercase();

    let hit = selector
        .available
        .iter()
        .find(|m| m.value == wanted)
        .or_else(|| selector.available.iter().find(|m| m.value.eq_ignore_ascii_case(wanted)))
        .or_else(|| selector.available.iter().find(|m| m.label.eq_ignore_ascii_case(wanted)))
        .or_else(|| {
            selector.available.iter().find(|m| {
                m.value.to_lowercase().contains(&lower) || m.label.to_lowercase().contains(&lower)
            })
        })?;

    // Already there. Setting it again is a needless round trip, and it would make the
    // "we switched the model" log line a lie.
    if hit.value == selector.current {
        return None;
    }
    Some(hit.value.clone())
}

/// Boot an ACP agent, complete the `initialize` handshake, read back who answered,
/// and shut it down. **Blocking** — call it off the UI thread.
///
/// This is what "Connect" in Settings means: proof that the command in the seat
/// actually boots and speaks ACP, before the human is told the provider is ready.
/// It deliberately opens **no session** and answers **no permission request** — a
/// session is a turn, a turn is the daemon's job, and a UI that can approve a tool
/// call is a permission ladder with a hole in it.
///
/// Returns the agent's own name (ACP's `agent_info`), or the reason it failed.
pub fn probe(target: &AcpTarget) -> anyhow::Result<String> {
    let command = target.command_line();
    let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<String>>();

    // Same shape as `boot`: the SDK's connection API is scoped to its closure and
    // wants its own executor, so it gets its own thread.
    std::thread::Builder::new()
        .name("hadron-acp-probe".to_string())
        .spawn(move || {
            let outcome: anyhow::Result<String> = futures::executor::block_on(async move {
                let agent = AcpAgent::from_str(&command)
                    .map_err(|e| anyhow::anyhow!("bad ACP command {command:?}: {e}"))?;
                let name = agent_client_protocol::Client
                    .builder()
                    .name("hadron")
                    .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
                        let init = cx
                            .send_request(InitializeRequest::new(ProtocolVersion::V1))
                            .block_task()
                            .await?;
                        let sess = cx
                            .send_request(NewSessionRequest::new(std::env::temp_dir()))
                            .block_task()
                            .await?;
                        let opts = sess.config_options.unwrap_or_default();
                        
                        if let Some(selector) = model_selector(&opts) {
                            Ok(selector.current)
                        } else {
                            Ok(init.agent_info.map(|i| i.name).unwrap_or_else(|| "unnamed agent".into()))
                        }
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("ACP handshake failed: {e}"))?;
                Ok(name)
            });
            let _ = tx.send(outcome);
        })?;

    // A boot that never answers must fail, not hang the Settings window forever.
    match rx.recv_timeout(std::time::Duration::from_secs(120)) {
        Ok(result) => result,
        Err(_) => anyhow::bail!("ACP agent did not answer `initialize` within 120s"),
    }
}

/// A quark backed by a resident ACP agent.
/// Publishes what this quark is doing, mid-turn, for the chamber to render.
///
/// Cheap to clone (it is moved into the ACP notification handler, which the SDK
/// drives on its own thread) and **throttled**: an agent emits a thought chunk
/// every few tokens, and a file write per token would be a hot loop that helps
/// nobody read faster.
#[derive(Clone)]
struct LiveFeed {
    dir: PathBuf,
    quark: QuarkId,
    last: Arc<Mutex<Option<Instant>>>,
}

impl LiveFeed {
    /// The minimum gap between two published activities. A tool call ignores it —
    /// it is the one update the human is actually reading.
    const THROTTLE: std::time::Duration = std::time::Duration::from_millis(200);

    fn publish(&self, doing: Doing, detail: &str) {
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

    fn clear(&self) {
        let _ = live::clear(&self.dir, &self.quark);
    }
}

pub struct AcpQuark {
    id: QuarkId,
    flavor: Flavor,
    /// The `@mention` name (see [`Quark::display_name`]); `None` = id-only.
    display_name: Option<String>,
    /// The model this seat **asks** for. It is not necessarily the one that runs: the
    /// agent advertises what it can offer on `session/new` and we match against that
    /// (see [`model_selector`] and [`resolve_model`]). The model that actually ran is
    /// on [`AcpSession::model`], because only the agent knows it.
    model: String,
    effort: Option<String>,
    mode_config: Option<String>,
    /// How to boot this agent.
    target: AcpTarget,
    /// `None` until the first turn: booting is lazy, exactly as the CLI path spawns
    /// nothing until `excite`.
    session: Option<AcpSession>,
    /// The watermark for [`turn_spend`].
    last_spend: SpendWatermark,
    /// Where to publish mid-turn activity. `None` = nobody is watching (tests, and
    /// any caller that has no field on disk), and the stream is simply dropped.
    live: Option<LiveFeed>,
}

impl AcpQuark {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, effort: Option<String>, mode_config: Option<String>, target: AcpTarget) -> Self {
        AcpQuark {
            id,
            flavor,
            display_name: None,
            model: model.into(),
            effort,
            mode_config,
            target,
            session: None,
            last_spend: SpendWatermark::default(),
            live: None,
        }
    }

    /// Stream this quark's mid-turn activity into `dir` (see `hadron_lattice::live`).
    /// The daemon calls this; a test that has no field does not, and the quark then
    /// publishes nothing.
    pub fn watching(mut self, dir: PathBuf) -> Self {
        self.live = Some(LiveFeed {
            dir,
            quark: self.id.clone(),
            last: Arc::new(Mutex::new(None)),
        });
        self
    }

    /// Set the `@mention` display name (from the resolved team config).
    pub fn with_display_name(mut self, name: Option<String>) -> Self {
        self.display_name = name;
        self
    }

    /// The model the agent reported it is **actually** running, once a session is open.
    ///
    /// **Implemented, unwired.** Nothing consumes this yet, and that is a deliberate
    /// stopping point rather than an oversight: its home is a `model` field on
    /// `hadron_lattice::Usage`, so that a turn's telemetry records the model that ran
    /// and a turn can finally be *priced* (you cannot cost a turn you cannot attribute
    /// to a model). Adding that field touches every exhaustive `Usage { .. }` literal,
    /// two of which are in files another quark is mid-write in. It lands next, on a
    /// tree that is not moving.
    pub fn running_model(&self) -> Option<String> {
        self.session.as_ref()?.model.lock().unwrap().clone()
    }

    /// Boot the agent and open one session in `cwd`. Blocks until the agent has
    /// answered `initialize` and `session/new`, so a boot failure (missing `npx`, a
    /// dead adapter, an unauthenticated CLI) surfaces as a failed turn rather than a
    /// silent hang.
    ///
    /// `cwd` is the quark's own worktree, straight off the `Projection` — ACP's
    /// `session/new` takes a required `cwd`, so hadron's existing cwd chain lands on
    /// the protocol with no adaptation.
    fn boot(
        target: &AcpTarget,
        cwd: PathBuf,
        want_model: String,
        want_effort: Option<String>,
        want_mode: Option<String>,
        live: Option<LiveFeed>,
    ) -> anyhow::Result<AcpSession> {
        let (turns_tx, mut turns_rx) = tokio::sync::mpsc::unbounded_channel::<TurnRequest>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();

        let mode = Arc::new(Mutex::new(Mode::default()));
        let handler_mode = Arc::clone(&mode);

        // What the agent says it is running, written once at boot by the pump.
        let model = Arc::new(Mutex::new(None::<String>));
        let pump_model = Arc::clone(&model);

        let command = target.command_line();
        // The reply accumulator and the context watermark are written by the
        // notification handler (which the SDK drives on the connection) and read by
        // the turn pump. Hence the Arcs.
        let transcript = Arc::new(Mutex::new(String::new()));
        let context = Arc::new(Mutex::new(None::<(u64, u64)>));
        let pump_transcript = Arc::clone(&transcript);
        let pump_context = Arc::clone(&context);

        // One handle for the happy path (moved into the pump, fired once the session
        // opens) and one for the failure path (kept out here, fired if the pump dies
        // before it ever gets that far).
        let boot_tx = ready_tx.clone();

        std::thread::Builder::new()
            .name("hadron-acp".to_string())
            .spawn(move || {
                let outcome: anyhow::Result<()> = futures::executor::block_on(async move {
                    let agent = AcpAgent::from_str(&command)
                        .map_err(|e| anyhow::anyhow!("bad ACP command {command:?}: {e}"))?;

                    let connect = agent_client_protocol::Client
                        .builder()
                        .name("hadron")
                        .on_receive_notification(
                            async move |n: SessionNotification, _cx| {
                                match n.update {
                                    // The reply text. `PromptResponse` carries none,
                                    // so this is the only place a message exists.
                                    SessionUpdate::AgentMessageChunk(chunk) => {
                                        if let ContentBlock::Text(t) = chunk.content {
                                            transcript.lock().unwrap().push_str(&t.text);
                                        }
                                    }
                                    // Real context numbers, including the window SIZE
                                    // — which the claude CLI never reports.
                                    SessionUpdate::UsageUpdate(u) => {
                                        *context.lock().unwrap() = Some((u.used, u.size));
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
                                            feed.publish(Doing::Working, &call.title);
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
                            },
                            agent_client_protocol::on_receive_notification!(),
                        )
                        .on_receive_request(
                            async move |req: RequestPermissionRequest, responder, _cx| {
                                let want = permission_choice(*handler_mode.lock().unwrap());
                                // Pick the offered option whose kind matches our
                                // posture. An agent need not offer every kind, so
                                // fall back to *rejecting* rather than to whatever
                                // happens to be first — never fail open.
                                let chosen = req
                                    .options
                                    .iter()
                                    .find(|o| o.kind == want)
                                    .or_else(|| {
                                        req.options
                                            .iter()
                                            .find(|o| o.kind == PermissionOptionKind::RejectOnce)
                                    })
                                    .map(|o| o.option_id.clone());

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

                            let session = cx
                                .send_request(NewSessionRequest::new(cwd))
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

                                let sent = cx
                                    .send_request(PromptRequest::new(
                                        sid.clone(),
                                        vec![ContentBlock::Text(TextContent::new(turn.prompt))],
                                    ))
                                    .block_task()
                                    .await;

                                let reply = match sent {
                                    Ok(resp) => Ok(TurnReply {
                                        text: pump_transcript.lock().unwrap().clone(),
                                        usage: resp.usage.clone(),
                                        context: *pump_context.lock().unwrap(),
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
}

#[async_trait]
impl Quark for AcpQuark {
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
    /// An ACP quark is a **resident** session: the agent is booted once and keeps the
    /// conversation across turns, so the skill library injected on the first turn stays
    /// in its context (and is a prompt-cache read thereafter).
    fn resident(&self) -> bool {
        true
    }

    /// The turn ends the moment this returns, however it returns. Clearing the live
    /// feed here — rather than on the happy path inside [`AcpQuark::run_turn`] — is
    /// what makes "a turn that died still goes idle" true by construction: a quark
    /// whose agent crashed must not sit in the chamber `thinking` forever.
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let outcome = self.run_turn(turn).await;
        if let Some(feed) = &self.live {
            feed.clear();
        }
        outcome
    }

    /// Force-restart: drop the resident session. Dropping the [`AcpSession`] drops the
    /// `turns` channel, which ends the pump thread, tears down the connection, and reaps
    /// the agent subprocess (see the struct doc). The next turn re-boots from scratch.
    /// A no-op if no session is open, so it is safe to call on an idle quark.
    fn reset_session(&mut self) {
        self.session = None;
    }
}

impl AcpQuark {
    async fn run_turn(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let mode = turn.mode;
        let prompt = crate::adapter::prompt::build(&turn, &self.id);

        // If the chat history has been cleared/reset, discard the resident session so the agent boots fresh.
        if turn.field_window.is_empty() {
            self.session = None;
        }

        // Boot on the first turn, in the quark's own worktree, and keep it.
        if self.session.is_none() {
            self.session = Some(Self::boot(
                &self.target,
                turn.cwd.clone(),
                self.model.clone(),
                self.effort.clone(),
                self.mode_config.clone(),
                self.live.clone(),
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
        // A failed `session/prompt` on a LIVE connection (a refusal, a bad request) is
        // not a dead agent — keep the session and let the turn fail on its own.
        let reply = reply?;

        // Per-turn spend, by component, from the cumulative counters.
        let (spend, new_watermark) = turn_spend(self.last_spend, reply.usage.as_ref());
        self.last_spend = new_watermark;

        // Context, when the agent sent a `usage_update`. NOTE the honesty rule this
        // codebase already enforces (see `telemetry.rs`): ACP has **no quota concept
        // at all**, so `quota` stays empty rather than claiming a full budget. And
        // `used_percentage` is computed here only because ACP — unlike agy — does not
        // send one; `size` is the agent's own reported window, never invented.
        let usage = Usage {
            spend,
            context: reply.context.map(|(used, size)| ContextUsage {
                used_tokens: used.min(u32::MAX as u64) as u32,
                context_window_size: size.min(u32::MAX as u64) as u32,
                used_percentage: if size > 0 { (used as f64 / size as f64) * 100.0 } else { 0.0 },
            }),
            model: self.session.as_ref().and_then(|s| s.model.lock().unwrap().clone()),
            quota: vec![],
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn usage(total: u64) -> AcpUsage {
        AcpUsage::new(total, total / 2, total / 2)
    }

    /// **The delta, and why it exists.** ACP reports token usage *cumulatively across
    /// the session*, while a turn's spend is what that one turn cost. Feed the
    /// cumulative number straight through and a 3-turn session bills
    /// 100 + 250 + 400 for what actually cost 400 — the ledger would grow
    /// quadratically in the length of the conversation.
    #[test]
    fn spend_is_the_delta_not_the_cumulative_total() {
        // usage(n) puts n/2 in input and n/2 in output, so fresh() == n.
        let (s1, w1) = turn_spend(SpendWatermark::default(), Some(&usage(100)));
        assert_eq!(s1.fresh(), Some(100));
        assert_eq!(w1.input, 50, "watermark follows the cumulative components");
        assert_eq!(w1.output, 50);

        // Turn 2: the agent reports 250 cumulative. This turn cost 150, not 250.
        let (s2, w2) = turn_spend(w1, Some(&usage(250)));
        assert_eq!(s2.fresh(), Some(150));

        // Turn 3: 400 cumulative → 150 this turn.
        let (s3, w3) = turn_spend(w2, Some(&usage(400)));
        assert_eq!(s3.fresh(), Some(150));
        assert_eq!(w3.input, 200);

        // The ledger sums the per-turn costs and lands on the agent's own total.
        assert_eq!(s1.fresh().unwrap() + s2.fresh().unwrap() + s3.fresh().unwrap(), 400);
    }

    /// **The bug this whole type exists to kill.** `total_tokens` is "sum of all token
    /// types" — cache included — and cache reads dwarf everything: a turn with N tool
    /// calls re-reads the whole prompt N times. The old adapter reported that total as
    /// `used_tokens`, so acp-claude logged 1,307,987 for a turn whose real work was a
    /// few greps, against opus's median of 5,338 for a full engineering turn.
    ///
    /// `fresh()` must count input+output ONLY. If cache ever leaks back into it, this
    /// fails.
    #[test]
    fn cache_reads_are_carried_but_never_counted_as_fresh() {
        let mut u = AcpUsage::new(300_000, 20, 2_400);
        u.cached_read_tokens = Some(250_000);
        u.cached_write_tokens = Some(45_000);

        let (spend, _) = turn_spend(SpendWatermark::default(), Some(&u));

        // The comparable unit: what the model actually processed anew.
        assert_eq!(spend.fresh(), Some(2_420), "input + output, and NOTHING else");
        // The cache is not discarded — it is just not confused with work.
        assert_eq!(spend.cached(), Some(295_000));
        assert_eq!(spend.cache_read, Some(250_000));
        assert_eq!(spend.cache_write, Some(45_000));
        // And the 300_000 `total_tokens` the old code used is nowhere in sight.
        assert_ne!(spend.fresh(), Some(300_000));
    }

    /// An agent that does not implement the (unstable) end-turn usage extension
    /// reports nothing. **Absent is absent — `None`, not 0** — and the watermark must
    /// NOT move, or the next real reading comes out as a bogus delta.
    #[test]
    fn an_agent_without_usage_reports_unknown_and_does_not_move_the_watermark() {
        let w0 = SpendWatermark { input: 250, output: 250, ..Default::default() };
        let (s, w) = turn_spend(w0, None);
        assert_eq!(s.fresh(), None, "unknown, NOT zero");
        assert!(s.is_empty());
        assert_eq!(w, w0, "watermark held");

        // and the next real reading is still a correct delta against it
        let (s2, _) = turn_spend(w, Some(&usage(560)));
        assert_eq!(s2.fresh(), Some(60));
    }

    /// An agent that reports tokens but no cache columns must get `None` for cache,
    /// never `Some(0)` — a zero would assert a fact we do not have, and the UI could
    /// not tell "no cache used" from "this agent doesn't report cache".
    #[test]
    fn a_missing_cache_column_is_unknown_not_zero() {
        let (spend, w) = turn_spend(SpendWatermark::default(), Some(&usage(100)));
        assert_eq!(spend.cache_read, None);
        assert_eq!(spend.cache_write, None);
        assert_eq!(spend.cached(), None, "unknown, not 0");
        assert_eq!(w.cache_read, 0, "and an unreported column does not move its watermark");
    }

    /// A counter that goes backwards (the agent restarted its count, or reports
    /// per-turn despite the schema saying cumulative) must not silently drop the
    /// turn's cost to zero.
    #[test]
    fn a_backwards_counter_is_read_as_an_absolute_not_as_zero() {
        let w0 = SpendWatermark { input: 500, output: 500, ..Default::default() };
        let (s, w) = turn_spend(w0, Some(&usage(42)));
        assert_eq!(s.fresh(), Some(42), "not 0");
        assert_eq!(w.input, 21, "and the watermark follows the agent");
    }

    /// Posture decides the answer to ACP's blocking permission request, and the
    /// unattended postures are the only ones that act. Never fail open.
    #[test]
    fn permission_follows_the_turn_posture() {
        assert_eq!(permission_choice(Mode::Ask), PermissionOptionKind::RejectOnce);
        assert_eq!(permission_choice(Mode::Write), PermissionOptionKind::RejectOnce);
        assert_eq!(permission_choice(Mode::Auto), PermissionOptionKind::AllowOnce);
        assert_eq!(permission_choice(Mode::Bypass), PermissionOptionKind::AllowOnce);
    }

    /// Build the shape the agent actually sends: a `Model` select alongside the other
    /// config options it advertises (a `Mode` select, a `Fast mode` boolean), so the
    /// tests below prove we pick the model out of a realistic crowd, not out of a list
    /// of one.
    fn advertised_options() -> Vec<SessionConfigOption> {
        use agent_client_protocol::schema::v1::{
            SessionConfigBoolean, SessionConfigSelect, SessionConfigSelectOption,
        };

        let model = SessionConfigOption::new(
            "model",
            "Model",
            SessionConfigKind::Select(SessionConfigSelect::new(
                "claude-sonnet-4-5",
                vec![
                    SessionConfigSelectOption::new("claude-opus-4-8", "Opus 4.8"),
                    SessionConfigSelectOption::new("claude-sonnet-4-5", "Sonnet 4.5"),
                    SessionConfigSelectOption::new("claude-haiku-4-5", "Haiku 4.5"),
                ],
            )),
        )
        .category(SessionConfigOptionCategory::Model);

        // A NON-model select. If we picked by position, or by the id "model" happening
        // to sort first, this is what would catch it.
        let mode = SessionConfigOption::new(
            "mode",
            "Mode",
            SessionConfigKind::Select(SessionConfigSelect::new(
                "default",
                vec![
                    SessionConfigSelectOption::new("default", "Always ask"),
                    SessionConfigSelectOption::new("bypassPermissions", "Bypass"),
                ],
            )),
        )
        .category(SessionConfigOptionCategory::Mode);

        let fast = SessionConfigOption::new(
            "fast-mode",
            "Fast mode",
            SessionConfigKind::Boolean(SessionConfigBoolean::new(false)),
        );

        vec![mode, fast, model]
    }

    /// The model picker is found by **category**, not by guessing the option's id — an
    /// id is that agent's private business and a client that hard-codes one works for
    /// exactly one agent.
    #[test]
    fn the_model_selector_is_found_by_category_not_by_name() {
        let s = model_selector(&advertised_options()).expect("a Model category is advertised");
        assert_eq!(s.config_id.to_string(), "model");
        assert_eq!(s.current, "claude-sonnet-4-5");
        assert_eq!(s.available.len(), 3, "the Mode select and the boolean are not models");
    }

    /// An agent that offers no model selector is not an error — it just cannot be
    /// re-modelled. We must say "no picker", not invent one.
    #[test]
    fn an_agent_with_no_model_option_offers_no_picker() {
        let no_models: Vec<SessionConfigOption> = advertised_options()
            .into_iter()
            .filter(|o| o.category != Some(SessionConfigOptionCategory::Model))
            .collect();
        assert_eq!(model_selector(&no_models), None);
    }

    /// A seat's `model` is typed by a human, so resolution is forgiving — but it only
    /// ever returns a value the **agent** offered. We never send a model we invented.
    #[test]
    fn a_seat_resolves_its_model_against_what_the_agent_offers() {
        let s = model_selector(&advertised_options()).unwrap();

        // Exact id.
        assert_eq!(resolve_model(&s, "claude-opus-4-8").as_deref(), Some("claude-opus-4-8"));
        // The human label.
        assert_eq!(resolve_model(&s, "Opus 4.8").as_deref(), Some("claude-opus-4-8"));
        // A bare family name — what Jake actually types in `team.json`.
        assert_eq!(resolve_model(&s, "opus").as_deref(), Some("claude-opus-4-8"));
        assert_eq!(resolve_model(&s, "HAIKU").as_deref(), Some("claude-haiku-4-5"));

        // Asking for nothing changes nothing.
        assert_eq!(resolve_model(&s, ""), None);
        assert_eq!(resolve_model(&s, "   "), None);

        // Asking for what is ALREADY running is not a change — setting it again would
        // be a needless round trip and would make "we switched the model" a lie.
        assert_eq!(resolve_model(&s, "claude-sonnet-4-5"), None);
        assert_eq!(resolve_model(&s, "sonnet"), None);

        // A model this agent does not have resolves to nothing — we do NOT pass the
        // human's string through to the wire and hope.
        assert_eq!(resolve_model(&s, "gpt-5"), None);
        assert_eq!(resolve_model(&s, "gemini-3-pro"), None);
    }

    /// Agents may group their models by family. A group is a display affordance; for
    /// *choosing*, the list is flat — and a grouped agent must be just as pickable.
    #[test]
    fn grouped_models_are_flattened_for_choosing() {
        use agent_client_protocol::schema::v1::{
            SessionConfigSelect, SessionConfigSelectGroup, SessionConfigSelectOption,
        };

        let grouped = SessionConfigOption::new(
            "model",
            "Model",
            SessionConfigKind::Select(SessionConfigSelect::new(
                "claude-sonnet-4-5",
                vec![
                    SessionConfigSelectGroup::new(
                        "frontier",
                        "Frontier",
                        vec![SessionConfigSelectOption::new("claude-opus-4-8", "Opus 4.8")],
                    ),
                    SessionConfigSelectGroup::new(
                        "fast",
                        "Fast",
                        vec![
                            SessionConfigSelectOption::new("claude-sonnet-4-5", "Sonnet 4.5"),
                            SessionConfigSelectOption::new("claude-haiku-4-5", "Haiku 4.5"),
                        ],
                    ),
                ],
            )),
        )
        .category(SessionConfigOptionCategory::Model);

        let s = model_selector(&[grouped]).expect("a grouped Model selector is still a selector");
        assert_eq!(s.available.len(), 3, "groups are flattened, not dropped");
        assert_eq!(resolve_model(&s, "opus").as_deref(), Some("claude-opus-4-8"));
    }

    /// The default boot command for `acp-claude`, and the free-form one. A seat is a
    /// config row, so this is the whole "new provider without a code change" claim.
    #[test]
    fn a_target_renders_its_command_line() {
        assert_eq!(
            AcpTarget::claude_adapter().command_line(),
            "npx -y @agentclientprotocol/claude-agent-acp@latest"
        );
        let custom = AcpTarget { program: "goose".into(), args: vec!["acp".into()] };
        assert_eq!(custom.command_line(), "goose acp");
    }

    /// A boot that cannot possibly work must FAIL the turn, not hang it. The pump
    /// thread dies before `session/new`, and `boot` has to notice rather than park on
    /// `ready_rx` forever. (This one is not `#[ignore]`d: it spawns a process, but a
    /// nonexistent one, so it is local, free and fast.)
    ///
    /// It also pins that a failed boot leaves **no** session behind — so the quark is
    /// not wedged, and a later turn is free to try again. A resident transport that
    /// cannot recover from a dead agent would be strictly worse than the one-shot CLI,
    /// which gets recovery for free by spawning per turn.
    #[tokio::test]
    async fn a_dead_agent_fails_the_turn_instead_of_hanging() {
        let target = AcpTarget {
            program: "hadron-definitely-not-a-real-acp-agent".into(),
            args: vec![],
        };
        let mut q = AcpQuark::new(QuarkId::new("dead"), Flavor::Worker, "", None, None, target);

        for attempt in 1..=2 {
            let err =
                tokio::time::timeout(std::time::Duration::from_secs(30), q.excite(projection()))
                    .await
                    .unwrap_or_else(|_| panic!("boot must not hang (attempt {attempt})"))
                    .expect_err("a nonexistent agent cannot open a session");
            eprintln!("boot error (attempt {attempt}): {err}");
            assert!(
                q.session.is_none(),
                "a failed boot must leave no session — the quark has to stay re-bootable"
            );
        }
    }

    /// Force-restart drops the resident session (which reaps the subprocess) and leaves
    /// the quark re-bootable — the same post-condition as a failed boot, but on demand.
    #[test]
    fn reset_session_drops_the_session_and_stays_rebootable() {
        use crate::quark::Quark as _;
        let mut q = AcpQuark::new(
            QuarkId::new("acp-claude"),
            Flavor::Worker,
            "",
            None,
            None,
            AcpTarget::claude_adapter(),
        );
        // Stand in a live session (a dummy pump handle) without booting a real agent.
        let (turns_tx, _turns_rx) = tokio::sync::mpsc::unbounded_channel();
        q.session = Some(AcpSession {
            turns: turns_tx,
            mode: Arc::new(Mutex::new(Mode::Ask)),
            model: Arc::new(Mutex::new(None)),
        });
        assert!(q.session.is_some(), "precondition: a session is open");

        q.reset_session();
        assert!(q.session.is_none(), "reset_session must drop the resident session");

        // Idempotent: a second reset on an already-idle quark is a no-op, not a panic.
        q.reset_session();
        assert!(q.session.is_none());
    }

    fn projection() -> Projection {
        Projection {
            task: "Reply with exactly the word: pong".into(),
            invariants: String::new(),
            available_invariants: vec![],
            nucleus_digest: String::new(),
            memory: String::new(),
            memory_truncated: false,
            memory_path: std::path::PathBuf::new(),
            memory_notes_dir: std::path::PathBuf::new(),
            live_activities: vec![], roster: vec![],
            field_window: vec![],
            field_truncated: false,
            git_diff: String::new(),
            cwd: std::env::temp_dir(),
            isolated: false,
            mode: Mode::Ask,
        }
    }

    /// **WHAT THE AGENT ACTUALLY ADVERTISES.** The model picker rested on a belief —
    /// mine and agy's both — that it needed ACP **v2**. It does not. `config_options`
    /// is on **v1**'s `NewSessionResponse`, and the agent has been sending it to us on
    /// every single `session/new` while we discarded it.
    ///
    /// This test is the receipt. It opens a real session against the real agent and
    /// asserts a `Model` selector comes back with more than one model in it. If a future
    /// agent stops advertising models, this is the test that says so.
    ///
    /// Note the `env -u CLAUDECODE`: Claude Code refuses to boot inside another Claude
    /// Code session, and it fails *by hanging*, not by erroring. That is what stopped
    /// the previous attempt to reach this agent.
    ///
    /// ```text
    /// env -u CLAUDECODE cargo test -p hadron-gluon --lib \
    ///     acp::tests::the_agent_advertises_its_models -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "live: needs node + npx + an authenticated claude"]
    async fn the_agent_advertises_its_models() {
        let command = AcpTarget::claude_adapter().command_line();
        let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<Option<ModelSelector>>>();

        std::thread::Builder::new()
            .name("hadron-acp-model-probe".to_string())
            .spawn(move || {
                let found = futures::executor::block_on(async move {
                    let agent = AcpAgent::from_str(&command)
                        .map_err(|e| anyhow::anyhow!("bad command: {e}"))?;
                    agent_client_protocol::Client
                        .builder()
                        .name("hadron")
                        .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
                            let init = cx
                                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                                .block_task()
                                .await?;
                            eprintln!(
                                "agent: {:?} — it negotiated protocol {:?}",
                                init.agent_info.map(|i| i.name),
                                init.protocol_version,
                            );

                            let sess = cx
                                .send_request(NewSessionRequest::new(std::env::temp_dir()))
                                .block_task()
                                .await?;
                            let opts = sess.config_options.unwrap_or_default();
                            eprintln!(
                                "config_options it volunteered: {:?}",
                                opts.iter().map(|o| (&o.name, &o.category)).collect::<Vec<_>>()
                            );
                            Ok(model_selector(&opts))
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("ACP handshake failed: {e}"))
                });
                let _ = tx.send(found);
            })
            .expect("probe thread");

        let selector = rx
            .recv_timeout(std::time::Duration::from_secs(180))
            .expect("the agent must answer within 180s")
            .expect("the handshake must succeed")
            .expect("the agent MUST advertise a Model config option — that is the picker");

        eprintln!("\n=== the model picker, over ACP v1 ===");
        eprintln!("config_id : {:?}", selector.config_id);
        eprintln!("current   : {}", selector.current);
        for m in &selector.available {
            eprintln!("  - {} ({})", m.label, m.value);
        }

        assert!(
            selector.available.len() > 1,
            "a picker with one model is not a picker: {:?}",
            selector.available
        );
        // And the resolution the seat will actually do, end to end.
        assert!(
            selector.available.iter().any(|m| m.value.to_lowercase().contains("opus")
                || m.label.to_lowercase().contains("opus")),
            "expected an Opus among the offered models: {:?}",
            selector.available
        );
    }

    /// **THE PICKER, END TO END.** A seat says `model: "haiku"`; the agent must come up
    /// running Haiku. This is the whole feature, and it is the difference between
    /// "the protocol permits model selection" (a schema fact) and "Hadron selects the
    /// model" (a Hadron fact).
    ///
    /// It asks for Haiku deliberately: the agent's own default is Opus, so a pass
    /// cannot be a false positive from us simply not changing anything.
    ///
    /// ```text
    /// env -u CLAUDECODE cargo test -p hadron-gluon --lib \
    ///     acp::tests::a_seat_gets_the_model_it_asked_for -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "live: needs node + npx + an authenticated claude"]
    async fn a_seat_gets_the_model_it_asked_for() {
        let mut q = AcpQuark::new(
            QuarkId::new("acp"),
            Flavor::Worker,
            "haiku",
            None,
            None,
            AcpTarget::claude_adapter(),
        );

        let out = q.excite(projection()).await.expect("live ACP turn");
        eprintln!("reply: {:?}", out.message);

        let running = q.running_model().expect("the agent must report the model it is running");
        eprintln!("seat asked for : haiku");
        eprintln!("agent is running: {running}");
        assert_eq!(
            running, "haiku",
            "the seat asked for haiku and the agent's own default is opus — \
             if this says opus, the picker did not take"
        );
    }

    /// **THE LIVE ROUND TRIP.** Hadron speaks ACP to a real agent binary — the Claude
    /// ACP adapter, booted with `npx` — and gets a real response back, over a real
    /// resident session.
    ///
    /// `#[ignore]`d by the same convention as the other live tests: it needs network
    /// (npx fetches the adapter), Node, and an authenticated `claude`. Run it by hand:
    ///
    /// ```text
    /// cargo test -p hadron-gluon --lib acp:: -- --ignored --nocapture
    /// ```
    ///
    /// It asserts the things that are *structurally impossible* on the CLI transport:
    /// two turns run on ONE session (the agent is booted once), and the second turn
    /// can see what the first one said — which is the resident-session payoff. And it
    /// prints the real token numbers, which is what `unstable_end_turn_token_usage`
    /// bought us.
    #[tokio::test]
    #[ignore = "live: needs node + npx + an authenticated claude"]
    async fn live_acp_round_trip_against_the_claude_adapter() {
        let mut q = AcpQuark::new(
            QuarkId::new("acp"),
            Flavor::Worker,
            "",
            None,
            None,
            AcpTarget::claude_adapter(),
        );

        // --- Turn 1: does a prompt cross the wire and come back?
        let mut t1 = projection();
        t1.task = "Reply with exactly the word: pong. Nothing else.".into();
        let o1 = q.excite(t1).await.expect("live ACP turn 1");
        eprintln!("\n=== TURN 1 ===");
        eprintln!("message     : {:?}", o1.message);
        eprintln!("spend       : {:?}", o1.usage.spend);
        eprintln!("usage       : {:?}", o1.usage);

        let m1 = o1.message.as_deref().unwrap_or("").to_lowercase();
        assert!(m1.contains("pong"), "a real reply came back over ACP, got {m1:?}");

        // Real, structured, per-turn tokens — the thing the agy adapter cannot do.
        assert!(o1.usage.spend.fresh().unwrap_or(0) > 0, "end-turn token usage must be real, not 0");

        // --- Turn 2: the SESSION is resident. The agent was booted once and still
        // remembers turn 1 — so we can ask it about turn 1 without re-sending it.
        let mut t2 = projection();
        t2.field_window = vec![hadron_lattice::Event::new(
            hadron_lattice::Actor::Human,
            None,
            hadron_lattice::Kind::Message { body: "Dummy message to prevent session reset".into() },
        )];
        t2.task = "What single word did you just say? Reply with only that word.".into();
        let o2 = q.excite(t2).await.expect("live ACP turn 2");
        eprintln!("\n=== TURN 2 ===");
        eprintln!("message     : {:?}", o2.message);
        eprintln!("spend       : {:?}", o2.usage.spend);
        eprintln!("usage       : {:?}", o2.usage);
        eprintln!("cumulative watermark: {:?}", q.last_spend);

        let m2 = o2.message.as_deref().unwrap_or("").to_lowercase();
        assert!(
            m2.contains("pong"),
            "the session is RESIDENT: turn 2 recalls turn 1 without us re-sending it, got {m2:?}"
        );

        // Turn 2 is billed for turn 2, not for the whole session — the delta is real.
        assert!(o2.usage.spend.fresh().is_some(), "turn 2 has its own cost");
        let cumulative = q.last_spend.input + q.last_spend.output;
        if q.last_spend.input >= o1.usage.spend.input.unwrap_or(0) as u64 {
            assert!(
                (o1.usage.spend.fresh().unwrap_or(0) as u64
                    + o2.usage.spend.fresh().unwrap_or(0) as u64)
                    <= cumulative.max(1),
                "per-turn deltas must not exceed the agent's own cumulative total"
            );
        }
    }

    /// **The Antigravity seat says WHY it cannot start.** This is the fix for the seat
    /// that "responded and automatically errored": the adapter used to return its
    /// failure inside `result`, which a JSON-RPC client reads as *success*, so the
    /// reason never reached Hadron and the turn died with a bare `stopReason: error`.
    ///
    /// The assertion is on the **reason**, not on the failure. Any broken seat fails;
    /// only a correctly-reporting one tells you it needs a key — and it must arrive
    /// through Hadron's own ACP client (`probe`, what the chamber's "Connect" runs),
    /// because that is the path that was swallowing it.
    ///
    /// No credential needed — the *absence* of one is the case under test:
    ///
    /// ```text
    /// cargo test -p hadron-gluon --lib acp::tests::the_antigravity_seat_names_the_credential_it_lacks -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "live: spawns the Antigravity SDK venv (crates/hadron-gluon/scripts/venv)"]
    fn the_antigravity_seat_names_the_credential_it_lacks() {
        // The SDK authenticates by API key ONLY — it has no OAuth path, so it cannot
        // reuse the agy CLI's login. Guarantee the key is absent: that is the state
        // Jake's daemon is actually in.
        unsafe { std::env::remove_var("GEMINI_API_KEY") };

        // The catalogue owns the command. But it stores a path RELATIVE to the
        // workspace root, and a test runs from the crate dir — so anchor it rather
        // than restate it. (That relativity is a live hazard: the seat only spawns
        // while the daemon's cwd IS the workspace root.)
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("crates/hadron-gluon sits two levels under the workspace root");
        let preset = AcpTarget::for_provider("acp-agy").expect("acp-agy is in the catalogue");
        let target = AcpTarget {
            program: root.join(&preset.program).display().to_string(),
            args: preset.args.iter().map(|a| root.join(a).display().to_string()).collect(),
        };

        let outcome = probe(&target);

        let err = outcome.expect_err("without a key the Antigravity agent cannot start").to_string();
        eprintln!("\n=== what Hadron is told ===\n{err}\n");
        assert!(
            err.contains("GEMINI_API_KEY"),
            "the seat must name the credential it lacks, not just fail; got {err:?}"
        );
    }

    /// **The live-preview proof.** The agent streams its thoughts and tool calls to
    /// us on every turn — we used to drop them on the floor. This asserts that a real
    /// turn now *publishes* them, and that the quark goes **idle** when it ends.
    ///
    /// It watches the live dir from a second task while the turn runs, because the
    /// whole point is what is visible **mid-turn**: an assertion made after `excite`
    /// returns could never tell the difference between "it streamed" and "it never
    /// did", since a finished turn is idle either way.
    ///
    /// ```text
    /// cargo test -p hadron-gluon --lib acp::tests::a_live_turn_publishes_what_it_is_doing -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "live: needs node + npx + an authenticated claude"]
    async fn a_live_turn_publishes_what_it_is_doing() {
        let dir = std::env::temp_dir().join(format!("hadron-live-test-{}", ulid::Ulid::new()));
        let id = QuarkId::new("acp");

        let mut q = AcpQuark::new(id.clone(), Flavor::Worker, "", None, None, AcpTarget::claude_adapter())
            .watching(dir.clone());

        // Watch the live dir while the turn is in flight.
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished_clone = finished.clone();
        let (watch_dir, watch_id) = (dir.clone(), id.clone());
        let seen = tokio::spawn(async move {
            let mut seen: Vec<Doing> = Vec::new();
            for _ in 0..600 {
                if finished_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if let Some(a) = live::read(&watch_dir, &watch_id, Utc::now()) {
                    if seen.last() != Some(&a.doing) {
                        eprintln!("[live] {} — {}", a.doing.label(), a.detail);
                        seen.push(a.doing);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            seen
        });

        let mut t = projection();
        // A task that forces a tool call: the agent must LOOK at the tree, so we get
        // a `ToolCall` update and not just thought chunks.
        t.task = "List the files in the current directory using your tools, then reply with the word: done.".into();
        let out = q.excite(t).await.expect("live ACP turn");
        eprintln!("reply: {:?}", out.message);

        finished.store(true, std::sync::atomic::Ordering::Relaxed);
        let seen = seen.await.unwrap_or_default();

        assert!(
            !seen.is_empty(),
            "the agent's mid-turn stream must reach the live feed — this is the \
             `acp.rs` `_ => {{}}` arm that used to throw it away"
        );

        // The turn is over, so the quark is idle. Absence IS idle: the file is gone.
        assert_eq!(
            live::read(&dir, &id, Utc::now()),
            None,
            "a finished turn must leave no activity behind"
        );
    }
}
