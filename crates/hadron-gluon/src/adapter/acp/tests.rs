use super::*;
use super::model::{model_selector, permission_choice, resolve_model, ModelSelector};
use super::session::AcpSession;
use super::spend::{turn_spend, SpendWatermark};

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use chrono::Utc;

use hadron_lattice::{live, Doing, Flavor, Mode, Projection, QuarkId};

use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionUpdate, Usage as AcpUsage,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};

use crate::adapter::registry::AcpTarget;

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
    let custom = AcpTarget { program: "goose".into(), args: vec!["acp".into()], env: Vec::new() };
    assert_eq!(custom.command_line(), "goose acp");
}

/// **The ACP carry path's pure core.** A resolved secret env must reach the
/// descriptor's `env` array in the exact shape `AcpAgent::from_str` (and the
/// underlying `McpServerStdio`/`EnvVariable` schema) expects — proven here by
/// round-tripping it through `AcpAgent::from_str` itself, not by re-implementing
/// the schema's shape as a second source of truth.
#[test]
fn acp_stdio_descriptor_includes_env() {
    let json = super::session::acp_stdio_descriptor(
        "python",
        &["a.py".to_string()],
        &[("GEMINI_API_KEY".to_string(), "k".to_string())],
    );

    let agent = AcpAgent::from_str(&json).expect("a well-formed stdio descriptor parses");
    let agent_client_protocol::schema::v1::McpServer::Stdio(stdio) = agent.into_server() else {
        panic!("expected a Stdio descriptor");
    };
    assert_eq!(stdio.command, std::path::PathBuf::from("python"));
    assert_eq!(stdio.args, vec!["a.py".to_string()]);
    assert_eq!(stdio.env.len(), 1);
    assert_eq!(stdio.env[0].name, "GEMINI_API_KEY");
    assert_eq!(stdio.env[0].value, "k");
}

/// **The no-secrets equivalence guarantee.** `boot` now ALWAYS builds a JSON
/// descriptor instead of a bare command string, so an ACP seat with no
/// `secret_env` must resolve to the exact same `command`/`args`/empty-`env` that
/// the old `AcpAgent::from_str(&target.command_line())` path produced — a seat
/// with no secrets must see no behaviour change at all.
#[test]
fn acp_stdio_descriptor_no_env_matches_bare_command() {
    let target = AcpTarget::claude_adapter();
    let json = super::session::acp_stdio_descriptor(&target.program, &target.args, &[]);

    let via_json = AcpAgent::from_str(&json).unwrap();
    let via_bare = AcpAgent::from_str(&target.command_line()).unwrap();

    let agent_client_protocol::schema::v1::McpServer::Stdio(json_stdio) = via_json.into_server() else {
        panic!("expected a Stdio descriptor");
    };
    let agent_client_protocol::schema::v1::McpServer::Stdio(bare_stdio) = via_bare.into_server() else {
        panic!("expected a Stdio descriptor");
    };

    // `name` is a cosmetic label only (`AcpAgent`'s stdio spawn never reads it —
    // see `spawn_process`), so it is deliberately excluded from this comparison;
    // only `command`/`args`/`env` affect what actually gets spawned.
    assert_eq!(json_stdio.command, bare_stdio.command);
    assert_eq!(json_stdio.args, bare_stdio.args);
    assert!(json_stdio.env.is_empty(), "no secret_env must mean an empty env array");
    assert_eq!(json_stdio.env.len(), bare_stdio.env.len());
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
        env: Vec::new(),
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
        nucleus_index: String::new(),
        nucleus_index_truncated: false,
        nucleus_index_path: std::path::PathBuf::new(),
        nucleus_notes_dir: std::path::PathBuf::new(),
        live_activities: vec![], roster: vec![],
        field_window: vec![],
        field_truncated: false,
        git_diff: String::new(),
        cwd: std::env::temp_dir(),
        isolated: false,
        mode: Mode::Ask,
        role_body: None,
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
    let preset = AcpTarget::for_vendor("agy").expect("agy is in the catalogue");
    let target = AcpTarget {
        program: root.join(&preset.program).display().to_string(),
        args: preset.args.iter().map(|a| root.join(a).display().to_string()).collect(),
        env: Vec::new(),
    };

    let outcome = super::model::probe(&target);

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
    t.mode = Mode::Bypass;
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

/// The live detail a human reads mid-turn: a kind verb + file for file tools,
/// the real command line for a shell call, the bare title only as a last resort.
#[test]
fn tool_call_detail_enriches_the_bare_title() {
    let call = |json: &str| -> agent_client_protocol::schema::v1::ToolCall {
        serde_json::from_str(json).expect("test ToolCall")
    };

    // A file tool with a location: verb + file name, not the title.
    let read = call(
        r#"{"toolCallId":"1","title":"Read","kind":"read",
            "locations":[{"path":"/repo/crates/hadron-gluon/src/lib.rs"}]}"#,
    );
    assert_eq!(session::tool_call_detail(&read), "Reading lib.rs");

    // A shell call: the actual command beats the generic "Terminal" title.
    let sh = call(
        r#"{"toolCallId":"2","title":"Terminal","kind":"execute",
            "rawInput":{"command":"cargo test --workspace"}}"#,
    );
    assert_eq!(session::tool_call_detail(&sh), "Running `cargo test --workspace`");

    // A long multi-line command is first-line-only and cut at 80 CHARS, not bytes.
    let long = format!(
        r#"{{"toolCallId":"3","title":"Terminal","kind":"execute",
            "rawInput":{{"command":"echo {}\nsecond line"}}}}"#,
        "é".repeat(100)
    );
    let detail = session::tool_call_detail(&call(&long));
    assert!(detail.starts_with("Running `echo ééé"));
    assert!(detail.ends_with("…`"));
    assert!(!detail.contains("second line"));

    // Nothing richer than the title → the title.
    let other = call(r#"{"toolCallId":"4","title":"Do a thing","kind":"other"}"#);
    assert_eq!(session::tool_call_detail(&other), "Do a thing");
}

/// A running tool's own output — a shell command's stdout, a file diff — beats
/// the static verb published when the call started, so the live card can show
/// the actual stream instead of a fixed "Running `cmd`" for the whole call.
#[test]
fn tool_call_update_detail_prefers_streamed_content_over_the_title() {
    let update = |json: &str| -> agent_client_protocol::schema::v1::ToolCallUpdate {
        serde_json::from_str(json).expect("test ToolCallUpdate")
    };

    // Streamed stdout content beats a title that didn't change.
    let stdout = update(
        r#"{"toolCallId":"1","title":"Terminal",
            "content":[{"type":"content","content":{"type":"text","text":"Compiling hadron-gluon..."}}]}"#,
    );
    assert_eq!(
        session::tool_call_update_detail(&stdout).as_deref(),
        Some("Compiling hadron-gluon...")
    );

    // A diff names the file it touched, not the raw before/after text.
    let diff = update(
        r#"{"toolCallId":"2",
            "content":[{"type":"diff","path":"/repo/src/lib.rs","newText":"fn main() {}"}]}"#,
    );
    assert_eq!(session::tool_call_update_detail(&diff).as_deref(), Some("Edited lib.rs"));

    // No content → fall back to a title update, if any.
    let title_only = update(r#"{"toolCallId":"3","title":"Completed"}"#);
    assert_eq!(session::tool_call_update_detail(&title_only).as_deref(), Some("Completed"));

    // A bare status flip with neither content nor title has nothing to show —
    // the caller must leave the previous detail alone rather than blank it.
    let status_only = update(r#"{"toolCallId":"4","status":"completed"}"#);
    assert_eq!(session::tool_call_update_detail(&status_only), None);
}

/// The regression this exists for: a worker's final `@orchestrator` report
/// arrived as a separate `AgentMessageChunk` notification right after a
/// narration sentence, with no whitespace of its own. Glued with a bare
/// `push_str`, `"...now.@orchestrator done"` is one line — `parse_all_addressees`
/// only matches a mention that STARTS a line, so the report silently routed to
/// nobody. A paragraph break between notifications restores the line start.
#[test]
fn append_message_chunk_separates_notifications_so_a_trailing_mention_starts_a_line() {
    let mut transcript = String::new();
    session::append_message_chunk(&mut transcript, "Committing now.");
    session::append_message_chunk(&mut transcript, "@orchestrator Task 2 complete.");
    assert!(
        transcript.lines().any(|l| l == "@orchestrator Task 2 complete."),
        "transcript: {transcript:?}"
    );
}

/// No spurious blank line when a chunk already carries its own leading/trailing
/// whitespace (the common case: the model's own text already has "\n\n" before
/// a fresh paragraph) — the separator is only inserted when truly needed.
#[test]
fn append_message_chunk_does_not_double_up_existing_whitespace() {
    let mut transcript = String::new();
    session::append_message_chunk(&mut transcript, "First paragraph.\n\n");
    session::append_message_chunk(&mut transcript, "@orchestrator second paragraph.");
    assert_eq!(transcript, "First paragraph.\n\n@orchestrator second paragraph.");
}

/// The first chunk never gets a leading separator.
#[test]
fn append_message_chunk_first_chunk_has_no_leading_separator() {
    let mut transcript = String::new();
    session::append_message_chunk(&mut transcript, "hello");
    assert_eq!(transcript, "hello");
}

/// **The bug that made `acp-agy` bill tokens and post nothing.** The Python SDK
/// adapter (`scripts/agy_acp.py`) hand-rolls the `session/update` JSON, and the
/// Rust client here is the only thing that ever deserializes it — the Rust tests
/// exercised Rust *types*, never the Python's actual output, so a wire mismatch
/// sailed through. `SessionUpdate` is `#[serde(tag = "sessionUpdate")]`: the
/// discriminator MUST be `sessionUpdate`, not `type`. Feed the exact object the
/// adapter emits and prove it lands as `AgentMessageChunk` with the text intact.
#[test]
fn the_python_adapters_message_update_deserializes_to_a_text_chunk() {
    let update: SessionUpdate = serde_json::from_value(serde_json::json!({
        "sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "PONG"}
    }))
    .expect("the adapter's `sessionUpdate`-tagged object must deserialize");

    match update {
        SessionUpdate::AgentMessageChunk(chunk) => match chunk.content {
            ContentBlock::Text(t) => assert_eq!(t.text, "PONG"),
            other => panic!("expected a text content block, got {other:?}"),
        },
        other => panic!("expected AgentMessageChunk, got {other:?}"),
    }
}

/// The negative control: the OLD shape the adapter used to send — discriminator
/// `type` instead of `sessionUpdate` — does NOT deserialize. This is exactly why
/// every chunk was silently dropped and the turn posted an empty message.
#[test]
fn the_old_type_tagged_shape_fails_to_deserialize() {
    let result: Result<SessionUpdate, _> = serde_json::from_value(serde_json::json!({
        "type": "agent_message_chunk",
        "content": {"type": "text", "text": "PONG"}
    }));
    assert!(
        result.is_err(),
        "a `type`-tagged update must fail — if this ever passes, the wire drift is back"
    );
}

/// The usage update travels the same path and must survive the same contract.
#[test]
fn the_python_adapters_usage_update_deserializes() {
    let update: SessionUpdate = serde_json::from_value(serde_json::json!({
        "sessionUpdate": "usage_update",
        "used": 13570,
        "size": 2000000
    }))
    .expect("the adapter's usage update must deserialize");

    match update {
        SessionUpdate::UsageUpdate(u) => {
            assert_eq!(u.used, 13570);
            assert_eq!(u.size, 2000000);
        }
        other => panic!("expected UsageUpdate, got {other:?}"),
    }
}

/// **The Settings model probe's boundary.** `agy_acp.py` now answers `session/new`
/// with its static model list and WITHOUT booting the SDK — so model detection no
/// longer needs the API key or a live Google connection (that dependency is why the
/// probe failed with "handshake failed …" whenever the key/keychain or Google was
/// momentarily unreachable, and it billed a connection on every Settings open). The
/// Rust client is the only thing that deserializes that response, so — per the
/// wire-contract lesson — assert the EXACT JSON the adapter emits deserializes to a
/// `NewSessionResponse` and that `model_selector` finds the model in it. This is the
/// proof the dropdown populates; the boot is deferred to the first prompt.
#[test]
fn the_python_adapters_session_new_response_yields_the_model_selector() {
    use agent_client_protocol::schema::v1::NewSessionResponse;

    // Byte-for-byte the object session/new returns (camelCase, `category: "model"`).
    let resp: NewSessionResponse = serde_json::from_value(serde_json::json!({
        "sessionId": "60a83257-f0dc-46b4-a21e-bd7076ab1bd9",
        "configOptions": [{
            "id": "model",
            "name": "Model",
            "type": "select",
            "category": "model",
            "currentValue": "gemini-3.6-flash",
            "options": [{"value": "gemini-3.6-flash", "name": "gemini-3.6-flash"}]
        }]
    }))
    .expect("the adapter's session/new response must deserialize to NewSessionResponse");

    let opts = resp.config_options.unwrap_or_default();
    let selector = model_selector(&opts).expect("the model selector must be found by category");
    assert_eq!(selector.current, "gemini-3.6-flash");
    assert_eq!(selector.available.len(), 1);
    assert_eq!(selector.available[0].value, "gemini-3.6-flash");
}

#[test]
fn native_edit_request_is_detected_and_rejected() {
    use agent_client_protocol::schema::v1::{
        PermissionOption, PermissionOptionKind, RequestPermissionRequest, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    };

    let make_req = |kind: Option<ToolKind>, title: Option<&str>| -> RequestPermissionRequest {
        let mut fields = ToolCallUpdateFields::default();
        fields.kind = kind;
        fields.title = title.map(String::from);

        RequestPermissionRequest::new(
            "sess1",
            ToolCallUpdate::new("tc1", fields),
            vec![
                PermissionOption::new("opt_allow", "Allow", PermissionOptionKind::AllowOnce),
                PermissionOption::new("opt_reject", "Reject", PermissionOptionKind::RejectOnce),
            ],
        )
    };

    assert!(session::is_native_edit_request(&make_req(Some(ToolKind::Edit), None)));
    assert!(session::is_native_edit_request(&make_req(None, Some("Edit"))));
    assert!(session::is_native_edit_request(&make_req(None, Some("Write"))));
    assert!(session::is_native_edit_request(&make_req(None, Some("MultiEdit"))));
    assert!(session::is_native_edit_request(&make_req(None, Some("fs/write_text_file"))));
    assert!(!session::is_native_edit_request(&make_req(Some(ToolKind::Read), Some("Read"))));
    assert!(!session::is_native_edit_request(&make_req(None, Some("hadron_forge_edit"))));
}
