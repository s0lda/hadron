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
//!   See [`turn_tokens`].

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use hadron_lattice::{
    ContextUsage, EnergyState, Flavor, Mode, Projection, QuarkId, TurnOutcome, Usage,
};

use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, StopReason, TextContent,
    Usage as AcpUsage,
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
}

/// **The per-turn token cost, from a cumulative counter.**
///
/// ACP's end-turn `Usage` is documented as session-cumulative — `total_tokens` is
/// "sum of all token types across session", `input_tokens` is "total input tokens
/// across all turns". Hadron's `TurnOutcome::used_tokens` is **per-turn** (it feeds
/// an energy ledger that sums it), so reporting the cumulative figure would make
/// every turn re-bill the whole session — the ledger would grow quadratically.
///
/// So: keep the last cumulative total, and report the difference.
///
/// Returns `(this_turn, new_last_total)`.
///
/// The guard: if the counter goes *backwards*, the agent either restarted its count
/// or never implemented the extension cumulatively. Treating that as a huge negative
/// (saturating to 0) would silently drop a turn's cost, so a backwards counter is
/// read as an absolute for that turn instead.
pub fn turn_tokens(last_total: u64, usage: Option<&AcpUsage>) -> (u32, u64) {
    let Some(u) = usage else {
        // The agent does not implement end-turn usage. Absent is absent: report 0
        // and do not move the watermark.
        return (0, last_total);
    };
    let total = u.total_tokens;
    let this_turn = if total >= last_total { total - last_total } else { total };
    (this_turn.min(u32::MAX as u64) as u32, total)
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
                        Ok(init.agent_info.map(|i| i.name).unwrap_or_else(|| "unnamed agent".into()))
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
pub struct AcpQuark {
    id: QuarkId,
    flavor: Flavor,
    /// Carried for parity with the CLI adapters and for quota-family lookup. ACP has
    /// no model-selection method in v1, so the agent picks; this is what we *asked*
    /// for, not necessarily what ran.
    #[allow(dead_code)]
    model: String,
    target: AcpTarget,
    /// `None` until the first turn: booting is lazy, exactly as the CLI path spawns
    /// nothing until `excite`.
    session: Option<AcpSession>,
    /// The watermark for [`turn_tokens`].
    last_total_tokens: u64,
}

impl AcpQuark {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, target: AcpTarget) -> Self {
        AcpQuark {
            id,
            flavor,
            model: model.into(),
            target,
            session: None,
            last_total_tokens: 0,
        }
    }

    /// Boot the agent and open one session in `cwd`. Blocks until the agent has
    /// answered `initialize` and `session/new`, so a boot failure (missing `npx`, a
    /// dead adapter, an unauthenticated CLI) surfaces as a failed turn rather than a
    /// silent hang.
    ///
    /// `cwd` is the quark's own worktree, straight off the `Projection` — ACP's
    /// `session/new` takes a required `cwd`, so hadron's existing cwd chain lands on
    /// the protocol with no adaptation.
    fn boot(target: &AcpTarget, cwd: PathBuf) -> anyhow::Result<AcpSession> {
        let (turns_tx, mut turns_rx) = tokio::sync::mpsc::unbounded_channel::<TurnRequest>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();

        let mode = Arc::new(Mutex::new(Mode::default()));
        let handler_mode = Arc::clone(&mode);

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
                                    // Thoughts, tool calls and plans are mid-turn
                                    // presence. Deliberately dropped: surfacing them
                                    // needs a mid-turn channel on `Quark::excite`,
                                    // which does not exist yet.
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
            Ok(Ok(())) => Ok(AcpSession { turns: turns_tx, mode }),
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
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }

    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let mode = turn.mode;
        let prompt = crate::adapter::prompt::build(&turn, &self.id);

        // Boot on the first turn, in the quark's own worktree, and keep it.
        if self.session.is_none() {
            self.session = Some(AcpQuark::boot(&self.target, turn.cwd.clone())?);
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

        // Per-turn cost from the cumulative counter.
        let (used_tokens, new_total) = turn_tokens(self.last_total_tokens, reply.usage.as_ref());
        self.last_total_tokens = new_total;

        // Context, when the agent sent a `usage_update`. NOTE the honesty rule this
        // codebase already enforces (see `telemetry.rs`): ACP has **no quota concept
        // at all**, so `quota` stays empty rather than claiming a full budget. And
        // `used_percentage` is computed here only because ACP — unlike agy — does not
        // send one; `size` is the agent's own reported window, never invented.
        let usage = Usage {
            context: reply.context.map(|(used, size)| ContextUsage {
                used_tokens: used.min(u32::MAX as u64) as u32,
                context_window_size: size.min(u32::MAX as u64) as u32,
                used_percentage: if size > 0 { (used as f64 / size as f64) * 100.0 } else { 0.0 },
            }),
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

        Ok(TurnOutcome { message, used_tokens, permission: None, usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(total: u64) -> AcpUsage {
        AcpUsage::new(total, total / 2, total / 2)
    }

    /// **The delta, and why it exists.** ACP reports token usage *cumulatively across
    /// the session*, while `TurnOutcome::used_tokens` is what one turn cost. Feed the
    /// cumulative number straight through and a 3-turn session bills
    /// 100 + 250 + 400 = 750 tokens for what actually cost 400 — the ledger would
    /// grow quadratically in the length of the conversation.
    #[test]
    fn used_tokens_is_the_delta_not_the_cumulative_total() {
        // Turn 1: nothing seen before, so the whole total is this turn's cost.
        let (t1, w1) = turn_tokens(0, Some(&usage(100)));
        assert_eq!(t1, 100);
        assert_eq!(w1, 100, "watermark moves to the cumulative total");

        // Turn 2: the agent reports 250 cumulative. This turn cost 150, not 250.
        let (t2, w2) = turn_tokens(w1, Some(&usage(250)));
        assert_eq!(t2, 150);
        assert_eq!(w2, 250);

        // Turn 3: 400 cumulative → 150 this turn.
        let (t3, w3) = turn_tokens(w2, Some(&usage(400)));
        assert_eq!(t3, 150);
        assert_eq!(w3, 400);

        // The ledger sums the per-turn costs and lands on the agent's own total.
        assert_eq!(t1 + t2 + t3, 400);
    }

    /// An agent that does not implement the (unstable) end-turn usage extension
    /// reports nothing. Absent is absent: 0, and the watermark must NOT move — a
    /// moved watermark would make the *next* real reading come out as a bogus delta.
    #[test]
    fn an_agent_without_usage_reports_zero_and_does_not_move_the_watermark() {
        let (t, w) = turn_tokens(500, None);
        assert_eq!(t, 0);
        assert_eq!(w, 500, "watermark held");

        // and the next real reading is still a correct delta against it
        let (t2, _) = turn_tokens(w, Some(&usage(560)));
        assert_eq!(t2, 60);
    }

    /// A counter that goes backwards (the agent restarted its count, or reports
    /// per-turn despite the schema saying cumulative) must not silently drop the
    /// turn's cost to zero.
    #[test]
    fn a_backwards_counter_is_read_as_an_absolute_not_as_zero() {
        let (t, w) = turn_tokens(1_000, Some(&usage(42)));
        assert_eq!(t, 42, "not 0");
        assert_eq!(w, 42, "and the watermark follows the agent");
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
        let mut q = AcpQuark::new(QuarkId::new("dead"), Flavor::Worker, "", target);

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
            roster: vec![],
            field_window: vec![],
            field_truncated: false,
            git_diff: String::new(),
            cwd: std::env::temp_dir(),
            isolated: false,
            mode: Mode::Ask,
        }
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
            AcpTarget::claude_adapter(),
        );

        // --- Turn 1: does a prompt cross the wire and come back?
        let mut t1 = projection();
        t1.task = "Reply with exactly the word: pong. Nothing else.".into();
        let o1 = q.excite(t1).await.expect("live ACP turn 1");
        eprintln!("\n=== TURN 1 ===");
        eprintln!("message     : {:?}", o1.message);
        eprintln!("used_tokens : {}", o1.used_tokens);
        eprintln!("usage       : {:?}", o1.usage);

        let m1 = o1.message.as_deref().unwrap_or("").to_lowercase();
        assert!(m1.contains("pong"), "a real reply came back over ACP, got {m1:?}");

        // Real, structured, per-turn tokens — the thing the agy adapter cannot do.
        assert!(o1.used_tokens > 0, "end-turn token usage must be a real number, not 0");

        // --- Turn 2: the SESSION is resident. The agent was booted once and still
        // remembers turn 1 — so we can ask it about turn 1 without re-sending it.
        let mut t2 = projection();
        t2.task = "What single word did you just say? Reply with only that word.".into();
        let o2 = q.excite(t2).await.expect("live ACP turn 2");
        eprintln!("\n=== TURN 2 ===");
        eprintln!("message     : {:?}", o2.message);
        eprintln!("used_tokens : {}", o2.used_tokens);
        eprintln!("usage       : {:?}", o2.usage);
        eprintln!("cumulative watermark: {}", q.last_total_tokens);

        let m2 = o2.message.as_deref().unwrap_or("").to_lowercase();
        assert!(
            m2.contains("pong"),
            "the session is RESIDENT: turn 2 recalls turn 1 without us re-sending it, got {m2:?}"
        );

        // Turn 2 is billed for turn 2, not for the whole session — the delta is real.
        assert!(o2.used_tokens > 0, "turn 2 has its own cost");
        assert!(
            (o1.used_tokens as u64 + o2.used_tokens as u64) <= q.last_total_tokens.max(1),
            "per-turn deltas must not exceed the agent's own cumulative total \
             ({} + {} vs {})",
            o1.used_tokens,
            o2.used_tokens,
            q.last_total_tokens
        );
    }
}
