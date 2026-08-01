use super::*;
use super::nucleus::{
    build_invariants, event_cost, nucleus_index_path, nucleus_notes_dir, read_nucleus_index,
    read_nucleus_index_with_fallback, FIELD_WINDOW_BUDGET_BYTES, NUCLEUS_INDEX_BUDGET,
};
use crate::field::{append_event, read_events};
use crate::mock::MockQuark;
use crate::router::next_pending;
use hadron_lattice::{Mode, QuarkState};
use std::fs;
use tokio::task::AbortHandle;

/// The daemon is launched as `hadron-gluon .hadron/field.jsonl` — a *relative*
/// path. That path's ancestors end in the empty path, and `"".join(".hadron")`
/// resolves against the process cwd, so the old ancestor search "found" a
/// workspace root of `""`. That empty root rode the projection down to
/// `Command::current_dir("")`, which the kernel answers with ENOENT — surfacing
/// as `failed to spawn claude: No such file or directory`, blaming a binary that
/// was on PATH the whole time.
///
/// The root must be the real workspace directory, and it must exist.
#[test]
fn a_relative_field_path_resolves_to_a_real_workspace_root() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().canonicalize().unwrap();
    fs::create_dir_all(workspace.join(".hadron")).unwrap();

    let root = workspace_root_of(Path::new(".hadron/field.jsonl"), &workspace);

    assert_eq!(root, workspace, "relative field path must resolve to its workspace");
    assert!(!root.as_os_str().is_empty(), "an empty root becomes current_dir(\"\") → ENOENT");
    assert!(root.is_dir(), "the CLI's cwd must be a directory that exists");
}

/// An absolute field path keeps working, and still finds the `.hadron` owner
/// rather than just the file's parent.
#[test]
fn an_absolute_field_path_finds_its_workspace_root() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().canonicalize().unwrap();
    fs::create_dir_all(workspace.join(".hadron")).unwrap();

    let root = workspace_root_of(&workspace.join(".hadron/field.jsonl"), Path::new("/nowhere"));

    assert_eq!(root, workspace);
}
use hadron_lattice::{Actor, EnergyState, Flavor, Kind, PermissionAsk, Projection, QuarkId, TurnOutcome};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;

/// Asks for permission on excite #1, replies on later excites, and records the
/// `task` it was handed each excite — so a test can prove task context survives
/// a resume (the load-bearing trigger-finder fix).
struct PermissionQuark {
    id: QuarkId,
    flavor: Flavor,
    ask: PermissionAsk,
    reply: String,
    calls: usize,
    tasks: Arc<Mutex<Vec<String>>>,
    /// This seat's config command allow/deny lists (see
    /// `crate::quark::Quark::commands`) — defaults to empty in every
    /// constructor below; the No-Human-Mode config-fold tests override it.
    commands: hadron_lattice::SeatCommands,
}

#[async_trait::async_trait]
impl crate::quark::Quark for PermissionQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        self.flavor.clone()
    }
    fn commands(&self) -> &hadron_lattice::SeatCommands {
        &self.commands
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        self.tasks.lock().unwrap().push(turn.task.clone());
        self.calls += 1;
        if self.calls == 1 {
            Ok(TurnOutcome { message: None, permission: Some(self.ask.clone()), ..Default::default()})
        } else {
            // Resumed: a denial is the SAME resume as a grant (the engine never
            // inspects `approved` — see `finish_turn`'s permission block) — it is
            // the resumed quark's own job to read its context and decline. This
            // mirrors the "existing AskHuman-denied semantics" the No-Human-Mode
            // plan reuses: check the most recent grant addressed to us in the
            // field window handed back on resume, and refuse the op if it was a
            // denial. Additive: every existing caller only ever grants
            // `approved: true`, so this branch is unreachable for them and their
            // assertions are unaffected.
            let denied = turn
                .field_window
                .iter()
                .rev()
                .find_map(|e| match (&e.to, &e.kind) {
                    (Some(to), Kind::PermissionGrant { approved, .. }) if to == &self.id => Some(!approved),
                    _ => None,
                })
                .unwrap_or(false);
            let message = if denied { format!("{} refused", self.reply) } else { self.reply.clone() };
            Ok(TurnOutcome { message: Some(message), permission: None, ..Default::default()})
        }
    }
}

fn perm_quark(id: &str, tasks: Arc<Mutex<Vec<String>>>) -> PermissionQuark {
    perm_quark_risk(id, tasks, hadron_gatekeeper::Risk::BashExec, "cargo publish", "published")
}

/// A permission quark with a chosen risk/op, so tests can exercise the edit
/// vs bash branches of the mode ladder.
fn perm_quark_risk(
    id: &str,
    tasks: Arc<Mutex<Vec<String>>>,
    risk: hadron_gatekeeper::Risk,
    desc: &str,
    reply: &str,
) -> PermissionQuark {
    PermissionQuark {
        id: QuarkId::new(id),
        flavor: Flavor::Orchestrator,
        ask: PermissionAsk { risk, description: desc.into() },
        reply: reply.into(),
        calls: 0,
        tasks,
        commands: hadron_lattice::SeatCommands::default(),
    }
}

/// Same double as `perm_quark_risk`, flavored `Worker` instead of the
/// historical (and here load-bearing-to-avoid) `Orchestrator` default:
/// `effective_mode`'s No-Human-Mode clamp only fires for a non-orchestrator
/// seat, so a wrongly-flavored asker would silently defeat the very clamp
/// the No-Human-Mode tests exist to prove.
fn perm_worker_risk(
    id: &str,
    tasks: Arc<Mutex<Vec<String>>>,
    risk: hadron_gatekeeper::Risk,
    desc: &str,
    reply: &str,
) -> PermissionQuark {
    PermissionQuark {
        id: QuarkId::new(id),
        flavor: Flavor::Worker,
        ask: PermissionAsk { risk, description: desc.into() },
        reply: reply.into(),
        calls: 0,
        tasks,
        commands: hadron_lattice::SeatCommands::default(),
    }
}

fn has_kind(events: &[Event], pred: impl Fn(&Kind) -> bool) -> bool {
    events.iter().any(|e| pred(&e.kind))
}

/// Records the `mode` on the projection it is handed, then quiesces in one
/// turn (a plain reply, no permission ask) — so a test can prove the engine
/// resolved and delivered the quark's effective mode before excitation.
struct ModeSpyQuark {
    id: QuarkId,
    seen: Arc<Mutex<Vec<hadron_gatekeeper::Mode>>>,
}

#[async_trait::async_trait]
impl crate::quark::Quark for ModeSpyQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        self.seen.lock().unwrap().push(turn.mode);
        Ok(TurnOutcome { message: Some("ok".into()), permission: None, ..Default::default()})
    }
}

#[tokio::test]
async fn engine_delivers_resolved_mode_on_the_projection() {
    use hadron_gatekeeper::Mode;
    // No ModeSet → the quark's turn runs under the default Ask.
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    let seen = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(ModeSpyQuark { id: QuarkId::new("agy"), seen: seen.clone() })],
        8,
    );
    engine.run_until_quiesce().await.unwrap();
    assert_eq!(seen.lock().unwrap().as_slice(), &[Mode::Ask], "default is Ask");

    // A per-quark override for agy → its next turn runs under Bypass.
    seed_mode(&field, Some("agy"), Mode::Bypass);
    seed_human_message(&field, "agy", "again");
    engine.run_until_quiesce().await.unwrap();
    assert_eq!(
        seen.lock().unwrap().last().copied(),
        Some(Mode::Bypass),
        "per-quark ModeSet reached the projection"
    );
}

/// The presence pair: a quark excites *before* its turn and grounds after, so
/// the chamber can render it working for the whole (slow) duration of a turn.
#[tokio::test]
async fn excitation_is_announced_before_the_turn_and_grounded_after() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(MockQuark::scripted(
            QuarkId::new("agy"),
            Flavor::Worker,
            vec![Some("done".into())],
        ))],
        8,
    );
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    let states: Vec<QuarkState> = events
        .iter()
        .filter_map(|e| match &e.kind {
            Kind::Status { state } => Some(state.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        states,
        vec![QuarkState::Excited, QuarkState::Ground],
        "excited then ground, in that order"
    );

    // The excitation must land before the reply, or the chamber would only
    // learn the quark was working once it had already stopped working.
    let excited_ix = events
        .iter()
        .position(|e| matches!(e.kind, Kind::Status { state: QuarkState::Excited }))
        .expect("excited emitted");
    let reply_ix = events
        .iter()
        .position(|e| matches!(&e.kind, Kind::Message { body } if body == "done"))
        .expect("reply emitted");
    assert!(excited_ix < reply_ix, "excited precedes the reply");
}

/// A turn that fails must still leave a terminal status behind — otherwise the
/// quark reads as forever-working in the roster.
#[tokio::test]
async fn a_failed_turn_does_not_strand_the_quark_as_excited() {
    struct FailingQuark;
    #[async_trait::async_trait]
    impl Quark for FailingQuark {
        fn id(&self) -> QuarkId {
            QuarkId::new("agy")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            Err(anyhow::anyhow!("cli blew up"))
        }
    }

    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    let mut engine = Engine::new(field.clone(), vec![Box::new(FailingQuark)], 8);
    assert!(engine.run_until_quiesce().await.is_err(), "the failure propagates");

    let events = read_events(&field).unwrap();
    let last_state = events
        .iter()
        .filter_map(|e| match &e.kind {
            Kind::Status { state } => Some(state.clone()),
            _ => None,
        })
        .next_back();
    assert_eq!(
        last_state,
        Some(QuarkState::Error),
        "the quark ends Error, not stranded Excited"
    );
}

#[tokio::test]
async fn failing_quark_turn_sends_error_message_to_orchestrator() {
    struct FailingWorker;

    #[async_trait::async_trait]
    impl crate::quark::Quark for FailingWorker {
        fn id(&self) -> QuarkId {
            QuarkId::new("agy")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            Err(anyhow::anyhow!("cli blew up"))
        }
    }

    struct OrchQuark;

    #[async_trait::async_trait]
    impl crate::quark::Quark for OrchQuark {
        fn id(&self) -> QuarkId {
            QuarkId::new("opus")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Orchestrator
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            Ok(TurnOutcome {
                message: Some("acknowledged".into()),
                permission: None,
                ..Default::default()
            })
        }
    }

    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(FailingWorker), Box::new(OrchQuark)],
        8,
    );
    let _ = engine.run_until_quiesce().await;

    let events = read_events(&field).unwrap();
    let gluon_msg = events.iter().find(|e| {
        e.from == Actor::Gluon
            && matches!(&e.kind, Kind::Message { body } if body.contains("errored"))
    });
    assert!(
        gluon_msg.is_some(),
        "gluon should emit an error message event"
    );
    let body = match &gluon_msg.unwrap().kind {
        Kind::Message { body } => body,
        _ => unreachable!(),
    };
    assert!(
        body.starts_with("@orchestrator Quark `agy` turn errored: cli blew up"),
        "error message must address orchestrator when one exists: got {body}"
    );
}

/// **THE discriminating test for the turn watchdog.**
///
/// The production failure, exactly: a quark is excited, its process dies (or
/// orphans the pipe the adapter is waiting on) and the turn future NEVER
/// RESOLVES. Nothing in the engine ever ends that turn: `run_until_quiesce`
/// cannot quiesce while a turn is in flight, so the dispatch loop wedges — no
/// `Ground`, no `Error`, no re-dispatch, and the quark is lost.
///
/// Before the deadline existed this test did not *fail*, it HUNG: the outer
/// `timeout` below is what turns the wedge into a red test instead of a stuck
/// suite. After it: the quark ends `Error`, and a new message re-excites it.
#[tokio::test]
async fn a_turn_whose_process_dies_without_an_outcome_is_ended_by_the_watchdog() {
    /// Excite #1 never returns — a turn whose process is gone. Later excites
    /// answer normally, which is what proves the quark is not stranded.
    struct VanishingQuark {
        calls: usize,
    }
    #[async_trait::async_trait]
    impl Quark for VanishingQuark {
        fn id(&self) -> QuarkId {
            QuarkId::new("agy")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            self.calls += 1;
            if self.calls == 1 {
                // The vanished process: no outcome, no error, ever.
                std::future::pending::<()>().await;
                unreachable!("pending() never resolves");
            }
            Ok(TurnOutcome {
                message: Some("back from the dead".into()),
                permission: None,
                ..Default::default()
            })
        }
    }

    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    let mut engine = Engine::new(field.clone(), vec![Box::new(VanishingQuark { calls: 0 })], 8)
        .with_turn_deadline(Duration::from_millis(200));

    // The wedge, made visible: WITHOUT the watchdog this never returns.
    let result = tokio::time::timeout(Duration::from_secs(5), engine.run_until_quiesce())
        .await
        .expect("the engine must not wedge forever on a turn that never returns");
    let err = result.expect_err("the watchdog ends the turn as a failure");
    assert!(
        err.to_string().contains("deadline"),
        "the error must say WHY the turn ended: {err}"
    );

    // The quark is not stranded Excited: it has a terminal status.
    let events = read_events(&field).unwrap();
    let states: Vec<QuarkState> = events
        .iter()
        .filter_map(|e| match &e.kind {
            Kind::Status { state } => Some(state.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        states,
        vec![QuarkState::Excited, QuarkState::Error],
        "excited, then ENDED — the watchdog wrote the terminal status the turn never did"
    );

    // …and it is excitable again. (Deliberately NOT re-excited by the *same*
    // message — the `Error` counts as the quark having answered, which is what
    // keeps a permanently-hanging quark from spinning the deadline forever. A
    // NEW message is what brings it back.)
    seed_human_message(&field, "agy", "you there?");
    tokio::time::timeout(Duration::from_secs(5), engine.run_until_quiesce())
        .await
        .expect("no wedge")
        .expect("the second turn runs normally");
    let events = read_events(&field).unwrap();
    assert!(
        has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "back from the dead")),
        "a quark the watchdog reaped must be excitable again"
    );
}

/// The watchdog measures SILENCE, not elapsed time. A turn that is genuinely
/// working — publishing mid-turn activity into `<field-dir>/live/` — must survive
/// far past the deadline, or a long-but-healthy turn is killed and its work lost.
///
/// Observed live 2026-07-30: a real `acp-claude` turn running a long comparison
/// suite was reaped at 1800s with the message "was excited for 1800s and its turn
/// never returned", while it was in fact still working.
#[tokio::test]
async fn a_turn_that_keeps_publishing_activity_outlives_the_deadline() {
    struct ChattyQuark {
        live_dir: std::path::PathBuf,
    }
    #[async_trait::async_trait]
    impl Quark for ChattyQuark {
        fn id(&self) -> QuarkId {
            QuarkId::new("agy")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            // Works for 10x the deadline, saying so the whole time.
            for _ in 0..20 {
                hadron_lattice::live::publish(
                    &self.live_dir,
                    &hadron_lattice::live::Activity::new(
                        QuarkId::new("agy"),
                        hadron_lattice::live::Doing::Working,
                        "still running the suite",
                    ),
                )
                .unwrap();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok(TurnOutcome {
                message: Some("suite green".into()),
                permission: None,
                ..Default::default()
            })
        }
    }

    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "run the long suite");
    let live_dir = hadron_lattice::live::live_dir(&field);
    std::fs::create_dir_all(&live_dir).unwrap();

    let mut engine = Engine::new(field.clone(), vec![Box::new(ChattyQuark { live_dir })], 8)
        .with_turn_deadline(Duration::from_millis(100));

    tokio::time::timeout(Duration::from_secs(10), engine.run_until_quiesce())
        .await
        .expect("no wedge")
        .expect("a turn that is visibly working must not be reaped");

    let events = read_events(&field).unwrap();
    assert!(
        has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "suite green")),
        "the turn ran to completion despite outliving the deadline many times over"
    );
}

/// Seed a mode-set event into the field before serving. `to = None` sets the
/// global default; `Some(quark)` sets a per-quark override.
fn seed_mode(field: &std::path::Path, to: Option<&str>, mode: hadron_gatekeeper::Mode) {
    append_event(
        field,
        &Event::new(Actor::Human, to.map(QuarkId::new), Kind::ModeSet { mode }),
    )
    .unwrap();
}

#[tokio::test]
async fn ask_mode_default_pauses_for_human() {
    // No ModeSet in the field → global default is Ask → a bash op pauses.
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(field.clone(), vec![Box::new(perm_quark("agy", tasks.clone()))], 8);
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(has_kind(&events, |k| matches!(k, Kind::PermissionReq { .. })), "req recorded");
    assert!(!has_kind(&events, |k| matches!(k, Kind::PermissionGrant { .. })), "no auto-grant under Ask");
    assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "quark waits");
    assert!(!has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published")), "op not performed yet");
    assert!(
        hadron_gatekeeper::pending_permission(&events, &QuarkId::new("agy")).is_some(),
        "chamber can surface the request"
    );
}

#[tokio::test]
async fn human_grant_resumes_the_quark_with_its_task() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(field.clone(), vec![Box::new(perm_quark("agy", tasks.clone()))], 8);
    engine.run_until_quiesce().await.unwrap();

    // Human approves, addressed to the quark.
    append_event(
        &field,
        &Event::new(Actor::Human, Some(QuarkId::new("agy")), Kind::PermissionGrant { approved: true, remember: false }),
    )
    .unwrap();
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published")), "op performed after grant");
    // THE FIX: the resumed excite got the original task, not the grant's empty context.
    let recorded = tasks.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2, "asked once, resumed once");
    assert_eq!(recorded[1], "hello", "resumed quark kept its task");
}

#[tokio::test]
async fn multi_mention_message_fans_out_to_each_named_quark() {
    // "@orch do X and you @worker do Y" (unaddressed, to: None — as the chamber
    // now writes it) must excite BOTH quarks, in mention order, each handed the
    // FULL message. This is the core multi-dispatch behavior.
    use hadron_lattice::{Projection, TurnOutcome};
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "@orch do X and you @worker do Y".into() },
        ),
    )
    .unwrap();

    struct Spy {
        id: &'static str,
        flavor: Flavor,
        seen: Arc<Mutex<Vec<String>>>,
    }
    #[async_trait::async_trait]
    impl crate::quark::Quark for Spy {
        fn id(&self) -> QuarkId {
            QuarkId::new(self.id)
        }
        fn flavor(&self) -> Flavor {
            self.flavor.clone()
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            self.seen.lock().unwrap().push(format!("{}:{}", self.id, turn.task));
            // Reply with no @mention → hand back, so the loop advances to the
            // next unserved addressee rather than a hand-off chain.
            Ok(TurnOutcome { message: Some(format!("{} done", self.id)), permission: None, ..Default::default()})
        }
    }

    let seen = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        path.clone(),
        vec![
            Box::new(Spy { id: "orch", flavor: Flavor::Orchestrator, seen: seen.clone() }),
            Box::new(Spy { id: "worker", flavor: Flavor::Worker, seen: seen.clone() }),
        ],
        10,
    );
    engine.run_until_quiesce().await.unwrap();

    let s = seen.lock().unwrap().clone();
    assert_eq!(
        s,
        vec![
            "orch:@orch do X and you @worker do Y".to_string(),
            "worker:@orch do X and you @worker do Y".to_string(),
        ],
        "both named quarks ran in mention order, each seeing the whole message"
    );
}

#[tokio::test]
async fn to_none_mention_message_resumes_the_quark_with_its_task() {
    // THE DISCRIMINATING TEST (advisor-flagged regression): the real chamber
    // writes human messages `to: None` with mentions in the BODY. A quark that
    // asks permission and is then granted must resume with its ORIGINAL task,
    // recovered from that driving (to:None) message — not an empty string. The
    // old `to == target` task-finder returns "" here; the addressee-resolving
    // fallback recovers it. `seed_human_message` (to:Some) can't catch this.
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@agy please publish".into() }),
    )
    .unwrap();
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(field.clone(), vec![Box::new(perm_quark("agy", tasks.clone()))], 8);
    engine.run_until_quiesce().await.unwrap();
    // Paused for the human under default Ask.
    let events = read_events(&field).unwrap();
    assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "asked, waiting");

    // Human approves (addressed to the quark, as the chamber writes a grant).
    append_event(
        &field,
        &Event::new(Actor::Human, Some(QuarkId::new("agy")), Kind::PermissionGrant { approved: true, remember: false }),
    )
    .unwrap();
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published")), "op performed after grant");
    let recorded = tasks.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2, "asked once, resumed once");
    assert_eq!(recorded[1], "@agy please publish", "resumed quark kept its task, not an empty string");
}

/// Helper: run a quark of the given risk/op under a seeded global mode and
/// return the resulting field events.
async fn serve_under_mode(
    mode: hadron_gatekeeper::Mode,
    risk: hadron_gatekeeper::Risk,
    desc: &str,
) -> (Vec<Event>, Arc<Mutex<Vec<String>>>) {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, mode);
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(perm_quark_risk("agy", tasks.clone(), risk, desc, "done"))],
        8,
    );
    engine.run_until_quiesce().await.unwrap();
    // Keep the tempdir alive by reading before it drops.
    (read_events(&field).unwrap(), tasks)
}

fn gluon_auto_granted(events: &[Event]) -> bool {
    events
        .iter()
        .any(|e| e.from == Actor::Gluon && matches!(e.kind, Kind::PermissionGrant { approved: true, .. }))
}

#[tokio::test]
async fn write_mode_auto_approves_edit_but_pauses_on_bash() {
    use hadron_gatekeeper::{Mode, Risk};
    // Edit under Write → auto-approved and completed.
    let (events, tasks) = serve_under_mode(Mode::Write, Risk::WorkspaceEdit, "patch src/main.rs").await;
    assert!(gluon_auto_granted(&events), "edit auto-granted under Write");
    assert!(has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "done")), "edit completed");
    assert_eq!(tasks.lock().unwrap()[1], "hello", "task survived the auto-resume");

    // Bash under Write → pauses for the human.
    let (events, _) = serve_under_mode(Mode::Write, Risk::BashExec, "cargo publish").await;
    assert!(!gluon_auto_granted(&events), "bash NOT auto-granted under Write");
    assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "bash waits for human");
}

#[tokio::test]
async fn bypass_mode_auto_approves_bash() {
    use hadron_gatekeeper::{Mode, Risk};
    let (events, _) = serve_under_mode(Mode::Bypass, Risk::BashExec, "cargo publish").await;
    assert!(has_kind(&events, |k| matches!(k, Kind::PermissionReq { .. })), "req still recorded (audit)");
    assert!(gluon_auto_granted(&events), "bash auto-granted under Bypass");
    assert!(has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "done")), "op completed with no human");
}

#[tokio::test]
async fn auto_mode_pauses_on_unlisted_then_honors_a_remembered_command() {
    use hadron_gatekeeper::{Mode, Risk};
    // Unlisted command under Auto → pauses.
    let (events, _) = serve_under_mode(Mode::Auto, Risk::BashExec, "cargo publish").await;
    assert!(!gluon_auto_granted(&events), "unlisted bash pauses under Auto");
    assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "waits");

    // Now with a prior remembered grant for the SAME (quark, op) → auto-approved.
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Auto);
    // Teach the rule: a prior req + an "always allow" grant for the same op.
    append_event(&field, &Event::new(Actor::Quark(QuarkId::new("agy")), None,
        Kind::PermissionReq { risk: Risk::BashExec, description: "cargo publish".into() })).unwrap();
    append_event(&field, &Event::new(Actor::Human, Some(QuarkId::new("agy")),
        Kind::PermissionGrant { approved: true, remember: true })).unwrap();
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(field.clone(),
        vec![Box::new(perm_quark_risk("agy", tasks.clone(), Risk::BashExec, "cargo publish", "done"))], 8);
    engine.run_until_quiesce().await.unwrap();
    let events = read_events(&field).unwrap();
    assert!(gluon_auto_granted(&events), "remembered command auto-granted under Auto");
    assert!(has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "done")), "op completed");
}

#[tokio::test]
async fn per_quark_bypass_override_beats_global_ask() {
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Ask); // global: ask for everything
    seed_mode(&field, Some("agy"), Mode::Bypass); // but agy is trusted
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(field.clone(),
        vec![Box::new(perm_quark_risk("agy", tasks.clone(), Risk::BashExec, "cargo publish", "done"))], 8);
    engine.run_until_quiesce().await.unwrap();
    let events = read_events(&field).unwrap();
    assert!(gluon_auto_granted(&events), "per-quark Bypass override auto-grants despite global Ask");
}

// ---- No-Human-Mode (spec §2 D): toggle, suspend, resume, denial ----
//
// A fake orchestrator standing in for a real one's future "grant tool":
// when excited it appends a `PermissionGrant` DIRECTLY to the field —
// exactly how the chamber appends a human's grant when they click
// Approve/Deny — rather than replying with a message the engine would have
// to parse. This is deliberate (see the advisor consult in the task
// report): the engine's job stops at putting the request in front of the
// orchestrator (`Engine::orchestrator_adjudication_message` +
// `run_until_quiesce`'s auto-scheduler); how a real orchestrator turns its
// judgement into a `PermissionGrant` event is a capability of THAT actor
// (a tool call), not a translation the engine performs — so this double
// proves the scheduler → resume path end-to-end without inventing one.
struct GrantingOrchestrator {
    id: QuarkId,
    field_path: std::path::PathBuf,
    grant_to: QuarkId,
    approved: bool,
}

#[async_trait::async_trait]
impl crate::quark::Quark for GrantingOrchestrator {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Orchestrator
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
        append_event(
            &self.field_path,
            &Event::new(
                Actor::Quark(self.id.clone()),
                Some(self.grant_to.clone()),
                Kind::PermissionGrant { approved: self.approved, remember: false },
            ),
        )?;
        Ok(TurnOutcome { message: Some("adjudicated".into()), permission: None, ..Default::default()})
    }
}

/// Toggle OFF (the default — no `with_no_human` call): a worker's
/// non-allow-listed bash under global Bypass auto-approves exactly as
/// today. Not parked, not orchestrator-adjudicated — `no_human = false`
/// makes `decide` byte-for-byte the pre-Task-3 table.
#[tokio::test]
async fn toggle_off_never_asks_orchestrator() {
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Bypass); // global Bypass
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(perm_worker_risk("agy", tasks.clone(), Risk::BashExec, "rm -rf /tmp/x", "done"))],
        8,
    );
    // No `.with_no_human(true)` — toggle stays off.
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(gluon_auto_granted(&events), "global Bypass auto-grants exactly as today");
    assert!(!has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "never parked");
    assert!(
        !events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("no-human-mode"))),
        "the orchestrator is never consulted"
    );
}

/// Toggle ON, global Bypass: a worker (no per-quark override) clamps to
/// `Auto`, its op is not allow-listed, and `decide` returns
/// `AskOrchestrator` — which parks the SAME way `AskHuman` does (Waiting +
/// PermissionReq), and the auto-scheduler puts the request in front of the
/// orchestrator (a `[no-human-mode]`-marked `Message`).
#[tokio::test]
async fn worker_suspends_on_askorchestrator() {
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Bypass); // global Bypass
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(perm_worker_risk("agy", tasks.clone(), Risk::BashExec, "cargo publish", "done")),
            // No orchestrator seated: the scheduler must still park the
            // worker (its job is independent of whether it can find
            // someone to escalate to) and simply find nothing to schedule.
        ],
        8,
    )
    .with_no_human(true);
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(has_kind(&events, |k| matches!(k, Kind::PermissionReq { .. })), "req recorded");
    assert!(!has_kind(&events, |k| matches!(k, Kind::PermissionGrant { .. })), "not yet granted");
    assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "worker parks");
}

/// The full loop: worker suspends on `AskOrchestrator`, the auto-scheduler
/// (on quiesce) puts the request in front of the seated orchestrator, whose
/// turn appends a `PermissionGrant{approved:true}` — and the worker resumes
/// and proceeds, via the EXISTING grant→resume mechanism (`next_pending`
/// treats any `PermissionGrant` addressed to a quark as a turn request,
/// regardless of which actor authored it — no new resume code was needed).
#[tokio::test]
async fn orchestrator_grant_resumes_worker() {
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Bypass);
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(perm_worker_risk("agy", tasks.clone(), Risk::BashExec, "cargo publish", "published")),
            Box::new(GrantingOrchestrator {
                id: QuarkId::new("orch"),
                field_path: field.clone(),
                grant_to: QuarkId::new("agy"),
                approved: true,
            }),
        ],
        8,
    )
    .with_no_human(true);
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.to.as_ref() == Some(&QuarkId::new("orch"))
                && matches!(&e.kind, Kind::Message { body } if body.starts_with("[no-human-mode]"))),
        "the orchestrator was auto-scheduled with the injected request"
    );
    assert!(
        has_kind(&events, |k| matches!(k, Kind::PermissionGrant { approved: true, .. })),
        "the orchestrator's grant landed"
    );
    assert!(
        has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published")),
        "the worker resumed and performed the op"
    );
    assert_eq!(tasks.lock().unwrap().len(), 2, "asked once, resumed once");
}

/// The same loop, but the orchestrator DENIES: the worker still resumes
/// (the engine never inspects `approved` — see `finish_turn`'s permission
/// block) but, seeing the denial in its own context on resume, refuses the
/// op rather than performing it — "the existing AskHuman-denied semantics"
/// the plan calls for reusing.
#[tokio::test]
async fn orchestrator_denial_refuses_op() {
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Bypass);
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(perm_worker_risk("agy", tasks.clone(), Risk::BashExec, "cargo publish", "published")),
            Box::new(GrantingOrchestrator {
                id: QuarkId::new("orch"),
                field_path: field.clone(),
                grant_to: QuarkId::new("agy"),
                approved: false,
            }),
        ],
        8,
    )
    .with_no_human(true);
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        has_kind(&events, |k| matches!(k, Kind::PermissionGrant { approved: false, .. })),
        "the denial landed"
    );
    assert!(
        !has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published")),
        "the op was NOT performed"
    );
    assert!(
        has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published refused")),
        "the worker reported the refusal instead"
    );
    assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Ground })), "turn still grounds");
}

/// The auto-scheduler asks the orchestrator AT MOST ONCE per still-pending
/// request: a re-run of `run_until_quiesce` with no answer forthcoming must
/// not inject a second adjudication message. Idempotency is what makes the
/// "fails closed" story true — an orchestrator with no grant tool (or a
/// mock quark that just replies) does not get spun on forever; the ask
/// goes out once and the request stays parked for a human.
#[tokio::test]
async fn orchestrator_is_asked_at_most_once_per_request() {
    use crate::mock::MockQuark;
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Bypass);
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(perm_worker_risk("agy", tasks.clone(), Risk::BashExec, "cargo publish", "published")),
            // A silent orchestrator: replies but never grants — the honest
            // "no grant tool wired up yet" production state.
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "reviewing")),
        ],
        8,
    )
    .with_no_human(true);
    engine.run_until_quiesce().await.unwrap();
    engine.run_until_quiesce().await.unwrap(); // a second pass: must not re-ask

    let events = read_events(&field).unwrap();
    let asks = events
        .iter()
        .filter(|e| {
            e.to.as_ref() == Some(&QuarkId::new("orch"))
                && matches!(&e.kind, Kind::Message { body } if body.starts_with("[no-human-mode]"))
        })
        .count();
    assert_eq!(asks, 1, "the orchestrator is asked exactly once, not re-spun");
    assert!(!has_kind(&events, |k| matches!(k, Kind::PermissionGrant { .. })), "still no grant — worker stays parked");
}

// ---- per-seat `commands` allow/deny fold (config source into `decide()`) ----

/// **SECURITY**: a config `not_allowed` pattern must block a worker's op under
/// No-Human-Mode via `AskHuman`, and NEVER via `AskOrchestrator` — even under
/// global `Bypass`, where a non-deny-listed op would escalate to the
/// orchestrator LLM instead of a human. `decide`'s deny branch already
/// guarantees this (see its SECURITY note); this test proves the config
/// `commands.not_allowed` list actually reaches that branch — the wiring
/// this task adds, not a change to the decision table itself.
#[tokio::test]
async fn config_deny_blocks_under_no_human() {
    use crate::mock::MockQuark;
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Bypass); // global Bypass
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut worker = perm_worker_risk("agy", tasks.clone(), Risk::BashExec, "danger now", "done");
    worker.commands = hadron_lattice::SeatCommands { not_allowed: vec!["danger *".into()], ..Default::default() };
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(worker),
            // Seated so a wrongly-escalated AskOrchestrator would be
            // observable (it would get the `[no-human-mode]` ask below).
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "reviewing")),
        ],
        8,
    )
    .with_no_human(true);
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(has_kind(&events, |k| matches!(k, Kind::PermissionReq { .. })), "req recorded");
    assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "worker parks");
    assert!(!has_kind(&events, |k| matches!(k, Kind::PermissionGrant { .. })), "never auto-granted");
    assert!(
        !events.iter().any(|e| e.to.as_ref() == Some(&QuarkId::new("orch"))
            && matches!(&e.kind, Kind::Message { body } if body.starts_with("[no-human-mode]"))),
        "a config-denied op must go to AskHuman, never AskOrchestrator — even under global Bypass"
    );
}

/// A config `allowed` pattern lets a clamped-`Auto` worker auto-approve its op
/// under No-Human-Mode, exactly as a remembered field-taught allow rule would —
/// proving the config `commands.allowed` list reaches the same `AllowRules`
/// fold `decide`'s `Mode::Auto` branch already consults.
#[tokio::test]
async fn config_allow_auto_approves_under_auto_no_human() {
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    seed_mode(&field, None, Mode::Bypass); // global Bypass; no per-quark override → clamps to Auto
    let tasks = Arc::new(Mutex::new(vec![]));
    let mut worker = perm_worker_risk("agy", tasks.clone(), Risk::BashExec, "safe cmd", "done");
    worker.commands = hadron_lattice::SeatCommands { allowed: vec!["safe *".into()], ..Default::default() };
    let mut engine = Engine::new(field.clone(), vec![Box::new(worker)], 8).with_no_human(true);
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(gluon_auto_granted(&events), "the config allow-list auto-approved the op");
    assert!(has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "done")), "the op completed");
    assert!(!has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "never parked");
}

/// With the No-Human-Mode toggle OFF, `decide` runs today's table byte-for-byte
/// and never consults `deny` — so a config `not_allowed` pattern must be
/// completely inert. Proven by running the SAME op/mode with and without the
/// config deny configured and showing the field ends up in an identical shape
/// either way.
#[tokio::test]
async fn config_rules_inert_when_no_human_off() {
    use hadron_gatekeeper::{Mode, Risk};

    async fn run_with(commands: hadron_lattice::SeatCommands) -> Vec<Event> {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        seed_human_message(&field, "agy", "hello");
        seed_mode(&field, None, Mode::Auto); // global default Auto, no per-quark override
        let tasks = Arc::new(Mutex::new(vec![]));
        let mut worker = perm_worker_risk("agy", tasks.clone(), Risk::BashExec, "danger now", "done");
        worker.commands = commands;
        let mut engine = Engine::new(field.clone(), vec![Box::new(worker)], 8);
        // No `.with_no_human(true)` — toggle stays off.
        engine.run_until_quiesce().await.unwrap();
        read_events(&field).unwrap()
    }

    let deny_events =
        run_with(hadron_lattice::SeatCommands { not_allowed: vec!["danger *".into()], ..Default::default() })
            .await;
    // C1 regression guard: an EXACT config `allowed` matching the op must ALSO be
    // inert toggle-off. `decide`'s toggle-off `Auto+BashExec` arm consults the same
    // `allow` set via `allow.contains((quark, op))`, so before the fold was gated on
    // `no_human` this exact allow flipped the ask into an auto-approve with the
    // toggle OFF — an asymmetric footgun (allow live / deny dead). The op here is
    // "danger now", so `allowed: ["danger now"]` is an exact hit.
    let allow_events =
        run_with(hadron_lattice::SeatCommands { allowed: vec!["danger now".into()], ..Default::default() })
            .await;
    let control_events = run_with(hadron_lattice::SeatCommands::default()).await;

    assert!(!gluon_auto_granted(&deny_events), "Auto+BashExec+no field-allow asks, deny or not");
    assert!(
        !gluon_auto_granted(&allow_events),
        "C1: a config `allowed` is inert with the No-Human-Mode toggle OFF — must NOT auto-approve"
    );
    assert!(!gluon_auto_granted(&control_events), "control: same result with no commands configured");
    assert!(has_kind(&allow_events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })));
    assert!(has_kind(&deny_events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })));
    assert!(has_kind(&control_events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })));
    for (label, evs) in [("allow", &allow_events), ("deny", &deny_events)] {
        assert_eq!(
            evs.iter().filter(|e| matches!(e.kind, Kind::PermissionReq { .. })).count(),
            control_events.iter().filter(|e| matches!(e.kind, Kind::PermissionReq { .. })).count(),
            "identical shape with and without the config {label} — zero effect while the toggle is off"
        );
    }
}

#[tokio::test]
async fn orchestrator_recursion_guard_downgrades_to_ask_human() {
    use hadron_gatekeeper::{Mode, Risk};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "orch", "hello");
    seed_mode(&field, None, Mode::Bypass); // global Bypass
    seed_mode(&field, Some("orch"), Mode::Auto); // per-quark override for orch to Auto
    
    let tasks = Arc::new(Mutex::new(vec![]));
    let orchestrator = PermissionQuark {
        id: QuarkId::new("orch"),
        flavor: Flavor::Orchestrator,
        ask: PermissionAsk { risk: Risk::BashExec, description: "some bash cmd".into() },
        reply: "done".into(),
        calls: 0,
        tasks: tasks.clone(),
        commands: hadron_lattice::SeatCommands::default(),
    };
    
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(orchestrator)],
        8,
    )
    .with_no_human(true);
    
    engine.run_until_quiesce().await.unwrap();
    
    let events = read_events(&field).unwrap();
    assert!(has_kind(&events, |k| matches!(k, Kind::PermissionReq { .. })), "req recorded");
    assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "orchestrator parks in waiting state");
    
    let already_asked = events.iter().any(|e| {
        e.to.as_ref() == Some(&QuarkId::new("orch"))
            && matches!(&e.kind, Kind::Message { body } if body.starts_with("[no-human-mode]"))
    });
    assert!(!already_asked, "orchestrator must not be auto-scheduled to adjudicate its own request");
}

#[tokio::test]
async fn orchestrator_slash_commands_in_finish_turn() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    
    struct CommandOrchestrator {
        id: QuarkId,
        messages: Vec<String>,
        calls: usize,
    }
    
    #[async_trait::async_trait]
    impl crate::quark::Quark for CommandOrchestrator {
        fn id(&self) -> QuarkId {
            self.id.clone()
        }
        fn flavor(&self) -> Flavor {
            Flavor::Orchestrator
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            let msg = if self.calls < self.messages.len() {
                Some(self.messages[self.calls].clone())
            } else {
                None
            };
            self.calls += 1;
            Ok(TurnOutcome {
                message: msg,
                permission: None,
                ..Default::default()
            })
        }
    }
    
    seed_human_message(&field, "orch", "hello");
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(CommandOrchestrator {
                id: QuarkId::new("orch"),
                messages: vec!["/approve @worker".to_string()],
                calls: 0,
            }),
            Box::new(crate::mock::MockQuark::repeating(QuarkId::new("worker"), Flavor::Worker, "reply")),
        ],
        8,
    );
    engine.run_until_quiesce().await.unwrap();
    
    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| {
            e.from == Actor::Quark(QuarkId::new("orch"))
                && e.to.as_ref() == Some(&QuarkId::new("worker"))
                && matches!(e.kind, Kind::PermissionGrant { approved: true, remember: false })
        }),
        "should parse /approve @worker and append permission grant"
    );
    
    let dir2 = tempdir().unwrap();
    let field2 = dir2.path().join("field.jsonl");
    seed_human_message(&field2, "orch", "hello");
    let mut engine2 = Engine::new(
        field2.clone(),
        vec![
            Box::new(CommandOrchestrator {
                id: QuarkId::new("orch"),
                messages: vec!["/deny @worker remember".to_string()],
                calls: 0,
            }),
            Box::new(crate::mock::MockQuark::repeating(QuarkId::new("worker"), Flavor::Worker, "reply")),
        ],
        8,
    );
    engine2.run_until_quiesce().await.unwrap();
    
    let events2 = read_events(&field2).unwrap();
    assert!(
        events2.iter().any(|e| {
            e.from == Actor::Quark(QuarkId::new("orch"))
                && e.to.as_ref() == Some(&QuarkId::new("worker"))
                && matches!(e.kind, Kind::PermissionGrant { approved: false, remember: true })
        }),
        "should parse /deny @worker remember and append permission grant with remember: true"
    );
}

fn seed_human_message(path: &std::path::Path, to: &str, body: &str) {
    append_event(
        path,
        &Event::new(
            Actor::Human,
            Some(QuarkId::new(to)),
            Kind::Message { body: body.into() },
        ),
    )
    .unwrap();
}

/// An UNADDRESSED (`to: None`) human message naming `to` by `@mention`, routed
/// through `unaddressed_message_targets`'s precise `.answers`-stamp check
/// (`has_answered`) rather than `next_pending`'s older positional "answered =
/// spoke after" reading that an ADDRESSED message (`seed_human_message`) still
/// goes through. Needed whenever a test seeds a second message while its
/// target is already mid-turn: an addressed message would be silently
/// swallowed the moment the first turn's own reply lands after it in the
/// field — a real, pre-existing gap unrelated to whatever the test is
/// actually exercising.
fn seed_unaddressed_mention(path: &std::path::Path, to: &str, body: &str) {
    append_event(
        path,
        &Event::new(Actor::Human, None, Kind::Message { body: format!("@{to} {body}") }),
    )
    .unwrap();
}

/// A temp git repo with one commit so HEAD exists (for git-safety tests).
fn git_init_repo() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap();
    };
    // `-b main` pinned: otherwise the host's `init.defaultBranch` decides
    // whether the base branch is `main` or `master`, and every worktree test
    // that talks about a base branch becomes host-dependent.
    run(&["init", "-q", "-b", "main"]);
    std::fs::write(root.join("f.txt"), "x\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);
    dir
}

#[tokio::test]
async fn engine_snapshots_before_excite_when_git_enabled() {
    let fdir = tempdir().unwrap();
    let path = fdir.path().join("field.jsonl");
    seed_human_message(&path, "orch", "do it");

    let repo = git_init_repo();
    let orch = MockQuark::scripted(
        QuarkId::new("orch"),
        Flavor::Orchestrator,
        vec![Some("done, back to human".into())],
    );
    let mut engine = Engine::new(path.clone(), vec![Box::new(orch)], 10)
        .with_git(repo.path().to_path_buf());
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&path).unwrap();
    let snapshots = events
        .iter()
        .filter(|e| matches!(e.kind, Kind::Snapshot { .. }))
        .count();
    assert_eq!(snapshots, 1, "one snapshot recorded before the single excite");
}

#[tokio::test]
async fn projection_carries_nucleus_digest() {
    let fdir = tempdir().unwrap();
    let path = fdir.path().join("field.jsonl");
    seed_human_message(&path, "orch", "go");

    // A probe quark asserts on the projection it receives.
    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("orch")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Orchestrator
        }
        fn energy(&self) -> hadron_lattice::EnergyState {
            hadron_lattice::EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            assert!(turn.nucleus_digest.contains("## map.md"));
            Ok(TurnOutcome { message: Some("done".into()), permission: None, ..Default::default()})
        }
    }

    let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10)
        .with_nucleus("## map.md\nthe project map".into());
    engine.run_until_quiesce().await.unwrap();
}

#[tokio::test]
async fn nucleus_digest_renders_from_a_real_features_file() {
    // Pins the REAL composition the daemon bin uses — `build_nucleus_digest`
    // feeding `with_nucleus` — against a real `.hadron/nucleus/features.md`
    // on disk. Discharges `nucleus-load-digest-is-unwired`: proves the
    // digest a quark actually sees comes from a file, not a hand-written
    // literal.
    let fdir = tempdir().unwrap();
    let path = fdir.path().join("field.jsonl");
    seed_human_message(&path, "orch", "go");

    let nucleus_dir = fdir.path().join(".hadron").join("nucleus");
    std::fs::create_dir_all(&nucleus_dir).unwrap();
    std::fs::write(nucleus_dir.join("features.md"), "## Login\nstatus: done\n").unwrap();

    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("orch")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Orchestrator
        }
        fn energy(&self) -> hadron_lattice::EnergyState {
            hadron_lattice::EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            assert!(turn.nucleus_digest.contains("Login"), "got: {:?}", turn.nucleus_digest);
            Ok(TurnOutcome { message: Some("done".into()), permission: None, ..Default::default()})
        }
    }

    let digest = super::build_nucleus_digest(fdir.path());
    let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10).with_nucleus(digest);
    engine.run_until_quiesce().await.unwrap();
}

#[tokio::test]
async fn orchestrated_handoff_runs_then_quiesces() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    seed_human_message(&path, "orch", "Build the thing. @worker will help.");

    // Handoffs begin a line (the line-start delegation convention): a mention
    // buried mid-sentence no longer routes, so the @mention is line-leading.
    let orch = MockQuark::scripted(
        QuarkId::new("orch"),
        Flavor::Orchestrator,
        vec![
            Some("Starting the build.\n@worker please build the UI.".into()),
            Some("All done. Handing back to the human.".into()),
        ],
    );
    let worker = MockQuark::scripted(
        QuarkId::new("worker"),
        Flavor::Worker,
        vec![Some("UI complete.\n@orch back to you.".into())],
    );

    let mut engine = Engine::new(
        path.clone(),
        vec![Box::new(orch), Box::new(worker)],
        10,
    );
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&path).unwrap();
    let messages: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            Kind::Message { body } => Some(body.as_str()),
            _ => None,
        })
        .collect();
    // human, orch->worker, worker->orch, orch->human (handback)
    assert_eq!(messages.len(), 4);
    assert!(messages[1].contains("@worker"));
    assert!(messages[2].contains("@orch"));
    assert!(messages[3].contains("Handing back"));
    // Quiesced cleanly: no backstop message.
    assert!(!messages.iter().any(|m| m.contains("backstop")));
}

#[tokio::test]
async fn unaddressed_human_message_routes_to_the_orchestrator() {
    use hadron_lattice::{Projection, TurnOutcome};
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    // The human just types — no @mention (to: None).
    append_event(
        &path,
        &Event::new(Actor::Human, None, Kind::Message { body: "hello, anyone home?".into() }),
    )
    .unwrap();

    // A probe orchestrator records the task it was handed; the worker must not run.
    struct OrchProbe {
        seen: Arc<Mutex<Vec<String>>>,
    }
    #[async_trait::async_trait]
    impl crate::quark::Quark for OrchProbe {
        fn id(&self) -> QuarkId {
            QuarkId::new("orch")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Orchestrator
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            self.seen.lock().unwrap().push(turn.task.clone());
            Ok(TurnOutcome { message: Some("I've got it.".into()), permission: None, ..Default::default()})
        }
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        path.clone(),
        vec![
            Box::new(OrchProbe { seen: seen.clone() }),
            Box::new(MockQuark::scripted(QuarkId::new("worker"), Flavor::Worker, vec![Some("nope".into())])),
        ],
        10,
    );
    engine.run_until_quiesce().await.unwrap();

    // The orchestrator was handed the exact unaddressed message as its task…
    assert_eq!(seen.lock().unwrap().as_slice(), &["hello, anyone home?".to_string()]);
    // …and the worker never ran (an unaddressed message is the orchestrator's).
    let events = read_events(&path).unwrap();
    assert!(
        !events.iter().any(|e| e.from == Actor::Quark(QuarkId::new("worker"))),
        "worker must not run for an unaddressed message"
    );
    // The orchestrator's reply (no @mention) hands control back → quiesce.
    assert!(next_pending(&events).is_none());
}

/// **THE HUMAN TYPED WHILE THE ORCHESTRATOR WAS WORKING.** Jake asked whether his
/// messages stack if he speaks while a quark is mid-turn. They must — a chat where
/// the second thing you say is thrown away is not a chat.
///
/// The trap this pins: "has the quark answered the human?" used to mean *"has it
/// authored anything since?"*. The quark finishes the turn it was already on, its
/// reply lands **after** the newer message, and the newer message is marked answered
/// by a reply that could not possibly have seen it. The human's second message is
/// then dropped, silently, forever.
///
/// The probe types the second message *from inside the first turn*, which is exactly
/// the race, made deterministic.
#[tokio::test]
async fn a_message_sent_while_the_quark_is_working_is_not_lost() {
    use hadron_lattice::{Projection, TurnOutcome};
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(Actor::Human, None, Kind::Message { body: "first".into() }),
    )
    .unwrap();

    /// Answers "first", and while it is doing so the human types "second".
    struct Interrupted {
        field: std::path::PathBuf,
        seen: Arc<Mutex<Vec<String>>>,
    }
    #[async_trait::async_trait]
    impl crate::quark::Quark for Interrupted {
        fn id(&self) -> QuarkId {
            QuarkId::new("orch")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Orchestrator
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            self.seen.lock().unwrap().push(turn.task.clone());
            // THE RACE: the human speaks again, mid-turn. This turn cannot see it —
            // its projection was already built — and it is about to reply anyway.
            if turn.task == "first" {
                append_event(
                    &self.field,
                    &Event::new(
                        Actor::Human,
                        None,
                        Kind::Message { body: "second".into() },
                    ),
                )
                .unwrap();
            }
            Ok(TurnOutcome {
                message: Some(format!("done with {}", turn.task)),
                permission: None,
                ..Default::default()
            })
        }
    }

    let seen = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        path.clone(),
        vec![Box::new(Interrupted { field: path.clone(), seen: seen.clone() })],
        10,
    );
    engine.run_until_quiesce().await.unwrap();

    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec!["first".to_string(), "second".to_string()],
        "the human spoke twice and must be answered twice — a message sent while the \
         quark was working is QUEUED, not swallowed by the reply to the previous one"
    );
}

#[tokio::test]
async fn unaddressed_message_with_no_orchestrator_quiesces() {
    // No orchestrator on the roster → an unaddressed message routes to no one.
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(Actor::Human, None, Kind::Message { body: "hi".into() }),
    )
    .unwrap();
    let mut engine = Engine::new(
        path.clone(),
        vec![Box::new(MockQuark::scripted(QuarkId::new("worker"), Flavor::Worker, vec![Some("x".into())]))],
        10,
    );
    engine.run_until_quiesce().await.unwrap();
    let events = read_events(&path).unwrap();
    assert!(!events.iter().any(|e| matches!(e.from, Actor::Quark(_))), "no quark runs without an orchestrator");
}

#[tokio::test]
async fn runaway_pingpong_trips_backstop() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    seed_human_message(&path, "orch", "start");

    // Both quarks address each other forever.
    let orch = MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "@worker go");
    let worker = MockQuark::repeating(QuarkId::new("worker"), Flavor::Worker, "@orch go");

    let mut engine = Engine::new(
        path.clone(),
        vec![Box::new(orch), Box::new(worker)],
        4,
    );
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&path).unwrap();
    let backstops = events
        .iter()
        .filter(|e| matches!(&e.kind, Kind::Message { body } if body.contains("backstop")))
        .count();
    assert_eq!(backstops, 1, "exactly one backstop message should be appended");
    // The loop bounded the number of quark turns.
    let ground_statuses = events
        .iter()
        .filter(|e| matches!(e.kind, Kind::Status { state: QuarkState::Ground }))
        .count();
    assert_eq!(ground_statuses, 4, "exactly max_exchanges turns ran");
}

#[tokio::test]
async fn bypass_mode_skips_backstop() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    seed_human_message(&path, "orch", "start");
    seed_mode(&path, None, Mode::Bypass);

    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    struct CounterQuark {
        id: QuarkId,
        flavor: Flavor,
        reply: String,
        count: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl Quark for CounterQuark {
        fn id(&self) -> QuarkId { self.id.clone() }
        fn flavor(&self) -> Flavor { self.flavor.clone() }
        fn energy(&self) -> EnergyState { EnergyState::Available }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            let current = self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let message = if current < 6 {
                Some(self.reply.clone())
            } else {
                None
            };
            Ok(TurnOutcome { message, permission: None, ..Default::default()})
        }
    }

    let orch = CounterQuark {
        id: QuarkId::new("orch"),
        flavor: Flavor::Orchestrator,
        reply: "@worker go".to_string(),
        count: count.clone(),
    };
    let worker = CounterQuark {
        id: QuarkId::new("worker"),
        flavor: Flavor::Worker,
        reply: "@orch go".to_string(),
        count: count.clone(),
    };

    let mut engine = Engine::new(
        path.clone(),
        vec![Box::new(orch), Box::new(worker)],
        4, // max_exchanges is 4, but we should exceed it
    );
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&path).unwrap();
    let backstops = events
        .iter()
        .filter(|e| matches!(&e.kind, Kind::Message { body } if body.contains("backstop")))
        .count();
    assert_eq!(backstops, 0, "no backstop message should be appended in Bypass mode");

    let ground_statuses = events
        .iter()
        .filter(|e| matches!(e.kind, Kind::Status { state: QuarkState::Ground }))
        .count();
    assert!(ground_statuses > 4, "should run more than 4 turns: {}", ground_statuses);
}

#[tokio::test]
async fn engine_blocks_depleted_quarks_and_records_usage() {
    use crate::ledger::Ledger;
    let fdir = tempdir().unwrap();
    let path = fdir.path().join("field.jsonl");

    struct HeavyQuark;
    #[async_trait::async_trait]
    impl Quark for HeavyQuark {
        fn id(&self) -> QuarkId { QuarkId::new("worker") }
        fn flavor(&self) -> Flavor { Flavor::Worker }
        fn energy(&self) -> hadron_lattice::EnergyState { hadron_lattice::EnergyState::Available }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<hadron_lattice::TurnOutcome> {
            // Consume 100 tokens per turn
            Ok(hadron_lattice::TurnOutcome {
                message: None,
                permission: None,
                usage: hadron_lattice::Usage {
                    spend: hadron_lattice::TokenSpend {
                        input: Some(60),
                        output: Some(40),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            })
        }
    }

    let ledger = Ledger::open_in_memory().unwrap();
    let mut engine = Engine::new(path.clone(), vec![Box::new(HeavyQuark)], 5)
        .with_ledger(ledger, Some(150));

    // Turn 1: 0 used. Executes, uses 100. Total: 100.
    seed_human_message(&path, "worker", "do heavy work 1");
    engine.run_until_quiesce().await.unwrap();

    // Turn 2: 100 used (<= 150 limit). Executes, uses 100. Total: 200.
    seed_human_message(&path, "worker", "do heavy work 2");
    engine.run_until_quiesce().await.unwrap();

    // Turn 3: 200 used (> 150 limit). Blocked!
    seed_human_message(&path, "worker", "do heavy work 3");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&path).unwrap();
    
    let reports = events.iter().filter(|e| matches!(e.kind, Kind::EnergyReport { .. })).count();
    assert_eq!(reports, 2, "Quark should execute 2 times before depleting");
    
    let blocks = events.iter().filter(|e| matches!(e.kind, Kind::Status { state: QuarkState::Blocked })).count();
    assert_eq!(blocks, 1, "Quark should be blocked on the 3rd attempt");
}

/// A quark that burns 100 tokens a turn and optionally carries its own limit.
struct SpenderQuark(Option<u32>);

#[async_trait::async_trait]
impl Quark for SpenderQuark {
    fn id(&self) -> QuarkId { QuarkId::new("worker") }
    fn flavor(&self) -> Flavor { Flavor::Worker }
    fn energy(&self) -> hadron_lattice::EnergyState { hadron_lattice::EnergyState::Available }
    fn energy_limit(&self) -> Option<u32> { self.0 }
    async fn excite(&mut self, _turn: Projection) -> anyhow::Result<hadron_lattice::TurnOutcome> {
        Ok(hadron_lattice::TurnOutcome {
            message: None,
            permission: None,
            usage: hadron_lattice::Usage {
                spend: hadron_lattice::TokenSpend { input: Some(60), output: Some(40), ..Default::default() },
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

/// Runs three 100-token turns and reports `(energy_reports, blocks)`.
async fn three_spending_turns(seat_limit: Option<u32>, default_limit: Option<u32>) -> (usize, usize) {
    use crate::ledger::Ledger;
    let fdir = tempdir().unwrap();
    let path = fdir.path().join("field.jsonl");
    let ledger = Ledger::open_in_memory().unwrap();
    let mut engine = Engine::new(path.clone(), vec![Box::new(SpenderQuark(seat_limit))], 5)
        .with_ledger(ledger, default_limit);
    for n in 1..=3 {
        seed_human_message(&path, "worker", &format!("do heavy work {n}"));
        engine.run_until_quiesce().await.unwrap();
    }
    let events = read_events(&path).unwrap();
    (
        events.iter().filter(|e| matches!(e.kind, Kind::EnergyReport { .. })).count(),
        events.iter().filter(|e| matches!(e.kind, Kind::Status { state: QuarkState::Blocked })).count(),
    )
}

/// The depletion gate is **off** unless somebody asked for it.
///
/// It used to be armed unconditionally: the daemon handed every seat a
/// hard-coded 500k ceiling, so a quark could be cut off mid-plan by a number
/// nobody chose. The ledger still records every token — spend is telemetry, and
/// telemetry is not a gate.
#[tokio::test]
async fn a_ledger_with_no_limit_records_usage_but_never_blocks() {
    let (reports, blocks) = three_spending_turns(None, None).await;
    assert_eq!(reports, 3, "every turn should run and be metered");
    assert_eq!(blocks, 0, "no limit was set, so nothing may be blocked");
}

/// …and it is still a real gate for the seat the human armed.
#[tokio::test]
async fn a_per_seat_limit_gates_even_with_no_swarm_wide_default() {
    let (reports, blocks) = three_spending_turns(Some(150), None).await;
    assert_eq!(reports, 2, "two turns fit under the seat's own 150-token limit");
    assert_eq!(blocks, 1, "the third is refused");
}

/// The reason the Standard Model is `include_str!`d rather than read from disk.
///
/// Rules that stop a quark confabulating are worthless if a fresh clone, a
/// `.gitignore`, or a deleted directory can silently remove them — the swarm
/// would just quietly get worse, with nothing to notice. Point the engine at a
/// workspace with NO `.hadron` at all: the invariants must still arrive.
#[test]
fn the_standard_model_survives_a_workspace_with_no_files_at_all() {
    let empty = tempdir().unwrap();
    let (text, available) = build_invariants(empty.path(), &[]);

    assert!(text.contains("# The Standard Model"));
    assert!(text.contains("Prove it runs"), "rule 1 — the one both quarks broke");
    assert!(text.contains("Make invalid states unrepresentable"), "rule 8 — agy's");
    assert!(available.is_empty(), "no repo tier exists here, and that is fine");
}

/// An over-budget index is handed over WHOLE — this used to be the test that proved
/// the opposite (a head-slice keeping the newest lessons), and Task 4 deliberately
/// removed that truncation: the prompt builder now emits a tag manifest plus the
/// task-relevant lines instead, so nothing is silently dropped on the way in.
/// What the old test was really protecting — "the lesson a quark just paid for must
/// survive" — is what is asserted here.
#[test]
fn an_over_budget_index_is_returned_whole_so_the_newest_lesson_survives() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.md");

    let mut raw = String::from("# Nucleus index\n\nFormat: `- **<slug>** — <lesson>`\n\n");
    for i in 0..400 {
        raw.push_str(&format!("- **lesson-{i}** — {}\n", "x".repeat(200)));
    }
    raw.push_str("- **the-newest-lesson** — the one just paid for\n");
    assert!(raw.len() > NUCLEUS_INDEX_BUDGET, "the fixture must overflow");
    fs::write(&path, &raw).unwrap();

    let (out, truncated) = read_nucleus_index(&path);
    assert!(!truncated, "Task 4 removed the read-side cut");
    assert_eq!(out, raw, "an over-budget index must not be sliced on the way in");
    assert!(out.contains("the-newest-lesson"));
    assert!(out.contains("**lesson-0**"), "the oldest lesson is no longer dropped either");
}

/// An index that fits is handed over whole, and is not reported as cut.
#[test]
fn an_index_within_budget_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.md");
    let raw = "# Nucleus index\n\n- **a** — one\n- **b** — two\n";
    fs::write(&path, raw).unwrap();

    let (out, truncated) = read_nucleus_index(&path);
    assert_eq!(out, raw);
    assert!(!truncated);
}

/// The prompt tests prove `prompt.rs` *renders* the nucleus index. They prove
/// nothing about whether the engine ever *reads* it — which is the exact gap
/// ("correct" vs "runs") that cost us a whole session. This is the caller test:
/// put a real file on disk at the real path, drive a real turn, and assert the
/// quark received it.
///
/// The index is SHARED: the file is `index.md`, not `worker.md`. A lesson one quark
/// paid for has to reach the others, or the swarm learns nothing as a swarm.
#[tokio::test]
async fn the_shared_nucleus_index_actually_reaches_a_quarks_projection() {
    use std::fs;
    let ws = tempdir().unwrap();
    let nucleus_dir = ws.path().join(".hadron").join("nucleus");
    fs::create_dir_all(&nucleus_dir).unwrap();
    fs::write(nucleus_dir.join("index.md"), "The forge crate is unwired.").unwrap();

    let path = ws.path().join(".hadron").join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Human,
            Some(QuarkId::new("worker")),
            Kind::Message { body: "go".into() },
        ),
    )
    .unwrap();

    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("worker")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> hadron_lattice::EnergyState {
            hadron_lattice::EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            assert_eq!(
                turn.nucleus_index.trim(),
                "The forge crate is unwired.",
                "the engine must load the shared nucleus index from disk"
            );
            assert!(
                turn.nucleus_index_path.ends_with("nucleus/index.md"),
                "one index for the whole swarm, not one file per quark, got {:?}",
                turn.nucleus_index_path
            );
            assert!(
                turn.nucleus_notes_dir.ends_with("nucleus/notes"),
                "and it must know where the long-form notes live, got {:?}",
                turn.nucleus_notes_dir
            );
            assert!(!turn.nucleus_index_truncated, "this index is two lines long");
            Ok(TurnOutcome {
                message: Some("done".into()),
                permission: None,
                ..Default::default()
            })
        }
    }

    let mut engine = Engine::new(path, vec![Box::new(Probe)], 10);
    engine.run_until_quiesce().await.unwrap();
}

/// The index is in every prompt of every turn, so an unbounded one is a bill that
/// grows forever. Cap it — but never silently: a lesson dropped for size that nobody
/// is told about is indistinguishable from a lesson never learned.
#[test]
fn tag_manifest_summarises_by_heading_not_by_dropping_lines() {
    let index = "## GUI\n- **a** — one [tag:gui]\n- **b** — two [tag:gui]\n\
                 ## IPC\n- **c** — three [tag:ipc]\n";
    let manifest = super::nucleus::tag_manifest(index);
    assert!(manifest.contains("GUI") && manifest.contains("2"));
    assert!(manifest.contains("IPC") && manifest.contains("1"));
    assert!(manifest.len() < index.len(), "the manifest must be smaller than the full index");
}

/// The manifest is what a quark sees INSTEAD of the index once the index is over
/// budget, so it is the one number nobody can sanity-check against the source. It
/// must therefore count BOTH index shapes: the pointer form the chamber writes now
/// and the bold form every pre-migration line on disk still uses.
#[test]
fn tag_manifest_counts_pointer_lines_as_well_as_the_old_bold_ones() {
    let index = "## GUI\n- [a](notes/a.md) — one\n- **b** — two\n\
                 ## IPC\n- [c](notes/c.md) — three\n";
    let manifest = super::nucleus::tag_manifest(index);
    assert!(manifest.contains("GUI") && manifest.contains("2"), "{manifest}");
    assert!(manifest.contains("IPC") && manifest.contains("1"), "{manifest}");
}

#[test]
fn an_oversized_index_is_no_longer_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let path = nucleus_index_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let big = "- **x** — ".to_string() + &"a".repeat(NUCLEUS_INDEX_BUDGET + 1000);
    std::fs::write(&path, &big).unwrap();
    let (text, truncated) = read_nucleus_index(&path);
    assert!(!truncated, "Task 4 removes truncation entirely");
    assert_eq!(text.len(), big.len());
}

#[test]
fn an_oversized_nucleus_index_is_cut_and_says_so() {
    use std::fs;
    let ws = tempdir().unwrap();
    let path = nucleus_index_path(ws.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    let fat = "é".repeat(NUCLEUS_INDEX_BUDGET);
    fs::write(&path, &fat).unwrap();

    let (text, truncated) = read_nucleus_index(&path);
    assert!(!truncated, "truncation removed in Task 4");
    assert_eq!(text, fat);

    // A small index is passed through whole and NOT flagged.
    fs::write(&path, "- **a** — a lesson.").unwrap();
    let (text, truncated) = read_nucleus_index(&path);
    assert_eq!(text, "- **a** — a lesson.");
    assert!(!truncated);

    // A missing index is the first-run case, not an error.
    let empty = tempdir().unwrap();
    assert_eq!(read_nucleus_index(&nucleus_index_path(empty.path())), (String::new(), false));
}

/// Nucleus is the single knowledge root now — lessons live there, not in the
/// old `.hadron/memory/`.
#[test]
fn nucleus_index_paths_live_under_nucleus_dir() {
    let root = std::path::Path::new("/repo");
    assert_eq!(nucleus_index_path(root), std::path::PathBuf::from("/repo/.hadron/nucleus/index.md"));
    assert_eq!(nucleus_notes_dir(root), std::path::PathBuf::from("/repo/.hadron/nucleus/notes"));
}

/// Until a project's legacy `.hadron/memory/` has been migrated (daemon
/// boot, `Engine::migrate_legacy_memory`), a quark must still see its real
/// lessons — never an empty index just because the move hasn't happened yet.
#[test]
fn fallback_reads_legacy_memory_when_nucleus_is_empty() {
    let dir = tempdir().unwrap();
    let legacy = dir.path().join(".hadron").join("memory");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("index.md"), "- **old-lesson** — from before the move\n").unwrap();

    let (text, truncated) = read_nucleus_index_with_fallback(dir.path());
    assert!(text.contains("old-lesson"));
    assert!(!truncated);
}

/// Once nucleus has real content, the legacy file is ignored — no split-brain
/// between the two locations.
#[test]
fn fallback_prefers_nucleus_once_it_has_content() {
    let dir = tempdir().unwrap();
    let nucleus = dir.path().join(".hadron").join("nucleus");
    std::fs::create_dir_all(&nucleus).unwrap();
    std::fs::write(nucleus.join("index.md"), "- **new-lesson** — after the move\n").unwrap();
    let legacy = dir.path().join(".hadron").join("memory");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("index.md"), "- **old-lesson** — should not appear\n").unwrap();

    let (text, _) = read_nucleus_index_with_fallback(dir.path());
    assert!(text.contains("new-lesson"));
    assert!(!text.contains("old-lesson"));
}

/// Tiers are labelled. A quark that cannot tell a rule Hadron *ships* from a rule
/// *this project* added cannot reason about which to question when they conflict.
#[test]
fn repo_rules_are_labelled_as_the_projects_own() {
    use std::fs;
    let ws = tempdir().unwrap();
    let dir = ws.path().join(".hadron").join("nucleus").join("invariants");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("always.md"), "Threat-model every endpoint.").unwrap();

    let (text, _) = build_invariants(ws.path(), &[]);
    assert!(text.contains("# The Standard Model"), "tier 1 still first");
    assert!(text.contains("# Project rule: always"), "tier 3 is named as the project's");
    assert!(text.contains("Threat-model every endpoint."));
}

/// `/learn-std-model` writes `laws.md`, not the Standard Model itself — this is the
/// proof it actually reaches the prompt, right after tier 1 and ahead of the named
/// `invariants/` tier.
#[test]
fn repo_laws_are_appended_right_after_the_standard_model() {
    use std::fs;
    let ws = tempdir().unwrap();
    let nucleus = ws.path().join(".hadron").join("nucleus");
    fs::create_dir_all(&nucleus).unwrap();
    fs::write(nucleus.join("laws.md"), "- Never merge on a red gate.\n").unwrap();

    let (text, _) = build_invariants(ws.path(), &[]);
    assert!(text.contains("# This project's standard laws"));
    assert!(text.contains("Never merge on a red gate."));

    let laws_pos = text.find("standard laws").unwrap();
    let model_pos = text.find("# The Standard Model").unwrap();
    assert!(model_pos < laws_pos, "the Standard Model must still come first");
}

/// A missing `laws.md` is the normal case (nobody has pinned a law yet) and must
/// not appear as an empty, confusing section header.
#[test]
fn no_laws_file_means_no_laws_section() {
    let ws = tempdir().unwrap();
    let (text, _) = build_invariants(ws.path(), &[]);
    assert!(!text.contains("standard laws"));
}

/// The whole point, driven end to end: a task that says "execute the plan" must
/// arrive at the quark's CLI as the executing-plans procedure — and because the
/// plan on disk records THIS quark as its author, the same prompt must refuse to
/// let it grade its own homework and name a peer who can.
///
/// Asserted against `prompt::build`, not just the projection: a field the prompt
/// never renders is a rule the model never sees (`available_invariants` is exactly
/// that today — set on every projection, printed nowhere).
#[tokio::test]
async fn a_quark_handed_its_own_plan_is_told_to_hand_verification_to_a_peer() {
    use std::fs;
    let fdir = tempdir().unwrap();

    // Anchor the workspace root, so `docs/plans/...` in the task resolves.
    fs::create_dir_all(fdir.path().join(".hadron")).unwrap();
    let plans = fdir.path().join("docs").join("plans");
    fs::create_dir_all(&plans).unwrap();
    fs::write(
        plans.join("2026-07-14-acp-auth.md"),
        "---\nauthor: worker\nstatus: draft\n---\n\n# ACP auth — implementation plan\n",
    )
    .unwrap();

    let path = fdir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Human,
            Some(QuarkId::new("worker")),
            Kind::Message {
                body: "@worker execute the plan at docs/plans/2026-07-14-acp-auth.md".into(),
            },
        ),
    )
    .unwrap();

    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("worker")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            // The rendered prompt is what the model actually reads.
            let prompt = crate::adapter::prompt::build(&turn, &QuarkId::new("worker"));

            assert!(
                prompt.contains("# Skill for this turn: executing-plans"),
                "the engine must select the skill from the task text:\n{prompt}"
            );
            assert!(
                prompt.contains("Load plan, review critically"),
                "the skill BODY must be injected, not just its name"
            );
            // The Standard Model is still there — a skill augments the protocol,
            // it does not replace it.
            assert!(prompt.contains("Prove it runs"));

            // Ground truth from disk: this quark wrote the plan it was handed.
            assert!(
                prompt.contains("you wrote this plan"),
                "must refuse self-verification:\n{prompt}"
            );
            // …and the peer it may hand to is named, because a disabled or absent
            // seat would be a handoff into the void.
            assert!(prompt.contains("`@reviewer`"), "must name the available peer");

            Ok(TurnOutcome {
                message: Some("done".into()),
                permission: None,
                ..Default::default()
            })
        }
    }

    struct Peer;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Peer {
        fn id(&self) -> QuarkId {
            QuarkId::new("reviewer")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            Ok(TurnOutcome {
                message: Some("idle".into()),
                permission: None,
                ..Default::default()
            })
        }
    }

    let mut engine =
        Engine::new(path.clone(), vec![Box::new(Probe), Box::new(Peer)], 10);
    engine.run_until_quiesce().await.unwrap();
}

/// A turn that is not plan work must be byte-for-byte what it was before skills
/// existed. A router that fires on everything is a tax on every turn.
///
/// `Engine::new` (used below) defaults `global_skills_dir` to `None`, so this
/// negative assertion cannot be defeated by whatever custom skills happen to sit
/// under the real `~/.hadron/skills` on the machine running the test — the engine
/// simply never looks there unless `with_global_skills_dir` says to.
#[tokio::test]
async fn an_ordinary_task_gets_no_skill_and_no_extra_prompt() {
    use std::fs;
    let fdir = tempdir().unwrap();
    fs::create_dir_all(fdir.path().join(".hadron")).unwrap();
    let path = fdir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Human,
            Some(QuarkId::new("worker")),
            Kind::Message { body: "@worker fix the clipped completion popup".into() },
        ),
    )
    .unwrap();

    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("worker")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            assert!(
                !turn.invariants.contains("Skill for this turn"),
                "no trigger, no skill — the no-match path must be a true no-op"
            );
            assert!(turn.invariants.contains("Prove it runs"), "protocol still arrives");
            Ok(TurnOutcome {
                message: Some("done".into()),
                permission: None,
                ..Default::default()
            })
        }
    }

    let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10);
    engine.run_until_quiesce().await.unwrap();
}

/// A repo `.hadron/skills/*.md` file must actually reach a quark's turn — not
/// just parse (`skills.rs` Task 1 proved that in isolation). This is the
/// end-to-end proof: a custom skill dropped in the workspace's own
/// `.hadron/skills/` is loaded by the ENGINE and its body shows up in the
/// projection, same as a built-in's would.
///
/// `Engine::new` leaves `global_skills_dir` at its `None` default, so this test
/// never touches the real `~/.hadron/skills` either — only the repo (tempdir)
/// skill is in play. See `engine_uses_injected_global_skills_dir` for the global
/// half, via `with_global_skills_dir`.
#[tokio::test]
async fn engine_loads_repo_skills() {
    use std::fs;
    let fdir = tempdir().unwrap();
    let skills_dir = fdir.path().join(".hadron").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        skills_dir.join("custom.md"),
        "---\nname: custom-thing\ndescription: does a custom thing\ntriggers: [frobnicate]\n---\n\nTHE DISTINCTIVE CUSTOM-SKILL BODY LINE.\n",
    )
    .unwrap();

    let path = fdir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Human,
            Some(QuarkId::new("worker")),
            Kind::Message { body: "@worker please frobnicate the widget".into() },
        ),
    )
    .unwrap();

    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("worker")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            assert!(
                turn.active_skill.as_deref().unwrap_or_default().contains("THE DISTINCTIVE CUSTOM-SKILL BODY LINE."),
                "the repo custom skill's body must be injected:\n{:?}",
                turn.active_skill
            );
            assert!(
                turn.invariants.contains("custom-thing"),
                "the custom skill must also be named in the index/header:\n{}",
                turn.invariants
            );
            Ok(TurnOutcome { message: Some("done".into()), permission: None, ..Default::default()})
        }
    }

    let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10);
    engine.run_until_quiesce().await.unwrap();
}

/// The global half of skill loading, proven the same way the repo half was
/// above: a skill placed under an INJECTED directory (never the real
/// `~/.hadron/skills` — see `with_global_skills_dir`) must reach the projection.
/// Positive assertion only, tempdir-controlled throughout — this is the seam the
/// daemon bin wires with the real path; here it's wired with a fake one, which is
/// exactly the point of making it injectable.
#[tokio::test]
async fn engine_uses_injected_global_skills_dir() {
    use std::fs;
    let fdir = tempdir().unwrap();
    let global_dir = tempdir().unwrap();
    let global_skills_dir = global_dir.path().join("skills");
    fs::create_dir_all(&global_skills_dir).unwrap();
    fs::write(
        global_skills_dir.join("global-custom.md"),
        "---\nname: global-thing\ndescription: a global custom skill\ntriggers: [zorbnicate]\n---\n\nTHE DISTINCTIVE GLOBAL-SKILL BODY LINE.\n",
    )
    .unwrap();

    let path = fdir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Human,
            Some(QuarkId::new("worker")),
            Kind::Message { body: "@worker please zorbnicate the widget".into() },
        ),
    )
    .unwrap();

    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("worker")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            assert!(
                turn.active_skill.as_deref().unwrap_or_default().contains("THE DISTINCTIVE GLOBAL-SKILL BODY LINE."),
                "the injected global skill's body must be injected:\n{:?}",
                turn.active_skill
            );
            assert!(
                turn.invariants.contains("global-thing"),
                "the global skill must also be named in the index/header:\n{}",
                turn.invariants
            );
            Ok(TurnOutcome { message: Some("done".into()), permission: None, ..Default::default()})
        }
    }

    let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10)
        .with_global_skills_dir(Some(global_skills_dir));
    engine.run_until_quiesce().await.unwrap();
}

/// Task 2 (preon routing): a REPO `.hadron/preons/*.md` preon must reach
/// actual dispatch, not just parse (`preons.rs` Task 1 proved parsing in
/// isolation). `@security-reviewer` is neither a card id, a reserved alias,
/// nor a role — it's a preon whose `preferred_role` is `security`; the
/// engine must resolve it to the seat carrying that role and excite it, the
/// same unaddressed-human-message path an `@role` mention already takes.
///
/// `Engine::new` leaves `global_preons_dir` at its `None` default, so this
/// test never touches the real `~/.hadron/preons` — only the repo (tempdir)
/// preon is in play, mirroring `engine_loads_repo_skills`.
#[tokio::test]
async fn engine_routes_a_repo_preon() {
    use std::fs;
    let fdir = tempdir().unwrap();
    let preons_dir = fdir.path().join(".hadron").join("preons");
    fs::create_dir_all(&preons_dir).unwrap();
    fs::write(
        preons_dir.join("security-reviewer.md"),
        "---\nname: security-reviewer\npreferred_role: security\n---\n\nYou review for security issues.\n",
    )
    .unwrap();

    let path = fdir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(Actor::Human, None, Kind::Message { body: "@security-reviewer please review this diff".into() }),
    )
    .unwrap();

    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("sec")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        fn roles(&self) -> Vec<String> {
            vec!["security".to_string()]
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            assert!(
                turn.task.contains("please review this diff"),
                "the preon-addressed message must reach the role holder's turn:\n{}",
                turn.task
            );
            Ok(TurnOutcome { message: Some("reviewed".into()), permission: None, ..Default::default()})
        }
    }

    let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10);
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&path).unwrap();
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body == "reviewed")),
        "the security seat (resolved via the repo preon's preferred_role) must have actually run and replied:\n{events:#?}"
    );
}

#[tokio::test]
async fn engine_injects_invariants() {
    use std::fs;
    let fdir = tempdir().unwrap();
    
    // The REPO tier: this project's own rules. `always.md` loads every turn;
    // the rest load only when a turn asks for them by name.
    let invariants_dir = fdir.path().join(".hadron").join("nucleus").join("invariants");
    fs::create_dir_all(&invariants_dir).unwrap();
    fs::write(invariants_dir.join("always.md"), "Be nice.").unwrap();
    fs::write(invariants_dir.join("rust_style.md"), "Use camelCase... wait no.").unwrap();
    fs::write(invariants_dir.join("unrequested.md"), "SHOULD-NOT-APPEAR").unwrap();

    let path = fdir.path().join("field.jsonl");
    
    // Create an Assign event requesting "rust_style" invariant
    append_event(
        &path,
        &Event::new(
            Actor::Human,
            Some(QuarkId::new("worker")),
            Kind::Assign { task: "Fix formatting".into(), invariants: vec!["rust_style".to_string()] },
        ),
    ).unwrap();

    use hadron_lattice::{Projection, TurnOutcome};
    struct Probe;
    #[async_trait::async_trait]
    impl crate::quark::Quark for Probe {
        fn id(&self) -> QuarkId {
            QuarkId::new("worker")
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> hadron_lattice::EnergyState {
            hadron_lattice::EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            // Tier 1 — the hardcoded Standard Model, present without any file on disk.
            assert!(
                turn.invariants.contains("Prove it runs"),
                "the compiled-in Standard Model must reach every turn"
            );
            // Tier 3 — this repo's always-on rule, and the one this turn asked for.
            assert!(turn.invariants.contains("Be nice."));
            assert!(turn.invariants.contains("# Project rule: rust_style"));
            assert!(turn.invariants.contains("Use camelCase... wait no."));
            // …but NOT a repo rule nobody asked for.
            assert!(
                !turn.invariants.contains("SHOULD-NOT-APPEAR"),
                "an unrequested repo rule must not be injected"
            );
            assert_eq!(
                turn.available_invariants,
                vec!["always".to_string(), "rust_style".to_string(), "unrequested".to_string()]
            );
            Ok(TurnOutcome { message: Some("done".into()), permission: None, ..Default::default()})
        }
    }

    let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10);
    engine.run_until_quiesce().await.unwrap();
}

/// A quark that holds `running` true for the length of its turn, and records
/// whether its *sibling* was mid-turn at the moment it was excited. Two of these
/// pointed at each other prove overlap directly: if neither ever observed the
/// other running, the turns were serialised.
struct OverlapQuark {
    id: QuarkId,
    /// Set for the duration of *this* quark's turn.
    running: Arc<std::sync::atomic::AtomicBool>,
    /// The sibling's flag, sampled on entry.
    sibling_running: Arc<std::sync::atomic::AtomicBool>,
    /// True if the sibling was mid-turn when this quark was excited.
    saw_sibling: Arc<std::sync::atomic::AtomicBool>,
    hold: std::time::Duration,
}

#[async_trait::async_trait]
impl crate::quark::Quark for OverlapQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
        use std::sync::atomic::Ordering;
        if self.sibling_running.load(Ordering::SeqCst) {
            self.saw_sibling.store(true, Ordering::SeqCst);
        }
        self.running.store(true, Ordering::SeqCst);
        tokio::time::sleep(self.hold).await;
        self.running.store(false, Ordering::SeqCst);
        Ok(TurnOutcome { message: Some("done".into()), permission: None, ..Default::default()})
    }
}

/// **The burst bug.** A human fires several unaddressed messages to different quarks
/// before any is dispatched. The old `human_message_targets` looked only at the
/// single latest message (`rposition`), so the earlier requests were silently
/// abandoned — the exact "I typed @Claude three times and got nothing" complaint.
/// Every unserved quark must now get its MOST RECENT mention.
#[tokio::test]
async fn every_unserved_human_message_is_serviced_not_just_the_latest() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    // Claude asked twice, agy once in between — nobody dispatched yet.
    for body in ["@claude do X", "@agy do Y", "@claude actually do Z"] {
        append_event(&path, &Event::new(Actor::Human, None, Kind::Message { body: body.into() }))
            .unwrap();
    }

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let ids: Vec<&str> = targets.iter().map(|(q, _)| q.as_str()).collect();

    assert!(ids.contains(&"claude"), "claude's request was abandoned: {ids:?}");
    assert!(ids.contains(&"agy"), "agy's request was abandoned: {ids:?}");
    // Claude is serviced for its LATEST mention (Z), not the stale earlier one (X).
    let claude_task = targets
        .iter()
        .find(|(q, _)| q.as_str() == "claude")
        .map(|(_, t)| t.task.as_str())
        .unwrap();
    assert!(claude_task.contains("do Z"), "claude got a stale request: {claude_task:?}");
}

/// **The stranding bug.** A quark is dispatched, emits its `Excited` status, and then
/// the turn is interrupted (daemon restart, crash) before any reply or terminal status
/// is written. The `Excited` status carries no `answers` stamp, so the old `has_answered`
/// hit its legacy `None => true` arm and counted "I started" as "I answered" — leaving the
/// quark marked answered and never re-dispatched. `next_pending` never made this mistake
/// (it filters to replies + terminal statuses), so the two disagreed. A merely-excited
/// quark must still be pending.
#[tokio::test]
async fn a_quark_that_only_went_excited_is_still_pending_not_stranded() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    append_event(&path, &Event::new(Actor::Human, None, Kind::Message { body: "@claude do X".into() }))
        .unwrap();
    // Dispatched and started, then interrupted: Excited only — no reply, no terminal status.
    append_event(
        &path,
        &Event::new(
            Actor::Quark(QuarkId::new("claude")),
            None,
            Kind::Status { state: hadron_lattice::QuarkState::Excited },
        ),
    )
    .unwrap();

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let ids: Vec<&str> = targets.iter().map(|(q, _)| q.as_str()).collect();
    assert!(
        ids.contains(&"claude"),
        "a quark that only went Excited must still be pending, not stranded: {ids:?}"
    );
}

/// The other side of the same predicate: a real reply (or a terminal status) DOES count
/// as answered, so a quark that actually finished is not re-dispatched forever.
#[tokio::test]
async fn a_reply_or_terminal_status_counts_as_answered() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    let human = Event::new(Actor::Human, None, Kind::Message { body: "@claude do X".into() });
    let msg_id = human.id;
    append_event(&path, &human).unwrap();
    // Started, then genuinely finished: a reply stamped as answering this message.
    append_event(
        &path,
        &Event::new(
            Actor::Quark(QuarkId::new("claude")),
            None,
            Kind::Status { state: hadron_lattice::QuarkState::Excited },
        ),
    )
    .unwrap();
    append_event(
        &path,
        &Event::new(Actor::Quark(QuarkId::new("claude")), None, Kind::Message { body: "done".into() })
            .answering(Some(msg_id)),
    )
    .unwrap();

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let ids: Vec<&str> = targets.iter().map(|(q, _)| q.as_str()).collect();
    assert!(!ids.contains(&"claude"), "a quark that replied must not be re-dispatched: {ids:?}");
}

/// Two quarks named in ONE message must run at the same time, not one after the
/// other. This is the whole point of the concurrent dispatch loop: "@a do X and
/// @b do Y" should not make b wait out a's entire turn.
#[tokio::test]
async fn two_quarks_named_in_one_message_run_concurrently() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "@a do X and @b do Y".into() },
        ),
    )
    .unwrap();

    let a_running = Arc::new(AtomicBool::new(false));
    let b_running = Arc::new(AtomicBool::new(false));
    let overlap = Arc::new(AtomicBool::new(false));
    let hold = std::time::Duration::from_millis(200);

    let mut engine = Engine::new(
        path.clone(),
        vec![
            Box::new(OverlapQuark {
                id: QuarkId::new("a"),
                running: a_running.clone(),
                sibling_running: b_running.clone(),
                saw_sibling: overlap.clone(),
                hold,
            }),
            Box::new(OverlapQuark {
                id: QuarkId::new("b"),
                running: b_running.clone(),
                sibling_running: a_running.clone(),
                saw_sibling: overlap.clone(),
                hold,
            }),
        ],
        10,
    );
    engine.run_until_quiesce().await.unwrap();

    assert!(
        overlap.load(Ordering::SeqCst),
        "the two turns never overlapped — dispatch is still serial"
    );
}

#[tokio::test]
async fn multiple_mentions_in_quark_message_run_concurrently() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");

    // Seed the event: a message from the orchestrator containing line-start mentions
    let orch_msg = Event::new(
        Actor::Quark(QuarkId::new("orch")),
        None,
        Kind::Message {
            body: "Plan:\n@a do X\n@b do Y".into(),
        },
    );
    append_event(&path, &orch_msg).unwrap();

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("a"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("b"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let ids: Vec<&str> = targets.iter().map(|(q, _)| q.as_str()).collect();

    assert!(ids.contains(&"a"), "worker a was not targeted: {ids:?}");
    assert!(ids.contains(&"b"), "worker b was not targeted: {ids:?}");
}

#[test]
fn gluon_messages_addressed_to_a_quark_are_routed_to_target() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");

    let gluon_msg = Event::new(
        Actor::Gluon,
        None,
        Kind::Message {
            body: "@orch Quark `acp-agy` turn errored: API key missing".into(),
        },
    );
    append_event(&path, &gluon_msg).unwrap();

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let ids: Vec<&str> = targets.iter().map(|(q, _)| q.as_str()).collect();

    assert!(ids.contains(&"orch"), "orchestrator was not targeted by Gluon message: {ids:?}");
}


#[tokio::test]
async fn multiple_mentions_in_quark_reply_results_in_unaddressed_event() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("a"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("b"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    // Let the orchestrator run a turn and emit a reply with multiple mentions
    let outcome = TurnOutcome {
        message: Some("Plan:\n@a do X\n@b do Y".into()),
        ..Default::default()
    };
    engine.finish_turn(&QuarkId::new("orch"), outcome, None, Some(ulid::Ulid::new())).await.unwrap();

    // Check that the written event has to: None
    let events = read_events(&path).unwrap();
    let reply_ev = events.iter().find(|e| e.from == Actor::Quark(QuarkId::new("orch")) && matches!(e.kind, Kind::Message { .. })).unwrap();
    assert!(reply_ev.to.is_none(), "quark message with multiple mentions must have to: None");
}

/// The behaviour the human actually asked for: while a worker grinds through a
/// long turn, a message arriving for a DIFFERENT quark must be picked up straight
/// away, not queued behind the running turn. Otherwise handing a big task to one
/// quark freezes the conversation with every other quark — which is exactly the
/// "waiting is a killer" complaint.
///
/// This is strictly stronger than fanning out one multi-mention message: it
/// requires the loop to keep *re-reading the field* while turns are in flight.
#[tokio::test]
async fn a_message_arriving_mid_turn_is_dispatched_without_waiting() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    // Only the slow worker is addressed to begin with.
    seed_human_message(&path, "slow", "a big grinding task");

    let slow_running = Arc::new(AtomicBool::new(false));
    let fast_running = Arc::new(AtomicBool::new(false));
    let fast_saw_slow = Arc::new(AtomicBool::new(false));

    // Mid-flight, the human sends a second message to the *other* quark.
    let mid_flight = {
        let path = path.clone();
        let slow_running = slow_running.clone();
        tokio::spawn(async move {
            // Wait until the slow turn is genuinely underway.
            for _ in 0..100 {
                if slow_running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            seed_human_message(&path, "fast", "quick question");
        })
    };

    let mut engine = Engine::new(
        path.clone(),
        vec![
            Box::new(OverlapQuark {
                id: QuarkId::new("slow"),
                running: slow_running.clone(),
                // The slow quark doesn't care what the fast one is doing.
                sibling_running: Arc::new(AtomicBool::new(false)),
                saw_sibling: Arc::new(AtomicBool::new(false)),
                hold: std::time::Duration::from_millis(1500),
            }),
            Box::new(OverlapQuark {
                id: QuarkId::new("fast"),
                running: fast_running.clone(),
                sibling_running: slow_running.clone(),
                saw_sibling: fast_saw_slow.clone(),
                hold: std::time::Duration::from_millis(10),
            }),
        ],
        10,
    );
    engine.run_until_quiesce().await.unwrap();
    mid_flight.await.unwrap();

    assert!(
        fast_saw_slow.load(Ordering::SeqCst),
        "the fast quark only ran AFTER the slow turn finished — a message arriving \
         mid-turn is still queued behind the grinding worker"
    );
}

/// **Task 6, Step 1 of the responsive-orchestrator plan.** A second human
/// message to a quark that is already mid-turn must, for an
/// `Orchestrator`-flavoured seat that has been given a chat lane
/// (`Engine::seat_chat_lane`), be able to start running on that lane *while
/// the work lane is still going* (Jake's "orchestrator never blocks"). A
/// `Worker`-flavoured seat — which never gets a chat lane — keeps today's
/// behaviour exactly: its second turn does not start until the first
/// resolves.
#[tokio::test]
async fn a_busy_orchestrator_can_take_a_second_turn_but_a_busy_worker_cannot() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct SlowCountingQuark {
        id: QuarkId,
        flavor: Flavor,
        call_count: Arc<AtomicUsize>,
        first_turn_running: Arc<AtomicBool>,
        second_call_overlapped_first: Arc<AtomicBool>,
        hold: std::time::Duration,
    }

    #[async_trait::async_trait]
    impl crate::quark::Quark for SlowCountingQuark {
        fn id(&self) -> QuarkId {
            self.id.clone()
        }
        fn flavor(&self) -> Flavor {
            self.flavor.clone()
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            let n = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
            if n == 1 {
                self.first_turn_running.store(true, Ordering::SeqCst);
                tokio::time::sleep(self.hold).await;
                self.first_turn_running.store(false, Ordering::SeqCst);
            } else if self.first_turn_running.load(Ordering::SeqCst) {
                self.second_call_overlapped_first.store(true, Ordering::SeqCst);
            }
            Ok(TurnOutcome { message: Some("done".into()), permission: None, ..Default::default()})
        }
    }

    /// Runs the scenario for one flavour and reports whether the second turn
    /// overlapped the first. An orchestrator gets a chat-lane instance
    /// sharing the same atomics as the work lane, so "overlapped" is
    /// detectable across either instance; a worker gets no chat lane at all.
    async fn overlapped_for(flavor: Flavor) -> bool {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        // Quark-authored on purpose (Task 6 Step 5: "everything else" routes to Work) —
        // a hand-off/self-delegated task, not a human ask, so it lands on the work lane
        // and leaves the chat lane free for the human message below.
        append_event(
            &path,
            &Event::new(
                Actor::Quark(QuarkId::new("reporter")),
                Some(QuarkId::new("seat")),
                Kind::Message { body: "first, slow task".into() },
            ),
        )
        .unwrap();

        let first_turn_running = Arc::new(AtomicBool::new(false));
        let overlapped = Arc::new(AtomicBool::new(false));
        let call_count = Arc::new(AtomicUsize::new(0));
        let mid_flight = {
            let path = path.clone();
            let running = first_turn_running.clone();
            tokio::spawn(async move {
                for _ in 0..200 {
                    if running.load(Ordering::SeqCst) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                seed_human_message(&path, "seat", "second, urgent question");
            })
        };

        let is_orchestrator = flavor == Flavor::Orchestrator;
        let mut engine = Engine::new(
            path.clone(),
            vec![Box::new(SlowCountingQuark {
                id: QuarkId::new("seat"),
                flavor: flavor.clone(),
                call_count: call_count.clone(),
                first_turn_running: first_turn_running.clone(),
                second_call_overlapped_first: overlapped.clone(),
                hold: std::time::Duration::from_millis(500),
            })],
            10,
        );
        if is_orchestrator {
            engine.seat_chat_lane(
                &QuarkId::new("seat"),
                Box::new(SlowCountingQuark {
                    id: QuarkId::new("seat"),
                    flavor,
                    call_count,
                    first_turn_running,
                    second_call_overlapped_first: overlapped.clone(),
                    hold: std::time::Duration::from_millis(500),
                }),
            );
        }
        engine.run_until_quiesce().await.unwrap();
        mid_flight.await.unwrap();
        overlapped.load(Ordering::SeqCst)
    }

    assert!(
        overlapped_for(Flavor::Orchestrator).await,
        "a busy orchestrator's second message must start on its chat lane before the \
         work lane's turn resolves (Task 6)"
    );
    assert!(
        !overlapped_for(Flavor::Worker).await,
        "a busy worker's second message must still wait for the running turn to finish"
    );
}

/// **Task 6, Step 2.** The same regression guard as above, but against a real
/// `CliQuark` rather than a bare `Quark` mock — the lane split must not be
/// something only `AcpQuark` gets. `CliSpec::generic` has `ResumeMode::None`,
/// so this exercises no resume semantics (that is Task 6a's job); it only
/// proves the engine-level in_flight gate, which is transport-agnostic.
#[tokio::test]
async fn a_busy_cli_backed_orchestrator_can_take_a_second_turn() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct SlowRunner {
        call_count: Arc<AtomicUsize>,
        first_turn_running: Arc<AtomicBool>,
        second_call_overlapped_first: Arc<AtomicBool>,
        hold: std::time::Duration,
    }

    #[async_trait::async_trait]
    impl crate::adapter::runner::CliRunner for SlowRunner {
        async fn run(
            &self,
            _inv: crate::adapter::runner::CliInvocation,
        ) -> anyhow::Result<crate::adapter::runner::CliResult> {
            let n = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
            if n == 1 {
                self.first_turn_running.store(true, Ordering::SeqCst);
                tokio::time::sleep(self.hold).await;
                self.first_turn_running.store(false, Ordering::SeqCst);
            } else if self.first_turn_running.load(Ordering::SeqCst) {
                self.second_call_overlapped_first.store(true, Ordering::SeqCst);
            }
            Ok(crate::adapter::runner::CliResult { stdout: "done".into(), exit: 0 })
        }
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    // Quark-authored, same reason as the mock-quark test above: "everything else"
    // (Task 6 Step 5) routes to Work, leaving the chat lane free for the human ask.
    append_event(
        &path,
        &Event::new(
            Actor::Quark(QuarkId::new("reporter")),
            Some(QuarkId::new("cliseat")),
            Kind::Message { body: "first, slow task".into() },
        ),
    )
    .unwrap();

    let first_turn_running = Arc::new(AtomicBool::new(false));
    let overlapped = Arc::new(AtomicBool::new(false));
    let mid_flight = {
        let path = path.clone();
        let running = first_turn_running.clone();
        tokio::spawn(async move {
            for _ in 0..200 {
                if running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            seed_human_message(&path, "cliseat", "second, urgent question");
        })
    };

    let call_count = Arc::new(AtomicUsize::new(0));
    let work = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Orchestrator,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        SlowRunner {
            call_count: call_count.clone(),
            first_turn_running: first_turn_running.clone(),
            second_call_overlapped_first: overlapped.clone(),
            hold: std::time::Duration::from_millis(500),
        },
    );
    let chat = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Orchestrator,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        SlowRunner {
            call_count,
            first_turn_running: first_turn_running.clone(),
            second_call_overlapped_first: overlapped.clone(),
            hold: std::time::Duration::from_millis(500),
        },
    );

    let mut engine = Engine::new(path.clone(), vec![Box::new(work)], 10);
    engine.seat_chat_lane(&QuarkId::new("cliseat"), Box::new(chat));
    engine.run_until_quiesce().await.unwrap();
    mid_flight.await.unwrap();

    assert!(
        overlapped.load(Ordering::SeqCst),
        "a busy CLI-backed orchestrator's second message must start before the first \
         turn resolves — a lane split written only against AcpQuark would miss this"
    );
}

#[tokio::test]
async fn a_reseat_retains_the_orchestrators_chat_lane() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct ReseatSlowRunner {
        call_count: Arc<AtomicUsize>,
        first_turn_running: Arc<AtomicBool>,
        second_call_overlapped_first: Arc<AtomicBool>,
        hold: std::time::Duration,
    }

    #[async_trait::async_trait]
    impl crate::adapter::runner::CliRunner for ReseatSlowRunner {
        async fn run(
            &self,
            _inv: crate::adapter::runner::CliInvocation,
        ) -> anyhow::Result<crate::adapter::runner::CliResult> {
            let n = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
            if n == 1 {
                self.first_turn_running.store(true, Ordering::SeqCst);
                tokio::time::sleep(self.hold).await;
                self.first_turn_running.store(false, Ordering::SeqCst);
            } else if self.first_turn_running.load(Ordering::SeqCst) {
                self.second_call_overlapped_first.store(true, Ordering::SeqCst);
            }
            Ok(crate::adapter::runner::CliResult { stdout: "done".into(), exit: 0 })
        }
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");

    append_event(
        &path,
        &Event::new(
            Actor::Quark(QuarkId::new("reporter")),
            Some(QuarkId::new("cliseat")),
            Kind::Message { body: "first, slow task".into() },
        ),
    )
    .unwrap();

    let first_turn_running = Arc::new(AtomicBool::new(false));
    let overlapped = Arc::new(AtomicBool::new(false));
    let mid_flight = {
        let path = path.clone();
        let running = first_turn_running.clone();
        tokio::spawn(async move {
            for _ in 0..200 {
                if running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            seed_human_message(&path, "cliseat", "second, urgent question");
        })
    };

    let call_count = Arc::new(AtomicUsize::new(0));
    let work = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Orchestrator,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        ReseatSlowRunner {
            call_count: call_count.clone(),
            first_turn_running: first_turn_running.clone(),
            second_call_overlapped_first: overlapped.clone(),
            hold: std::time::Duration::from_millis(500),
        },
    );
    let chat = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Orchestrator,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        ReseatSlowRunner {
            call_count: call_count.clone(),
            first_turn_running: first_turn_running.clone(),
            second_call_overlapped_first: overlapped.clone(),
            hold: std::time::Duration::from_millis(500),
        },
    );

    let mut engine = Engine::new(path.clone(), vec![Box::new(work)], 10);
    engine.seat_chat_lane(&QuarkId::new("cliseat"), Box::new(chat));

    let replacement_work = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Orchestrator,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        ReseatSlowRunner {
            call_count: call_count.clone(),
            first_turn_running: first_turn_running.clone(),
            second_call_overlapped_first: overlapped.clone(),
            hold: std::time::Duration::from_millis(500),
        },
    );
    let replacement_chat = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Orchestrator,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        ReseatSlowRunner {
            call_count: call_count.clone(),
            first_turn_running: first_turn_running.clone(),
            second_call_overlapped_first: overlapped.clone(),
            hold: std::time::Duration::from_millis(500),
        },
    );

    engine.seat(Box::new(replacement_work));
    engine.seat_chat_lane(&QuarkId::new("cliseat"), Box::new(replacement_chat));

    engine.run_until_quiesce().await.unwrap();
    mid_flight.await.unwrap();

    assert!(
        overlapped.load(Ordering::SeqCst),
        "a reseated orchestrator must retain its chat lane and allow non-blocking concurrent chat turns"
    );
}

#[tokio::test]
async fn a_reseat_clears_chat_lane_when_orchestrator_becomes_worker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("field.jsonl");

    let work = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Orchestrator,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        crate::adapter::runner::FakeRunner::with_stdout(vec!["ok"]),
    );
    let chat = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Orchestrator,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        crate::adapter::runner::FakeRunner::with_stdout(vec!["ok"]),
    );

    let mut engine = Engine::new(path, vec![Box::new(work)], 10);
    engine.seat_chat_lane(&QuarkId::new("cliseat"), Box::new(chat));

    assert!(engine.quarks.get(&QuarkId::new("cliseat")).unwrap().chat.is_some());

    let worker_replacement = crate::adapter::cli::CliQuark::new(
        QuarkId::new("cliseat"),
        Flavor::Worker,
        "",
        hadron_lattice::CliSpec::generic("true".to_string(), vec![]),
        crate::adapter::runner::FakeRunner::with_stdout(vec!["ok"]),
    );
    engine.seat(Box::new(worker_replacement));

    assert!(
        engine.quarks.get(&QuarkId::new("cliseat")).unwrap().chat.is_none(),
        "reseating an orchestrator as a worker must clear the chat lane"
    );
}

/// Writes one file into whatever directory it is told it works in, then replies.
/// It records the `cwd` it was handed, so a test can prove two concurrent quarks
/// were pointed at *different* directories — the property the whole plan exists
/// to establish.
struct WriterQuark {
    id: QuarkId,
    file: &'static str,
    cwds: Arc<Mutex<Vec<PathBuf>>>,
}

#[async_trait::async_trait]
impl crate::quark::Quark for WriterQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        self.cwds.lock().unwrap().push(turn.cwd.clone());
        // Overlap the sibling's turn: both quarks are inside `excite` at once, so
        // if they shared one tree they would both be writing into it concurrently.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        std::fs::write(turn.cwd.join(self.file), format!("from {}\n", self.id.as_str()))?;
        Ok(TurnOutcome {
            message: Some(format!("{} wrote {}", self.id.as_str(), self.file)),
            permission: None,
            ..Default::default()
        })
    }
}

/// **THE DISCRIMINATING TEST.** Two quarks named in one message run *at the same
/// time*. Each writes a file. Their work must be attributable, disjointly: quark
/// `a`'s branch diff shows `a.txt` and NOT `b.txt`, and vice-versa.
///
/// On the pre-worktree engine both CLIs inherited one shared checkout, so there
/// was no per-quark branch to diff at all and both files landed in the same tree —
/// a diff could not attribute a line to a quark even in principle. This test is
/// the proof that hazard is closed: two trees, two branches, two disjoint diffs,
/// and the human's own checkout (`main`) untouched by either.
#[tokio::test]
async fn two_concurrent_quarks_produce_disjoint_attribution() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    append_event(
        &field,
        &Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "@a write a.txt and @b write b.txt".into() },
        ),
    )
    .unwrap();

    let cwds = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(WriterQuark { id: QuarkId::new("a"), file: "a.txt", cwds: cwds.clone() }),
            Box::new(WriterQuark { id: QuarkId::new("b"), file: "b.txt", cwds: cwds.clone() }),
        ],
        10,
    )
    .with_git(root.clone());
    engine.run_until_quiesce().await.unwrap();

    // 1. The two quarks were pointed at DIFFERENT directories. This is the
    //    regression guard: a future change that quietly reverts to one shared
    //    tree fails here even if the branches still exist.
    let cwds = cwds.lock().unwrap().clone();
    assert_eq!(cwds.len(), 2, "both quarks ran");
    assert_ne!(cwds[0], cwds[1], "two concurrent quarks shared one working tree");

    // 2. Each quark has its own worktree on its own branch…
    let trees = crate::worktree::list(&root).unwrap();
    let tree_of = |id: &str| {
        trees
            .iter()
            .find(|w| w.quark == QuarkId::new(id))
            .unwrap_or_else(|| panic!("no worktree for {id}"))
            .clone()
    };
    let (wa, wb) = (tree_of("a"), tree_of("b"));
    assert!(wa.branch.starts_with("quark/a/"), "branch per quark: {}", wa.branch);
    assert!(wb.branch.starts_with("quark/b/"), "branch per quark: {}", wb.branch);

    // 3. …and the branch diffs are DISJOINT. This is the attribution property.
    let base = crate::worktree::default_branch(&root);
    let da = crate::worktree::branch_diff(&wa, &base).unwrap();
    let db = crate::worktree::branch_diff(&wb, &base).unwrap();
    assert!(da.contains("a.txt"), "a's branch carries a's work:\n{da}");
    assert!(!da.contains("b.txt"), "a's branch is CONTAMINATED with b's work:\n{da}");
    assert!(db.contains("b.txt"), "b's branch carries b's work:\n{db}");
    assert!(!db.contains("a.txt"), "b's branch is CONTAMINATED with a's work:\n{db}");

    // 4. The human's own tree is untouched: neither file reached it, and `main`
    //    has no new commits.
    assert!(!root.join("a.txt").exists(), "a quark wrote into the human's checkout");
    assert!(!root.join("b.txt").exists(), "a quark wrote into the human's checkout");
}

/// **THE ATTRIBUTION TEST.** Two quarks commit *concurrently*, and each turn's
/// `Kind::Edit` must carry that quark's OWN commit — not its sibling's.
///
/// This is the property every enforcement idea rests on (a machine-checked
/// Definition of Done can only judge a turn whose work it can name), and it is the
/// one nothing had observed. `finish_turn` decides "this turn committed" by
/// `head_now != t.head_before` (l. 939). That test is only sound because each turn
/// owns its tree: `head_before` is read from the quark's *own* worktree, so a
/// sibling's commit cannot move it. Here both turns are inside `excite` at the same
/// time (`WriterQuark` sleeps to guarantee the overlap), so a shared-HEAD
/// implementation would cross-attribute and fail.
#[tokio::test]
async fn concurrent_commits_are_attributed_to_the_turn_that_made_them() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    append_event(
        &field,
        &Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "@a write a.txt and @b write b.txt".into() },
        ),
    )
    .unwrap();

    let cwds = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(WriterQuark { id: QuarkId::new("a"), file: "a.txt", cwds: cwds.clone() }),
            Box::new(WriterQuark { id: QuarkId::new("b"), file: "b.txt", cwds: cwds.clone() }),
        ],
        10,
    )
    .with_git(root.clone());
    engine.run_until_quiesce().await.unwrap();

    // The turn ended on a commit, and the engine said so — one `Edit` per quark.
    let edits: Vec<(QuarkId, Vec<String>, String)> = read_events(&field)
        .unwrap()
        .into_iter()
        .filter_map(|e| match (e.from, e.kind) {
            (Actor::Quark(q), Kind::Edit { paths, git, .. }) => Some((q, paths, git)),
            _ => None,
        })
        .collect();
    assert_eq!(edits.len(), 2, "each turn reported its own commit: {edits:?}");

    let of = |id: &str| {
        edits
            .iter()
            .find(|(q, ..)| *q == QuarkId::new(id))
            .unwrap_or_else(|| panic!("no Edit event attributed to {id}: {edits:?}"))
            .clone()
    };
    let (_, paths_a, sha_a) = of("a");
    let (_, paths_b, sha_b) = of("b");

    // 1. Each quark is credited with its own file, and ONLY its own. In a shared
    //    tree both files land in one checkout and this cannot hold even in principle.
    assert_eq!(paths_a, vec!["a.txt".to_string()], "a was credited with b's work");
    assert_eq!(paths_b, vec!["b.txt".to_string()], "b was credited with a's work");

    // 2. The commits are DISTINCT, and each is the head of that quark's own branch.
    //    This is what `head_now != head_before` is actually asserting, and it is the
    //    line that would silently mis-fire on a shared HEAD.
    assert_ne!(sha_a, sha_b, "both turns were credited with the SAME commit");
    let trees = crate::worktree::list(&root).unwrap();
    let head_of = |id: &str| {
        let w = trees.iter().find(|w| w.quark == QuarkId::new(id)).expect("worktree");
        crate::worktree::head(&w.path).expect("the turn committed")
    };
    assert_eq!(sha_a, head_of("a"), "a's Edit does not name the commit on a's branch");
    assert_eq!(sha_b, head_of("b"), "b's Edit does not name the commit on b's branch");

    // 3. Neither commit reached the human's branch: nothing lands without the gate.
    let main_head = crate::snapshot::git(&root, &["rev-parse", "HEAD"]).unwrap();
    assert_ne!(main_head, sha_a);
    assert_ne!(main_head, sha_b);
}

/// A merge runner whose tests are green but whose `land` always FAILS — the exact
/// shape the live daemon hit: `git merge --ff-only` refused because the target
/// checkout had an uncommitted local change to a file the branch rewrites.
struct FailingLandRunner;

#[async_trait::async_trait]
impl crate::merge::MergeRunner for FailingLandRunner {
    async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
        Ok((true, String::new()))
    }
    fn land(
        &self,
        _repo_root: &std::path::Path,
        _wt: &crate::worktree::Worktree,
        _base: &str,
    ) -> anyhow::Result<crate::merge::Landed> {
        Err(anyhow::anyhow!(
            "simulated: your local changes to f.txt would be overwritten by merge"
        ))
    }
}

/// **THE LOOP GUARD.** When the merge gate's `land()` fails (a real git error, not a
/// rebase conflict — e.g. the target checkout is dirty with a file the branch
/// rewrites), the turn must still end on a TERMINAL status. Before the fix, `land()`'s
/// `Err` propagated out of `run_until_quiesce`; the daemon's re-invoke loop then re-read
/// the leftover audit `PermissionGrant{→quark}` as an unanswered turn-request and
/// re-dispatched the quark forever (observed live: many `Excited`, zero `Ground`).
///
/// The fix reroutes a failed land to `Blocked` (with an explanatory message), which is a
/// turn-completion — so `next_pending` sees the dangling grant as answered and the loop
/// cannot form. This test fails RED before the fix: `run_until_quiesce` returns `Err`
/// (the propagated land error) and the `unwrap` panics.
#[tokio::test]
async fn a_failing_merge_land_blocks_the_quark_instead_of_looping() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    // Bypass ⇒ the merge is delegated (auto-approved), so the gate takes the SAME
    // audit-grant-then-land path the live loop hit.
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w write w.txt".into() }),
    )
    .unwrap();

    let cwds = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(WriterQuark { id: QuarkId::new("w"), file: "w.txt", cwds })],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(FailingLandRunner));

    // Must NOT propagate the land error. (RED before the fix: this returns Err.)
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    let w = QuarkId::new("w");
    // The turn ends on a terminal Blocked — not stranded mid-turn.
    assert!(
        events.iter().any(|e| e.from == Actor::Quark(w.clone())
            && matches!(e.kind, Kind::Status { state: QuarkState::Blocked })),
        "a failed land must block the quark (terminal), not loop"
    );
    // And it never reports success (no Ground, no landed message).
    assert!(
        !events.iter().any(|e| e.from == Actor::Quark(w.clone())
            && matches!(e.kind, Kind::Status { state: QuarkState::Ground })),
        "a failed land must not ground as if the merge had succeeded"
    );
    // The human is told why, via the orchestrator channel.
    assert!(
        events.iter().any(|e| e.from == Actor::Gluon
            && matches!(&e.kind, Kind::Message { body } if body.contains("could not be merged") || body.contains("merge"))),
        "the failure is reported, not silent"
    );
    // And it is NOT handed back to the quark. This failure is in the TARGET checkout —
    // the human's own tree, carrying uncommitted work — which the quark cannot fix from
    // its worktree and must not try to: quarks run git in that very checkout, so one
    // told "fix the merge" would stash or commit work the human never committed.
    assert!(
        !events.iter().any(|e| e.from == Actor::Gluon
            && e.to.as_ref() == Some(&w)
            && matches!(&e.kind, Kind::Message { body } if body.contains(crate::engine::merge::GATE_HANDBACK_MARKER))),
        "a dirty TARGET checkout is a human's problem — the quark must not be sent to fix it"
    );
}

/// Records the worktree and the task of every turn it is given, and writes a fresh file
/// each turn so its branch always has something the gate could try to land.
struct RecordingWriter {
    id: QuarkId,
    turns: Arc<Mutex<Vec<(PathBuf, String)>>>,
    /// Reply for the FIRST turn only, when that turn must NOT complete the assignment —
    /// a hand-off to a peer leaves the branch unlanded and ungated, which is the only way
    /// to set up the superseded-branch case.
    handoff: Option<&'static str>,
}

#[async_trait::async_trait]
impl crate::quark::Quark for RecordingWriter {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let n = {
            let mut t = self.turns.lock().unwrap();
            t.push((turn.cwd.clone(), turn.task.clone()));
            t.len()
        };
        std::fs::write(turn.cwd.join(format!("w{n}.txt")), format!("turn {n}\n"))?;
        let reply = match self.handoff.filter(|_| n == 1) {
            Some(h) => h.to_string(),
            None => format!("wrote w{n}.txt"),
        };
        Ok(TurnOutcome {
            message: Some(reply),
            permission: None,
            ..Default::default()
        })
    }
}

/// A runner whose tests are permanently RED — the commonest quark-fixable merge refusal.
/// `land` is unreachable by construction: a red branch never reaches it.
struct RedTestRunner;

#[async_trait::async_trait]
impl crate::merge::MergeRunner for RedTestRunner {
    async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
        Ok((false, "test tests::it_works ... FAILED".to_string()))
    }
    fn land(
        &self,
        _repo_root: &std::path::Path,
        _wt: &crate::worktree::Worktree,
        _base: &str,
    ) -> anyhow::Result<crate::merge::Landed> {
        panic!("a branch with red tests must never reach land()")
    }
}

/// **THE PROPERTY THE WHOLE DESIGN TURNS ON.** A merge refusal the quark can actually
/// fix must hand the branch back to that quark — on the SAME branch, in the SAME
/// worktree — instead of parking it `Blocked` and telling only the orchestrator.
///
/// Before this, a gate refusal ended the quark's life on that assignment: it went
/// `Blocked`, the only message went to `@orchestrator`, and the next time anything
/// addressed the quark it resolved a NEW assignment ULID → a new branch name → the
/// superseded-branch check in `run.rs` re-gated the old branch, re-failed identically,
/// and `continue`d. Every later dispatch was consumed by that, so the quark never got
/// another turn: frozen, with the fix sitting in a tree it was never sent back to.
#[tokio::test]
async fn a_quark_fixable_merge_refusal_is_handed_back_on_the_same_branch() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w do the work".into() }),
    )
    .unwrap();

    let turns = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(RecordingWriter { id: QuarkId::new("w"), turns: turns.clone(), handoff: None })],
        20,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(RedTestRunner));

    engine.run_until_quiesce().await.unwrap();

    let turns = turns.lock().unwrap().clone();
    assert!(
        turns.len() > 1,
        "the quark got no repair turn — it was frozen by the refusal (got {} turn(s))",
        turns.len()
    );

    // Same worktree every turn: it is standing in the tree that needs fixing.
    let first_cwd = &turns[0].0;
    assert!(
        turns.iter().all(|(cwd, _)| cwd == first_cwd),
        "a repair turn ran somewhere other than the failing worktree: {turns:#?}"
    );

    // Same BRANCH every turn — the load-bearing half. A repair turn on a fresh branch
    // would be cut off from the very commits it was asked to fix.
    let branches = crate::snapshot::git(&root, &["branch", "--list", "quark/w/*"]).unwrap();
    assert_eq!(
        branches.lines().count(),
        1,
        "the hand-back cut a second branch instead of continuing the failing one:\n{branches}"
    );

    // And the repair turn was TOLD what to fix — the failure is its task, not a notice
    // it has to go digging for.
    let repair_task = &turns[1].1;
    assert!(
        repair_task.contains(crate::engine::merge::GATE_HANDBACK_MARKER),
        "the repair turn's task does not carry the gate's hand-back: {repair_task}"
    );
    assert!(
        repair_task.contains("FAILED"),
        "the repair turn was not shown the test output it must fix: {repair_task}"
    );
}

/// The gate has a SECOND entry point: `run.rs`'s superseded-branch check, which fires
/// when a quark is handed a new assignment while its previous branch still has unlanded
/// commits. A hand-back from there must continue the **old** assignment — the branch that
/// actually failed — not the new one it is about to be given.
///
/// It works because that call site builds its `TurnTree` with
/// `parse_assignment_from_branch(&old_branch)`, so `t.assignment` is already the old
/// ULID and the hand-back stamps it. That is a quiet dependency between two files, and
/// getting it wrong would send the repair turn to a fresh branch cut off from the very
/// commits it was asked to fix — so it is pinned here rather than left to inspection.
#[tokio::test]
async fn a_handback_from_the_superseded_branch_gate_continues_the_old_assignment() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    // `w` hands off to a PEER, not the orchestrator: a peer hand-off is a mid-chain
    // step, so the assignment does not complete, the gate never fires, and `w`'s branch
    // is left with unlanded commits — the precondition the superseded check looks for.
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w do the work".into() }),
    )
    .unwrap();

    let turns = Arc::new(Mutex::new(vec![]));
    let quarks: Vec<Box<dyn crate::quark::Quark>> = vec![
        Box::new(RecordingWriter {
            id: QuarkId::new("w"),
            turns: turns.clone(),
            handoff: Some("@p over to you"),
        }),
        Box::new(MockQuark::repeating(QuarkId::new("p"), Flavor::Worker, "done")),
    ];
    let mut engine = Engine::new(field.clone(), quarks, 20)
        .with_git(root.clone())
        .with_merge_gate(Arc::new(RedTestRunner));
    engine.run_until_quiesce().await.unwrap();

    // The branch `w` left behind, unlanded and ungated.
    let stale_branch = crate::worktree::current_branch(&crate::worktree::trees_dir(&root).join("w"))
        .expect("w should be sitting on its first assignment's branch");
    let stale_assignment = crate::worktree::parse_assignment_from_branch(&stale_branch).unwrap();
    assert_eq!(turns.lock().unwrap().len(), 1, "the gate must not have fired on a peer hand-off");

    // A NEW assignment arrives while that branch is still unlanded and still red.
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w now do something else".into() }),
    )
    .unwrap();
    engine.run_until_quiesce().await.unwrap();

    // Every hand-back is stamped with the OLD assignment — the failing branch's — so the
    // repair turns go back to it rather than to the new task's fresh branch.
    let w = QuarkId::new("w");
    let events = read_events(&field).unwrap();
    let handbacks: Vec<_> = events
        .iter()
        .filter(|e| e.from == Actor::Gluon
            && e.to.as_ref() == Some(&w)
            && matches!(&e.kind, Kind::Message { body } if body.contains(crate::engine::merge::GATE_HANDBACK_MARKER)))
        .collect();
    assert!(!handbacks.is_empty(), "the superseded-branch gate froze the quark instead of handing back");
    assert!(
        handbacks.iter().all(|e| e.answers == Some(stale_assignment)),
        "a hand-back named the new assignment, which would cut a branch without the failing commits"
    );

    // And the repair turns ran on that same branch — no second branch was ever cut.
    let branches = crate::snapshot::git(&root, &["branch", "--list", "quark/w/*"]).unwrap();
    assert_eq!(
        branches.lines().count(),
        1,
        "the new assignment cut a branch while the old one was still failing:\n{branches}"
    );
    assert!(branches.contains(&stale_branch), "the failing branch was not the one repaired");
    assert!(
        turns.lock().unwrap().iter().skip(1).any(|(_, task)| task
            .contains(crate::engine::merge::GATE_HANDBACK_MARKER)),
        "no repair turn was actually dispatched"
    );
}

/// A runner whose branch cannot even be replayed onto `base`. Tests and `land` are
/// unreachable by construction: a branch that will not rebase is never tested and never
/// landed. This is the hand-back site that fires EARLIEST in the gate — before the field
/// is read, before the mode ladder — and the one that invites the quark into rebase
/// surgery, so it gets its own coverage rather than riding on the red-tests path.
struct ConflictingSyncRunner;

#[async_trait::async_trait]
impl crate::merge::MergeRunner for ConflictingSyncRunner {
    async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
        panic!("a branch that cannot rebase onto base must never be tested")
    }
    fn sync(&self, _wt: &crate::worktree::Worktree, _base: &str) -> crate::merge::Synced {
        crate::merge::Synced::Conflicted("CONFLICT (content): Merge conflict in f.txt".to_string())
    }
    fn land(
        &self,
        _repo_root: &std::path::Path,
        _wt: &crate::worktree::Worktree,
        _base: &str,
    ) -> anyhow::Result<crate::merge::Landed> {
        panic!("a branch that cannot rebase onto base must never be landed")
    }
}

/// A rebase conflict is a machine refusing to guess — NOT a reason to freeze the one
/// party who can actually judge the resolution. The quark goes back into its own tree,
/// on its own branch, and is shown the conflict.
#[tokio::test]
async fn a_rebase_conflict_sends_the_quark_back_to_resolve_it() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w do the work".into() }),
    )
    .unwrap();

    let turns = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(RecordingWriter { id: QuarkId::new("w"), turns: turns.clone(), handoff: None })],
        20,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(ConflictingSyncRunner));

    engine.run_until_quiesce().await.unwrap();

    let turns = turns.lock().unwrap().clone();
    assert!(turns.len() > 1, "a rebase conflict froze the quark instead of handing it back");
    assert_eq!(turns[0].0, turns[1].0, "the repair turn must run in the conflicting worktree");
    assert!(
        turns[1].1.contains(crate::engine::merge::GATE_HANDBACK_MARKER)
            && turns[1].1.contains("CONFLICT (content)"),
        "the quark was sent back without being shown the conflict: {}",
        turns[1].1
    );
    // Same bound as every other hand-back — a conflict it cannot resolve still escalates.
    assert_eq!(turns.len(), 3, "one original turn plus two repair turns — no more");
}

/// The bound. Each hand-back costs a full test run plus a model turn, so a branch that
/// will not heal must escalate to a human rather than loop. Two repair turns, then the
/// gate stops spending and says so — the quark is left `Blocked` with its branch intact.
///
/// Run **with an orchestrator seated** — Jake's live topology, and the only shape where
/// the escalation is actually routed rather than left as a bare notice. It is also the
/// shape with two ways to loop: the escalation body gains an `@orchestrator` prefix while
/// still naming `@w` in its text, and the orchestrator's own turn adds a second addressed
/// candidate for `next_pending` to consider. Quiescing here is the assertion.
#[tokio::test]
async fn a_branch_that_will_not_heal_escalates_instead_of_looping() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w do the work".into() }),
    )
    .unwrap();

    let turns = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "ok")),
            Box::new(RecordingWriter { id: QuarkId::new("w"), turns: turns.clone(), handoff: None }),
        ],
        20,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(RedTestRunner));

    // It quiesces at all — that is half the assertion. A hand-back that never gave up
    // would spin here until the exchange backstop, burning a test run per pass.
    engine.run_until_quiesce().await.unwrap();

    let w = QuarkId::new("w");
    let events = read_events(&field).unwrap();
    let handbacks = events
        .iter()
        .filter(|e| e.from == Actor::Gluon
            && e.to.as_ref() == Some(&w)
            && matches!(&e.kind, Kind::Message { body } if body.contains(crate::engine::merge::GATE_HANDBACK_MARKER)))
        .count();
    assert_eq!(handbacks, 2, "the gate must hand a failing branch back exactly twice");
    assert_eq!(
        turns.lock().unwrap().len(),
        3,
        "one original turn plus two repair turns — no more"
    );

    // The give-up is explicit, and the quark ends terminal rather than mid-turn.
    assert!(
        events.iter().any(|e| e.from == Actor::Gluon
            && e.to.is_none()
            && matches!(&e.kind, Kind::Message { body } if body.contains("escalating"))),
        "the gate gave up silently — a human is never told the branch is stuck"
    );
    assert!(
        events.iter().rev().any(|e| e.from == Actor::Quark(w.clone())
            && matches!(e.kind, Kind::Status { state: QuarkState::Blocked })),
        "the quark must end Blocked, not stranded"
    );
    // Nothing was landed, and the work is still there.
    assert!(!root.join("w1.txt").exists(), "a red branch must not reach the base");
    assert!(
        crate::snapshot::git(&root, &["branch", "--list", "quark/w/*"]).unwrap().contains("quark/w/"),
        "the branch must be preserved — the work is evidence"
    );
}

/// A worker that WRITES a file (so there is a commit to land) and reports completion
/// with a configurable reply — used to prove a worker's `@orchestrator`-addressed
/// completion gates and lands, not only a no-mention hand-back.
struct ReportingWriter {
    id: QuarkId,
    file: &'static str,
    reply: &'static str,
}

#[async_trait::async_trait]
impl crate::quark::Quark for ReportingWriter {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        std::fs::write(turn.cwd.join(self.file), format!("from {}\n", self.id.as_str()))?;
        Ok(TurnOutcome {
            message: Some(self.reply.to_string()),
            permission: None,
            ..Default::default()
        })
    }
}

/// Green tests + a REAL `git merge --ff-only` land — the success path `FailingLandRunner`
/// never exercises. Proves the branch actually reaches `base`.
struct GreenLandingRunner;

#[async_trait::async_trait]
impl crate::merge::MergeRunner for GreenLandingRunner {
    async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
        Ok((true, String::new()))
    }
    fn land(
        &self,
        repo_root: &std::path::Path,
        wt: &crate::worktree::Worktree,
        base: &str,
    ) -> anyhow::Result<crate::merge::Landed> {
        crate::merge::land(repo_root, wt, base)
    }
}

/// **TASK 6.** A worker that reports completion to the orchestrator (`@orchestrator done`)
/// must have its branch GATED and LANDED — not stranded because the reply carries a
/// mention. Before the fix the gate fired only on a no-`@mention` hand-back, so a worker
/// completion (which always addresses `@orchestrator`) never landed and the swarm believed
/// work had merged when it had not (cost ~5 turns live). RED before the fix: the worker's
/// commit never reaches `base`.
#[tokio::test]
async fn a_worker_reporting_to_the_orchestrator_lands_its_branch() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    // Bypass ⇒ the merge is delegated (auto-approved) without a human grant.
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w write w.txt".into() }),
    )
    .unwrap();

    let base_before = crate::snapshot::git(&root, &["rev-parse", "HEAD"]).unwrap();

    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "ok")),
            Box::new(ReportingWriter {
                id: QuarkId::new("w"),
                file: "w.txt",
                reply: "@orchestrator done",
            }),
        ],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(GreenLandingRunner));

    engine.run_until_quiesce().await.unwrap();

    // The worker's branch LANDED: base HEAD advanced and now carries w.txt.
    let base_after = crate::snapshot::git(&root, &["rev-parse", "HEAD"]).unwrap();
    assert_ne!(
        base_before, base_after,
        "the worker's completion-to-orchestrator did not land — its branch stranded (Task 6 regression)"
    );
    assert!(root.join("w.txt").exists(), "the landed base is missing the worker's file");

    // The land was reported (not silent).
    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| e.from == Actor::Gluon
            && matches!(&e.kind, Kind::Message { body }
                if body.contains("merged") || body.to_lowercase().contains("fast-forward"))),
        "the land was not reported"
    );
}

/// Passes green, like `GreenLandingRunner`, but its `tests()` sleeps first — long
/// enough for a concurrent poller to observe the gate's heartbeat mid-run.
struct SlowGreenLandingRunner(std::time::Duration);

#[async_trait::async_trait]
impl crate::merge::MergeRunner for SlowGreenLandingRunner {
    async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
        tokio::time::sleep(self.0).await;
        Ok((true, String::new()))
    }
    fn land(
        &self,
        repo_root: &std::path::Path,
        wt: &crate::worktree::Worktree,
        base: &str,
    ) -> anyhow::Result<crate::merge::Landed> {
        crate::merge::land(repo_root, wt, base)
    }
}

/// **Flight Recorder A1, Step 3.** While the gate is actually running, it must have
/// published a fresh `Doing::Gating` heartbeat to `live::gates_dir` — the whole point
/// of the wrapper in `merge_gate`. A poller races the gate itself, so a runner slow
/// enough to still be inside `tests()` when the poller checks is required; without it
/// the gate could finish (and clean up) before anything ever looked.
#[tokio::test]
async fn a_running_gate_publishes_a_fresh_gating_activity() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w write w.txt".into() }),
    )
    .unwrap();

    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "ok")),
            Box::new(ReportingWriter { id: QuarkId::new("w"), file: "w.txt", reply: "@orchestrator done" }),
        ],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(SlowGreenLandingRunner(std::time::Duration::from_millis(400))));

    let gates_dir = hadron_lattice::live::gates_dir(&field);
    let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let poller = {
        let gates_dir = gates_dir.clone();
        let seen = seen.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                let fresh = hadron_lattice::live::gates(&gates_dir, chrono::Utc::now());
                if fresh.iter().any(|a| a.doing == hadron_lattice::Doing::Gating) {
                    seen.store(true, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
    };

    engine.run_until_quiesce().await.unwrap();
    poller.await.unwrap();

    assert!(
        seen.load(std::sync::atomic::Ordering::SeqCst),
        "the gate never published a fresh gating activity while it was running"
    );
}

/// Red tests, like `RedTestRunner`, but slow — so a heartbeat is guaranteed to have
/// actually published before the gate finishes and cleans up (see the comment on
/// `SlowGreenLandingRunner`: without a delay the spawned heartbeat task may never
/// get scheduled before its guard drops, which would pass the negative-control
/// tests below whether or not cleanup ran at all).
struct SlowRedTestRunner(std::time::Duration);

#[async_trait::async_trait]
impl crate::merge::MergeRunner for SlowRedTestRunner {
    async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
        tokio::time::sleep(self.0).await;
        Ok((false, "test tests::it_works ... FAILED".to_string()))
    }
    fn land(
        &self,
        _repo_root: &std::path::Path,
        _wt: &crate::worktree::Worktree,
        _base: &str,
    ) -> anyhow::Result<crate::merge::Landed> {
        panic!("a branch with red tests must never reach land()")
    }
}

/// **Negative control, happy path.** Once the gate has landed, its `live/gates/`
/// entry must be gone — a gate that leaves its file behind shows a quark gating
/// forever, exactly the lie `Activity::is_fresh` exists to bound at 120s, which is
/// still a lie for 120s. Uses `SlowGreenLandingRunner`, not the instant
/// `GreenLandingRunner` — a mutation that deleted the cleanup call still passed
/// against the instant runner, because the heartbeat's first publish never got
/// scheduled before the guard dropped. The delay is what makes this test load-bearing.
#[tokio::test]
async fn a_finished_gate_leaves_no_live_file() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w write w.txt".into() }),
    )
    .unwrap();

    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "ok")),
            Box::new(ReportingWriter { id: QuarkId::new("w"), file: "w.txt", reply: "@orchestrator done" }),
        ],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(SlowGreenLandingRunner(std::time::Duration::from_millis(200))));

    engine.run_until_quiesce().await.unwrap();

    let gates_dir = hadron_lattice::live::gates_dir(&field);
    assert!(
        hadron_lattice::live::gates(&gates_dir, chrono::Utc::now()).is_empty(),
        "a landed gate left its live file behind"
    );
}

/// **Negative control, the error/blocked path** — not just the happy one. Red tests
/// send the gate through `hand_back_to_quark` instead of `land`, and the heartbeat
/// guard must clean up there too: it is a `Drop`, not an explicit call on the
/// success branch alone. Slowed for the same reason as the happy-path test above.
#[tokio::test]
async fn a_blocked_gate_also_leaves_no_live_file() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w do the work".into() }),
    )
    .unwrap();

    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(RecordingWriter { id: QuarkId::new("w"), turns: Arc::new(Mutex::new(vec![])), handoff: None })],
        20,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(SlowRedTestRunner(std::time::Duration::from_millis(200))));

    engine.run_until_quiesce().await.unwrap();

    let gates_dir = hadron_lattice::live::gates_dir(&field);
    assert!(
        hadron_lattice::live::gates(&gates_dir, chrono::Utc::now()).is_empty(),
        "a blocked/handed-back gate left its live file behind"
    );
}

/// Records the order the gate calls the runner's steps in, and otherwise behaves
/// exactly like `GreenLandingRunner` (real `sync`, real `land`).
struct OrderRecordingRunner(Arc<Mutex<Vec<&'static str>>>);

#[async_trait::async_trait]
impl crate::merge::MergeRunner for OrderRecordingRunner {
    async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
        self.0.lock().unwrap().push("tests");
        Ok((true, String::new()))
    }
    fn sync(&self, wt: &crate::worktree::Worktree, base: &str) -> crate::merge::Synced {
        self.0.lock().unwrap().push("sync");
        crate::merge::sync(wt, base)
    }
    fn land(
        &self,
        repo_root: &std::path::Path,
        wt: &crate::worktree::Worktree,
        base: &str,
    ) -> anyhow::Result<crate::merge::Landed> {
        self.0.lock().unwrap().push("land");
        crate::merge::land(repo_root, wt, base)
    }
}

/// **The gate must rebase BEFORE it tests.** The gate used to test the branch exactly
/// as the quark left it and rebase only inside `land`, after the tests had passed. Two
/// live failures came out of that order: a branch cut before a fix landed on `base` was
/// tested WITHOUT that fix, reported red, and could never heal itself (it burned two
/// identical gate cycles on `quark/acp-agy/01KYC48…`); and the rebase inside `land`
/// produced a tree nobody had run while the message claimed it was "re-tested first".
/// Nothing in the type system pins the order, so this is the guard.
#[tokio::test]
async fn the_gate_syncs_the_branch_before_it_runs_the_tests() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w write w.txt".into() }),
    )
    .unwrap();

    let order = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(ReportingWriter {
            id: QuarkId::new("w"),
            file: "w.txt",
            reply: "done",
        })],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(OrderRecordingRunner(order.clone())));

    engine.run_until_quiesce().await.unwrap();

    assert_eq!(
        *order.lock().unwrap(),
        vec!["sync", "tests", "land"],
        "the tests must see the tree that is about to land, not the one before the rebase"
    );
}

/// A runner whose `sync` empties the branch — the outcome git really produces when
/// every commit is already upstream (proved against real git in
/// `merge::tests::a_branch_whose_work_already_landed_syncs_empty_and_lands_as_a_no_op`;
/// here the branch is moved to `base` directly so the engine path can be exercised
/// without staging a second turn's worth of history).
struct EmptyingSyncRunner(Arc<Mutex<Vec<&'static str>>>);

#[async_trait::async_trait]
impl crate::merge::MergeRunner for EmptyingSyncRunner {
    async fn tests(&self, _wt: &crate::worktree::Worktree) -> anyhow::Result<(bool, String)> {
        self.0.lock().unwrap().push("tests");
        Ok((true, String::new()))
    }
    fn sync(&self, wt: &crate::worktree::Worktree, base: &str) -> crate::merge::Synced {
        self.0.lock().unwrap().push("sync");
        crate::snapshot::git(&wt.path, &["reset", "--hard", base]).unwrap();
        crate::merge::Synced::Rebased
    }
    fn land(
        &self,
        _repo_root: &std::path::Path,
        _wt: &crate::worktree::Worktree,
        _base: &str,
    ) -> anyhow::Result<crate::merge::Landed> {
        self.0.lock().unwrap().push("land");
        Ok(crate::merge::Landed::FastForward)
    }
}

/// A branch whose work already reached `base` by another route (someone cherry-picked
/// it forward) must land as a NO-OP and say so — not run the suite, not "merge", and
/// above all not fail the gate on every retry, which is what it did live. The notice
/// matters as much as the skip: this branch had been visibly erroring, and going silent
/// reads as the gate breaking.
#[tokio::test]
async fn a_branch_emptied_by_the_sync_is_reported_as_nothing_to_merge() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w write w.txt".into() }),
    )
    .unwrap();

    let calls = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(ReportingWriter { id: QuarkId::new("w"), file: "w.txt", reply: "done" })],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(EmptyingSyncRunner(calls.clone())));

    engine.run_until_quiesce().await.unwrap();

    assert_eq!(*calls.lock().unwrap(), vec!["sync"], "nothing left to test or land");

    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| e.from == Actor::Gluon
            && matches!(&e.kind, Kind::Message { body } if body.contains("nothing to merge"))),
        "an emptied branch must be reported, not silently dropped"
    );
    // And the quark grounds: an already-landed branch is a completed turn, not a block.
    assert!(
        events.iter().any(|e| e.from == Actor::Quark(QuarkId::new("w"))
            && matches!(e.kind, Kind::Status { state: QuarkState::Ground })),
        "a no-op land must not park the quark"
    );
}

/// **Branch cleanup rides every job, not just daemon startup.** Before this fix,
/// `prune_merged_branches` only ran once at `bin/hadron-gluon.rs` startup, so a
/// long-running daemon accumulated one `quark/*` branch per turn — the disk
/// complaint that triggered this task. Simulate a branch from an EARLIER turn that
/// is already merged into `base` but was never swept (exactly what startup-only
/// pruning leaves behind), then land a fresh turn and prove the stale branch is
/// gone afterward — with no daemon restart in between.
#[tokio::test]
async fn a_successful_land_sweeps_other_already_merged_quark_branches() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();

    // A stale branch from a PRIOR turn: merged into main, never deleted (the gap
    // startup-only pruning leaves between restarts).
    crate::snapshot::git(&root, &["checkout", "-q", "-b", "quark/old/01OLD"]).unwrap();
    std::fs::write(root.join("old.txt"), "old work\n").unwrap();
    crate::snapshot::git(&root, &["add", "-A"]).unwrap();
    crate::snapshot::git(&root, &["commit", "-q", "-m", "old: work"]).unwrap();
    crate::snapshot::git(&root, &["checkout", "-q", "main"]).unwrap();
    crate::snapshot::git(&root, &["merge", "-q", "--ff-only", "quark/old/01OLD"]).unwrap();
    assert!(
        crate::snapshot::git(&root, &["branch", "--list", "quark/old/01OLD"])
            .unwrap()
            .contains("quark/old/01OLD"),
        "the stale branch must exist before the fix kicks in, to prove it gets swept"
    );

    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("w"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@w write w.txt".into() }),
    )
    .unwrap();

    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(ReportingWriter { id: QuarkId::new("w"), file: "w.txt", reply: "done" })],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(GreenLandingRunner));

    engine.run_until_quiesce().await.unwrap();

    // The new turn landed...
    assert!(root.join("w.txt").exists(), "the new turn's work did not land");
    // ...and landing it swept the OTHER quark's already-merged, long-stale branch.
    let remaining = crate::snapshot::git(&root, &["branch", "--list", "quark/old/*"]).unwrap();
    assert!(
        remaining.trim().is_empty(),
        "a successful land must prune other merged quark branches too, still found: {remaining}"
    );
}

/// The other half of the truth, and the reason the daemon attributes nothing today:
/// **without `with_git`, `TurnTree` is never constructed** (l. 1186), so the whole
/// `if let Some(t) = tree` block — `head_before`, `commit_turn`, `Kind::Edit` — is
/// skipped entirely.
///
/// Worth stating precisely, because it corrects the obvious guess: in a shared
/// checkout a turn's commit is not *mis*-attributed to a sibling, it is not
/// attributed **at all**. The engine emits no `Edit` events and never compares HEAD.
/// Attribution is dormant, not broken — and this guard fails the moment someone
/// wires commit-attribution to a shared tree, where it could not be sound.
#[tokio::test]
async fn without_worktree_isolation_the_engine_attributes_no_commit() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@a write a.txt".into() }),
    )
    .unwrap();

    let cwds = Arc::new(Mutex::new(vec![]));
    // NO `.with_git(..)` — exactly how `bin/hadron-gluon.rs` builds the engine today.
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(WriterQuark { id: QuarkId::new("a"), file: "a.txt", cwds: cwds.clone() })],
        10,
    );
    engine.run_until_quiesce().await.unwrap();

    assert_eq!(cwds.lock().unwrap().len(), 1, "the quark ran");
    let edits = read_events(&field)
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e.kind, Kind::Edit { .. }))
        .count();
    assert_eq!(edits, 0, "the engine attributed a commit without owning the tree to prove it");
}

struct OrchestratorDelegatingWriter {
    id: QuarkId,
    file: &'static str,
    delegate_to: &'static str,
}

#[async_trait::async_trait]
impl Quark for OrchestratorDelegatingWriter {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, projection: Projection) -> anyhow::Result<TurnOutcome> {
        std::fs::write(projection.cwd.join(self.file), "orch content\n")?;
        Ok(TurnOutcome {
            message: Some(format!("@{} please work on this", self.delegate_to)),
            permission: None,
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn orchestrator_delegating_turn_does_not_land_immediately() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");

    seed_mode(&field, Some("orch"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@orch delegate task".into() }),
    )
    .unwrap();

    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(OrchestratorDelegatingWriter {
                id: QuarkId::new("orch"),
                file: "orch1.txt",
                delegate_to: "worker",
            }),
            Box::new(WriterQuark {
                id: QuarkId::new("worker"),
                file: "w.txt",
                cwds: Arc::new(Mutex::new(vec![])),
            }),
        ],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(GreenLandingRunner));

    engine.run_until_quiesce().await.unwrap();

    // The orchestrator delegated to worker, so orchestrator's branch is NOT gated immediately.
    assert!(
        !root.join("orch1.txt").exists(),
        "orchestrator turn that delegated should not land immediately (Task 6 property)"
    );
}

#[tokio::test]
async fn superseded_assignment_branch_is_gated_and_lands_on_new_assignment() {
    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");

    seed_mode(&field, Some("orch"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@orch first task".into() }),
    )
    .unwrap();

    let mut engine = Engine::new(
        field.clone(),
        vec![
            Box::new(OrchestratorDelegatingWriter {
                id: QuarkId::new("orch"),
                file: "orch1.txt",
                delegate_to: "worker",
            }),
            Box::new(WriterQuark {
                id: QuarkId::new("worker"),
                file: "w.txt",
                cwds: Arc::new(Mutex::new(vec![])),
            }),
        ],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(GreenLandingRunner));

    engine.run_until_quiesce().await.unwrap();

    // 1st assignment completed, orch1.txt not landed yet.
    assert!(!root.join("orch1.txt").exists());

    // 2nd assignment for orch.
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@orch second task".into() }),
    )
    .unwrap();

    let mut engine2 = Engine::new(
        field.clone(),
        vec![Box::new(ReportingWriter {
            id: QuarkId::new("orch"),
            file: "orch2.txt",
            reply: "all done",
        })],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(GreenLandingRunner));

    engine2.run_until_quiesce().await.unwrap();

    // The superseded assignment 1 branch was gated when assignment 2 started, landing orch1.txt!
    assert!(
        root.join("orch1.txt").exists(),
        "superseded assignment branch should be gated and land when a new assignment starts"
    );
}

/// **The E2BIG regression test.** `field_window` used to be `events.to_vec()` —
/// the *entire* field, unbounded. A long-running swarm's field renders to
/// hundreds of KB, and `agy` takes its prompt as a single argv element, whose
/// hard kernel limit is `MAX_ARG_STRLEN` = 128 KiB. `execve` then failed with
/// E2BIG in under a millisecond: the quark went excited → error without any
/// subprocess ever starting.
#[tokio::test]
async fn the_field_window_is_bounded_however_big_the_field_grows() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    // ~400 KB of field: 200 messages × ~2 KB of body.
    for i in 0..200 {
        append_event(
            &field,
            &Event::new(
                Actor::Human,
                None,
                Kind::Message { body: format!("msg{i} {}", "x".repeat(2000)) },
            ),
        )
        .unwrap();
    }
    seed_human_message(&field, "agy", "what is the state of things?");
    let events = read_events(&field).unwrap();
    assert!(
        events.iter().map(event_cost).sum::<usize>() > 300_000,
        "precondition: the raw field really is huge"
    );

    let engine = Engine::new(field.clone(), vec![], 8);
    let driver = engine.driver_for(&events, &QuarkId::new("agy"), None);
    let proj = engine.projection_for(
        &events,
        &QuarkId::new("agy"),
        driver.as_ref(),
        String::new(),
        None,
    );

    let cost: usize = proj.field_window.iter().map(event_cost).sum();
    assert!(
        cost <= FIELD_WINDOW_BUDGET_BYTES,
        "the field window must be bounded by the byte budget, got {cost} > {FIELD_WINDOW_BUDGET_BYTES}"
    );
    assert!(!proj.field_window.is_empty(), "but not empty — recent context survives");

    // Most-recent-wins: the driving message is the last event and MUST survive.
    let last = proj.field_window.last().unwrap();
    assert!(
        matches!(&last.kind, Kind::Message { body } if body.contains("state of things")),
        "the newest event is kept, the oldest are the ones dropped"
    );
}

/// `Engine::set_nucleus_index_budget_bytes` is the live-reload seam
/// (`set_max_exchanges`'s sibling): the daemon bin calls it when `team.json`
/// changes, and every projection built after that must carry the new value —
/// never the shipped default silently.
#[test]
fn projection_carries_the_engines_configured_nucleus_index_budget() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "worker", "go");
    let events = read_events(&field).unwrap();

    let mut engine = Engine::new(field.clone(), vec![], 8);
    engine.set_nucleus_index_budget_bytes(64 * 1024);

    let driver = engine.driver_for(&events, &QuarkId::new("worker"), None);
    let proj = engine.projection_for(&events, &QuarkId::new("worker"), driver.as_ref(), String::new(), None);
    assert_eq!(proj.nucleus_index_budget_bytes, 64 * 1024);
}

/// **THE DISCRIMINATING TEST for the prompt-bloat trim (WS4 §5).** A resident
/// (ACP) quark used to get `skills::index()` PLUS the entire skill library
/// (`skills::corpus()`) crammed into its cache-stable prefix every turn — ~70-80k
/// tokens of markdown the quark mostly never touches. It must now get the index
/// (so it still knows the full menu) and the ACTIVE skill's body — nothing more.
#[test]
fn resident_quark_gets_index_plus_active_body_not_the_whole_corpus() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "worker", "execute the plan at docs/plans/2026-07-14-acp-auth.md");
    let events = read_events(&field).unwrap();

    let mut engine = Engine::new(field.clone(), vec![], 8);
    // No quark is seated (the engine's turn machinery is irrelevant here — only
    // `projection_for` is under test), so residency is set directly, exactly the
    // way `Engine::seat` records it off `Quark::resident()`.
    engine.resident.insert(QuarkId::new("worker"));

    let driver = engine.driver_for(&events, &QuarkId::new("worker"), None);
    let proj = engine.projection_for(&events, &QuarkId::new("worker"), driver.as_ref(), String::new(), None);

    // (1) The always-on index: a stable index-only line (skill id + its
    // front-matter description), which `render()`'s output never reproduces.
    assert!(
        proj.invariants.contains(
            "**executing-plans** — Use when you have a written implementation plan"
        ),
        "the index must still list every skill:\n{}",
        proj.invariants
    );

    // (2) The MATCHED skill's body — "execute the plan at docs/plans/..." selects
    // executing-plans (the bare `docs/plans/` path is itself a trigger).
    assert!(
        proj.active_skill.as_deref().unwrap_or_default().contains("Load plan, review critically, execute all tasks, report when complete."),
        "the active skill's body must be injected in full:\n{:?}",
        proj.active_skill
    );

    // (3) NOT a body-only line from a DIFFERENT, non-matched skill — this line
    // lives only in brainstorming.md's body, so it can only appear here if the
    // whole corpus were still being dumped.
    assert!(
        !proj.invariants.contains(
            "Help turn ideas into fully formed designs and specs through natural collaborative dialogue."
        ),
        "a resident quark must NOT get the whole skill library any more:\n{}",
        proj.invariants
    );
}

/// The CLI (one-shot) path is the pin: it already got index + active body, never
/// the corpus, and the trim must leave it byte-for-byte the same shape.
#[test]
fn one_shot_quark_still_gets_index_plus_active_body_only() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "worker", "execute the plan at docs/plans/2026-07-14-acp-auth.md");
    let events = read_events(&field).unwrap();

    // Not marked resident — this is the one-shot CLI shape.
    let engine = Engine::new(field.clone(), vec![], 8);

    let driver = engine.driver_for(&events, &QuarkId::new("worker"), None);
    let proj = engine.projection_for(&events, &QuarkId::new("worker"), driver.as_ref(), String::new(), None);

    assert!(proj.invariants.contains("**executing-plans** — Use when you have a written implementation plan"));
    assert!(proj.active_skill.as_deref().unwrap_or_default().contains("Load plan, review critically, execute all tasks, report when complete."));
    assert!(!proj.invariants.contains(
        "Help turn ideas into fully formed designs and specs through natural collaborative dialogue."
    ));
}

// ---- live re-seating -------------------------------------------------------
//
// `team.json` changes while the swarm is running (the human saves a provider in
// Settings). The roster must pick that up — without disturbing the quarks that
// did not change, because an ACP seat carries a *resident session*.

fn engine_with(ids: &[&str], dir: &std::path::Path) -> Engine {
    let quarks: Vec<Box<dyn Quark>> = ids
        .iter()
        .map(|id| {
            Box::new(MockQuark::scripted(
                QuarkId::new(*id),
                Flavor::Worker,
                vec![None],
            )) as Box<dyn Quark>
        })
        .collect();
    Engine::new(dir.join("field.jsonl"), quarks, 12)
}

/// **THE DISCRIMINATING TEST.** Seating a *new* quark must leave every existing
/// quark as the *same instance* — not an equal one, the same one.
///
/// `Arc::ptr_eq` is the only assertion that can tell "reconciled" from "rebuilt
/// everything and got lucky". It is what stands between us and silently dropping a
/// live ACP session (a booted subprocess whose second turn can see its first) every
/// time the human clicks Save in Settings.
#[test]
fn seating_a_new_quark_leaves_the_others_byte_for_byte_untouched() {
    let dir = tempdir().unwrap();
    let mut engine = engine_with(&["opus", "agy"], dir.path());

    let opus_before = engine.quarks.get(&QuarkId::new("opus")).unwrap().work.clone();
    let agy_before = engine.quarks.get(&QuarkId::new("agy")).unwrap().work.clone();

    engine.seat(Box::new(MockQuark::scripted(
        QuarkId::new("acp-claude"),
        Flavor::Worker,
        vec![None],
    )));

    assert_eq!(engine.seated_count(), 3, "the new seat joined the live roster");
    assert!(
        Arc::ptr_eq(&opus_before, &engine.quarks.get(&QuarkId::new("opus")).unwrap().work),
        "opus was rebuilt by a re-seat that had nothing to do with it"
    );
    assert!(
        Arc::ptr_eq(&agy_before, &engine.quarks.get(&QuarkId::new("agy")).unwrap().work),
        "agy was rebuilt by a re-seat that had nothing to do with it"
    );
}

// ---- participation (enable / disable) --------------------------------------

/// **The security property (rule 7).** Disable is an *authority reduction*, so the
/// risk runs the other way: the failure is a disabled quark that still takes a turn.
///
/// This drives the real engine loop and proves the turn never happens — the quark is
/// scripted to shout, and the field must not contain the shout.
#[tokio::test]
async fn a_disabled_quark_does_not_take_a_turn() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(MockQuark::scripted(
            QuarkId::new("agy"),
            Flavor::Worker,
            vec![Some("I ANSWERED".into())],
        ))],
        12,
    );

    engine.set_enabled(&QuarkId::new("agy"), false);
    seed_human_message(&field, "agy", "you there?");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    let spoke = events.iter().any(|e| {
        matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))
            && e.from == Actor::Quark(QuarkId::new("agy"))
    });
    assert!(!spoke, "a DISABLED quark took a turn — the switch does not switch anything");
}

/// And the mention must not vanish. A message that goes nowhere, with no trace, is
/// the failure mode this codebase keeps rediscovering — the human would be left
/// staring at a chat that simply never answered.
///
/// The trace is the `Status{Blocked}`, NOT a chat message: a disabled seat is a
/// switch the human flipped themselves, and restating it once per disabled seat on
/// every `@team` broadcast is noise they asked us to drop. The status still reaches
/// the field and the roster, and the daemon log still names it.
#[tokio::test]
async fn a_mention_of_a_disabled_quark_is_answered_in_the_field_not_dropped() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(MockQuark::scripted(
            QuarkId::new("agy"),
            Flavor::Worker,
            vec![Some("hi".into())],
        ))],
        12,
    );
    engine.set_enabled(&QuarkId::new("agy"), false);
    seed_human_message(&field, "agy", "you there?");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| e.from == Actor::Quark(QuarkId::new("agy"))
            && matches!(e.kind, Kind::Status { state: QuarkState::Blocked })),
        "the field must RECORD the refusal (Blocked), not silently swallow the mention"
    );
    assert!(
        !events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("disabled"))),
        "…but it must not post a chat message: the human flipped that switch themselves, \
         and it fires once per disabled seat on every @team broadcast"
    );
}

/// **Disabling is not unseating.** The quark keeps its exact instance — for an ACP
/// seat that is a live subprocess and a whole conversation. `Arc::ptr_eq` is the only
/// assertion that can tell "kept" from "rebuilt and got lucky".
#[tokio::test]
async fn disabling_keeps_the_very_same_instance_and_re_enabling_uses_it() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(MockQuark::scripted(
            QuarkId::new("agy"),
            Flavor::Worker,
            vec![Some("I ANSWERED".into())],
        ))],
        12,
    );
    let id = QuarkId::new("agy");
    let before = engine.quarks.get(&id).unwrap().work.clone();

    engine.set_enabled(&id, false);
    assert!(!engine.is_enabled(&id));
    assert_eq!(engine.seated_count(), 1, "disabling must not unseat");
    assert!(
        Arc::ptr_eq(&before, &engine.quarks.get(&id).unwrap().work),
        "the instance was rebuilt by a mere disable — an ACP session would have died here"
    );
    assert!(engine.roster.iter().any(|c| c.id == id), "still on the roster, so @mentions still resolve");

    // Switched back on, it answers — and it is still the SAME quark, which is why
    // its scripted reply (consumed by nobody, because it never ran) is still queued.
    engine.set_enabled(&id, true);
    assert!(Arc::ptr_eq(&before, &engine.quarks.get(&id).unwrap().work));

    seed_human_message(&field, "agy", "you there?");
    engine.run_until_quiesce().await.unwrap();
    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "re-enabled, it must take its turn"
    );
}

// ---- exclusivity filter + routing-gap reporting (WS4 §4 Phase 2) -----------

/// A quark carrying `roles` and `exclusive`, for the exclusivity-filter tests
/// below. `roles()`/`exclusive()` override the `Quark` trait's defaults exactly
/// the way a real (e.g. ACP) seat built from `team.json` would.
struct RoledQuark {
    id: QuarkId,
    display_name: Option<String>,
    roles: Vec<String>,
    exclusive: bool,
    reply: String,
    deny_skills: Vec<String>,
}

#[async_trait::async_trait]
impl crate::quark::Quark for RoledQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
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
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
        Ok(TurnOutcome { message: Some(self.reply.clone()), permission: None, ..Default::default()})
    }
}

fn roled_quark(id: &str, roles: &[&str], exclusive: bool, reply: &str) -> RoledQuark {
    RoledQuark {
        id: QuarkId::new(id),
        display_name: None,
        roles: roles.iter().map(|r| r.to_string()).collect(),
        exclusive,
        reply: reply.into(),
        deny_skills: vec![],
    }
}

/// Same as [`roled_quark`], but carrying a `display_name` — the router resolves
/// `@DisplayName` mentions to the card via this (`match_longest_mention`'s
/// display-name pass), independent of `id`/`roles`, which is exactly what
/// `exclusive_seat_admitted_by_its_display_name` exercises.
fn roled_quark_named(id: &str, display_name: &str, roles: &[&str], exclusive: bool, reply: &str) -> RoledQuark {
    RoledQuark {
        id: QuarkId::new(id),
        display_name: Some(display_name.to_string()),
        roles: roles.iter().map(|r| r.to_string()).collect(),
        exclusive,
        reply: reply.into(),
        deny_skills: vec![],
    }
}

/// **The exclusivity property.** A card marked `exclusive` must never take a turn
/// it isn't scoped for — even when something *directly* addresses it (`to:
/// Some(id)`) for a task whose text never names its role or its id. This is the
/// gap a text-only router check can't close: `seed_human_message` here mirrors
/// exactly what a raw `Kind::Assign`/hand-off event can do — set `to` to any id,
/// completely independent of what the task text says — which is the "picked as a
/// general fallback worker" case the spec calls out.
#[tokio::test]
async fn exclusive_seat_excluded_from_non_matching_task() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark("sec", &["security"], true, "I ANSWERED"))],
        12,
    );
    // Directly addressed, but the task text names neither "@security" nor "@sec".
    seed_human_message(&field, "sec", "please fix the css typo on the landing page");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        !events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "an exclusive seat took a turn it was never scoped for"
    );
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("exclusive"))),
        "the field must SAY why the exclusive seat was skipped, not silently swallow the task"
    );
}

/// The same exclusive card IS eligible once the task actually names its role or
/// its id — exclusivity is a restriction, not a block on ever being reached.
#[tokio::test]
async fn exclusive_seat_eligible_for_matching_role_task() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark("sec", &["security"], true, "I ANSWERED"))],
        12,
    );
    seed_human_message(&field, "sec", "please review this @security issue");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "a role-matching task must still reach the exclusive seat"
    );
}

/// **The `@team` gap (review follow-up).** `human_mentions` expands `@team` to
/// EVERY roster card, so a plain broadcast — naming nobody in particular by role
/// or id — must not be read as "addressed to this exclusive card specifically".
/// Reproduces the review's exact finding: `@team status check` admitting an
/// exclusive `security` card with no role/id in sight.
#[tokio::test]
async fn exclusive_seat_excluded_from_a_team_broadcast() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark("sec", &["security"], true, "I ANSWERED"))],
        12,
    );
    // Unaddressed human message, `@team` only — no role, no id.
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@team status check".into() }),
    )
    .unwrap();
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        !events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "a `@team` broadcast admitted an exclusive seat it never named by role or id"
    );
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("exclusive"))),
        "the field must SAY why the exclusive seat was skipped, not silently swallow the broadcast"
    );
}

/// A `@team` broadcast that ALSO names the exclusive card's role still admits it —
/// exclusivity reads the whole task text, not just whether `@team` appears in it.
#[tokio::test]
async fn exclusive_seat_admitted_when_team_broadcast_also_names_its_role() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark("sec", &["security"], true, "I ANSWERED"))],
        12,
    );
    append_event(
        &field,
        &Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "@team we have a @security incident".into() },
        ),
    )
    .unwrap();
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "a broadcast that also names the exclusive seat's role must still reach it"
    );
}

/// **Whole-branch review follow-up.** `match_longest_mention` resolves
/// `@DisplayName` to a card (the `display_name` pass, independent of `id`/
/// `roles`), so an exclusive card WITH a display name must be admitted by its
/// own primary handle — not rejected as "did not address it by role or @id"
/// just because the exclusivity check forgot to look at `display_name`.
#[tokio::test]
async fn exclusive_seat_admitted_by_its_display_name() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark_named("acp-claude", "Claude", &["security"], true, "I ANSWERED"))],
        12,
    );
    seed_human_message(&field, "acp-claude", "@Claude handle this");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "an exclusive seat named by its own display name must be admitted"
    );
}

/// A display name must not widen the `@team` exclusion: broadcasting to
/// everyone still does not name this card specifically, display name or not.
#[tokio::test]
async fn exclusive_seat_with_display_name_still_excluded_from_a_team_broadcast() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark_named("acp-claude", "Claude", &["security"], true, "I ANSWERED"))],
        12,
    );
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@team status check".into() }),
    )
    .unwrap();
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        !events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "a `@team` broadcast must still exclude an exclusive seat even when it has a display name"
    );
}

/// A card carrying `roles` but NOT `exclusive` stays in general dispatch — the
/// filter only ever *removes* eligibility, never grants it, and it must not
/// over-fire on a seat that never opted into exclusivity.
#[tokio::test]
async fn non_exclusive_role_seat_always_eligible() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark("docs", &["documentation"], false, "I ANSWERED"))],
        12,
    );
    // Task text names neither its role nor its id — irrelevant, since it is not exclusive.
    seed_human_message(&field, "docs", "please fix the css typo on the landing page");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "a non-exclusive role seat must take any task, matching or not"
    );
}

// ---- broadcast scope (Projection.named_specifically) -----------------------

/// A worker directly named — by `@id` or display name — is `named_specifically`,
/// regardless of whether anyone else was also addressed. Reuses
/// `router::task_names_card_specifically` (the exact predicate the exclusive-seat
/// filter already relies on), not a second matcher.
#[tokio::test]
async fn projection_marks_a_specifically_named_worker() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark_named("sonnet", "Sonnet", &[], false, "reply"))],
        12,
    );
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@Sonnet please do X".into() }),
    )
    .unwrap();
    let events = read_events(&field).unwrap();
    let driver = engine.driver_for(&events, &QuarkId::new("sonnet"), None);
    let proj = engine.projection_for(&events, &QuarkId::new("sonnet"), driver.as_ref(), String::new(), None);
    assert!(proj.named_specifically, "a direct @Sonnet mention must be named specifically");
}

/// The gap this closes: `@team` expands to the whole roster (Jake's ruling — the
/// fan-out itself is non-negotiable), so a worker reached ONLY that way used to
/// get the identical "go implement this" prompt as one directly assigned, and a
/// `@team` brainstorm read as a work order. `named_specifically` is what the
/// prompt now keys the think-vs-do directive on.
#[tokio::test]
async fn projection_does_not_mark_a_team_broadcast_as_specifically_named() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark_named("sonnet", "Sonnet", &[], false, "reply"))],
        12,
    );
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@team let's brainstorm this".into() }),
    )
    .unwrap();
    let events = read_events(&field).unwrap();
    let driver = engine.driver_for(&events, &QuarkId::new("sonnet"), None);
    let proj = engine.projection_for(&events, &QuarkId::new("sonnet"), driver.as_ref(), String::new(), None);
    assert!(
        !proj.named_specifically,
        "a @team broadcast must not read as specifically naming this worker"
    );
}

/// **The deny_skills property.** A card carrying `deny_skills: vec!["writing-plans".into()]`
/// must never receive a task whose starting skill is `writing-plans`.
#[tokio::test]
async fn deny_skills_locks_out_matching_skill_task() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut q = roled_quark("sec", &["security"], false, "I ANSWERED");
    q.deny_skills = vec!["writing-plans".into()];
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(q)],
        12,
    );
    // Directly addressed, but the task matches the denied skill "writing-plans"
    seed_human_message(&field, "sec", "write a plan for authentication");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        !events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "a seat received a task whose starting skill it denied"
    );
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("locks out 'writing-plans' tasks"))),
        "the field must SAY why the seat was skipped (locks out 'writing-plans' tasks)"
    );
}

/// **The soft preference property.** A task whose skill maps to a role
/// prefers the role-holder first among candidate targets.
#[tokio::test]
async fn soft_preference_bubbles_role_holder_to_front() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");

    // Roster of two workers, the second carries the role "architect"
    let q1 = roled_quark("w1", &[], false, "W1 ANSWERED");
    let q2 = roled_quark("w2", &["architect"], false, "W2 ANSWERED");

    let engine = Engine::new(
        field.clone(),
        vec![Box::new(q1), Box::new(q2)],
        12,
    );

    // Broadcast target list: a "@team write a plan" message.
    // It matches the skill "writing-plans", which prefers the role "architect" (held by w2).
    let addressees = engine.human_addressees("@team write a plan for authentication");
    assert_eq!(addressees, vec![QuarkId::new("w2"), QuarkId::new("w1")]);
}

/// **Role prompt injection.** A turn dispatched to a seat holding role R,
/// for a task whose skill maps to R, carries the roles/R.md body.
#[tokio::test]
async fn role_prompt_injection_carries_role_body() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");

    // Write a roles directory structure
    let roles_dir = dir.path().join(".hadron").join("roles");
    std::fs::create_dir_all(&roles_dir).unwrap();
    std::fs::write(
        roles_dir.join("architect.md"),
        "---\nname: architect\n---\n\nYou design the system architecture.\n",
    ).unwrap();

    // Roster of two workers, the second carries the role "architect"
    let q1 = roled_quark("w1", &[], false, "W1 ANSWERED");
    let q2 = roled_quark("w2", &["architect"], false, "W2 ANSWERED");

    let engine = Engine::new(
        field.clone(),
        vec![Box::new(q1), Box::new(q2)],
        12,
    );

    let driver = Some(crate::engine::Driver {
        assignment: ulid::Ulid::new(),
        task: "write a plan for auth".into(),
        invariants: vec![],
    });

    // Get projection for w2 with a task that matches "writing-plans"
    let events = vec![Event::new(
        Actor::Human,
        Some(QuarkId::new("w2")),
        Kind::Message { body: "write a plan for auth".into() },
    )];
    let proj = engine.projection_for(&events, &QuarkId::new("w2"), driver.as_ref(), String::new(), None);
    assert_eq!(
        proj.role_body.as_deref(),
        Some("You design the system architecture.")
    );

    // A worker not holding the role does not get the body
    let proj_w1 = engine.projection_for(&events, &QuarkId::new("w1"), driver.as_ref(), String::new(), None);
    assert_eq!(proj_w1.role_body, None);
}

/// **Routing gap, reported not stalled.** A task needs a role; the only seat that
/// carries it is `exclusive` AND disabled. The router's role resolution (Phase 1)
/// still resolves the mention to that card (it only filters `Depleted`, not
/// disabled), so this rides the EXISTING `is_enabled`/`reroute_blocked` diagnostic
/// — reused, not reinvented — and the turn quiesces instead of hanging.
#[tokio::test]
async fn routing_gap_is_reported_not_stalled() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(roled_quark("sec", &["security"], true, "I ANSWERED"))],
        12,
    );
    engine.set_enabled(&QuarkId::new("sec"), false);

    // Unaddressed human message naming the ROLE, not the id — routed by Phase 1.
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@security please look at this".into() }),
    )
    .unwrap();

    tokio::time::timeout(Duration::from_secs(5), engine.run_until_quiesce())
        .await
        .expect("must quiesce, not hang, on an unreachable role")
        .unwrap();

    let events = read_events(&field).unwrap();
    assert!(
        !events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
        "a disabled exclusive seat must never take the turn"
    );
    // The `is_enabled` gate fires before the exclusive gate, so this lands on the
    // quiet path: the refusal is the `Blocked` status, not a chat message.
    assert!(
        events.iter().any(|e| matches!(e.kind, Kind::Status { state: QuarkState::Blocked })),
        "the routing gap must be recorded in the field, not silently dropped"
    );
}

/// Every event one turn emits carries the SAME turn id, so a reader can join a reply
/// to its own telemetry instead of guessing by adjacency. This is the whole reason
/// the field gained a `turn` — without it the chamber cannot honestly say what a
/// given reply cost.
#[tokio::test]
async fn one_turn_stamps_its_reply_and_its_energy_report_with_the_same_id() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let mut engine = Engine::new(field.clone(), vec![Box::new(SpendingQuark)], 12);
    seed_human_message(&field, "spender", "go");
    engine.run_until_quiesce().await.unwrap();

    let events = read_events(&field).unwrap();
    let reply = events
        .iter()
        .find(|e| matches!(&e.kind, Kind::Message { body } if body.contains("done")))
        .expect("the quark replied");
    let energy = events
        .iter()
        .find(|e| matches!(e.kind, Kind::EnergyReport { .. }))
        .expect("and reported its spend");

    let turn = reply.turn.expect("the reply names its turn");
    assert_eq!(energy.turn, Some(turn), "the energy report must name the SAME turn");
    assert_ne!(reply.id, energy.id, "two distinct events, one turn — that is the point");

    // And the join actually yields the components, which is what the chamber needs.
    let spend = energy.usage.as_ref().expect("telemetry rode along").spend.clone();
    assert_eq!(spend.fresh(), Some(30), "input+output for THIS reply");
    assert_eq!(spend.cached(), Some(900), "cache carried, not counted as work");
}

/// A quark that reports real components, so the turn-id join has something to join.
struct SpendingQuark;

#[async_trait::async_trait]
impl Quark for SpendingQuark {
    fn id(&self) -> QuarkId {
        QuarkId::new("spender")
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, _t: Projection) -> anyhow::Result<TurnOutcome> {
        Ok(TurnOutcome {
            message: Some("done".into()),
            permission: None,
            usage: hadron_lattice::Usage {
                spend: hadron_lattice::TokenSpend {
                    input: Some(10),
                    output: Some(20),
                    cache_read: Some(800),
                    cache_write: Some(100),
                },
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

/// A newly seated quark is addressable — the roster and the map agreed, so routing
/// can actually find it. Seating something the router cannot see is the bug this
/// whole change exists to fix, one layer down.
#[test]
fn a_newly_seated_quark_is_on_the_roster_the_router_reads() {
    let dir = tempdir().unwrap();
    let mut engine = engine_with(&["opus"], dir.path());
    engine.seat(Box::new(MockQuark::scripted(
        QuarkId::new("acp-claude"),
        Flavor::Worker,
        vec![None],
    )));

    let seated = QuarkId::new("acp-claude");
    assert!(engine.roster.iter().any(|c| c.id == seated), "not on the roster");
    assert!(engine.quarks.contains_key(&seated), "not in the quark map");
}

/// Replacing a seat (the human changed its model) swaps the instance — a changed
/// seat is a different agent and must NOT inherit the old one's session.
#[test]
fn replacing_a_seat_actually_swaps_the_instance() {
    let dir = tempdir().unwrap();
    let mut engine = engine_with(&["agy"], dir.path());
    let before = engine.quarks.get(&QuarkId::new("agy")).unwrap().work.clone();

    engine.seat(Box::new(MockQuark::scripted(
        QuarkId::new("agy"),
        Flavor::Worker,
        vec![None],
    )));

    assert_eq!(engine.seated_count(), 1, "a replacement must not duplicate the id");
    assert_eq!(
        engine.roster.iter().filter(|c| c.id == QuarkId::new("agy")).count(),
        1,
        "a replaced seat must not appear on the roster twice"
    );
    assert!(
        !Arc::ptr_eq(&before, &engine.quarks.get(&QuarkId::new("agy")).unwrap().work),
        "a changed seat kept its old instance — the old model would keep answering"
    );
}

#[test]
fn unseating_removes_from_both_the_map_and_the_roster() {
    let dir = tempdir().unwrap();
    let mut engine = engine_with(&["opus", "agy"], dir.path());

    assert!(engine.unseat(&QuarkId::new("agy")));
    assert!(!engine.unseat(&QuarkId::new("agy")), "unseating twice is not a lie");

    assert_eq!(engine.seated_count(), 1);
    assert!(
        !engine.roster.iter().any(|c| c.id == QuarkId::new("agy")),
        "unseated quark still on the roster — it would resolve to a turn we cannot run"
    );
}

/// The @Claude routing fix. A seat's display name must reach the router's roster card
/// so `@Claude` resolves to the seat whose id is `acp-claude` — instead of matching
/// nothing and falling through to the orchestrator. Exercises the real
/// Seat → registry → adapter → `Quark::display_name` → roster-card path, which is
/// exactly where the name used to be dropped (the card was hardcoded `display_name: None`).
#[test]
fn a_seats_display_name_reaches_the_router_so_at_mentions_resolve() {
    use crate::adapter::registry;
    use hadron_lattice::secrets::MemoryStore;
    use hadron_lattice::Seat;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    let store = MemoryStore::new();

    let mut claude = Seat::cli(QuarkId::new("acp-claude"), "claude", "opus", Flavor::Worker);
    claude.display_name = Some("Claude".into());
    // `claude` has no built-in CLI preset any more (Claude is ACP-only, per
    // spec's "ACP-only for Claude" decision) — this test is about display-name
    // routing, not CLI dispatch, so give the seat an explicit generic spec
    // rather than switching vendors and losing the `@Claude` intent.
    claude.cli = Some(hadron_lattice::CliSpec::generic("claude".into(), vec![]));
    let agy = Seat::cli(QuarkId::new("agy"), "agy", "", Flavor::Orchestrator);

    let engine = Engine::new(
        path,
        vec![
            registry::build_seat(&claude, &store).unwrap(),
            registry::build_seat(&agy, &store).unwrap(),
        ],
        10,
    );

    // The card now carries the name — the field that used to be hardcoded to None.
    assert_eq!(
        engine
            .roster
            .iter()
            .find(|c| c.id == QuarkId::new("acp-claude"))
            .and_then(|c| c.display_name.as_deref()),
        Some("Claude"),
        "the display name never reached the roster the router reads"
    );
    // @Claude resolves to the worker by name, not the orchestrator fallback.
    assert_eq!(
        engine.human_addressees("@Claude please fix it"),
        vec![QuarkId::new("acp-claude")]
    );
    // No regression: an unaddressed message still falls back to the orchestrator.
    assert_eq!(engine.human_addressees("just a thought"), vec![QuarkId::new("agy")]);
}

/// Role routing's make-or-break wiring. A seat's `roles`/`exclusive` must reach the
/// roster card the same way `display_name` does — carried on the live quark, not
/// populated by a daemon-side step that does not exist (see
/// `docs/superpowers/specs/2026-07-18-role-routing-design.md` §2.1). Exercises the
/// real Seat → registry → adapter → `Quark::roles`/`Quark::exclusive` →
/// roster-card path.
#[test]
fn roster_card_carries_the_quarks_roles() {
    use crate::adapter::registry;
    use hadron_lattice::secrets::MemoryStore;
    use hadron_lattice::Seat;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");

    let mut security = Seat::cli(QuarkId::new("security-quark"), "agy", "", Flavor::Worker);
    security.roles = vec!["security".to_string()];
    security.exclusive = true;

    let engine = Engine::new(path, vec![registry::build_seat(&security, &MemoryStore::new()).unwrap()], 10);

    let card = engine
        .roster
        .iter()
        .find(|c| c.id == QuarkId::new("security-quark"))
        .expect("the seat is on the roster");
    assert_eq!(card.roles, vec!["security".to_string()], "roles never reached the roster card");
    assert!(card.exclusive, "exclusive never reached the roster card");
}

// ---- force-restart (Kind::Reboot) ------------------------------------------
//
// The human's manual "Restart" reaps a wedged quark's resident session. Idle →
// just reset it; mid-turn → abort the turn, ground it, reset it, and leave every
// sibling running. A reboot that predates this daemon (no live session to kill) is
// stale-ignored by the baseline. Servicing keys on the reboot event's *id* (a set of
// serviced ids), not a position, so a `/clear` truncation cannot hide a fresh reboot.

/// A quark that records whether its session was reset — the observable of a reboot.
struct ResettableQuark {
    id: QuarkId,
    was_reset: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl crate::quark::Quark for ResettableQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, _t: Projection) -> anyhow::Result<TurnOutcome> {
        Ok(TurnOutcome { message: Some("ok".into()), permission: None, ..Default::default()})
    }
    fn reset_session(&mut self) {
        self.was_reset.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A slow quark that, the instant it is excited, appends the human's force-restart
/// to the field (mirroring a mid-turn Restart click) and then grinds. Its reply
/// must never land — the reboot aborts it first.
struct RebootingSlowQuark {
    id: QuarkId,
    field: PathBuf,
    was_reset: Arc<std::sync::atomic::AtomicBool>,
    hold: Duration,
}

#[async_trait::async_trait]
impl crate::quark::Quark for RebootingSlowQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        Flavor::Worker
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, _t: Projection) -> anyhow::Result<TurnOutcome> {
        append_event(
            &self.field,
            &Event::new(Actor::Human, Some(self.id.clone()), Kind::Reboot),
        )
        .unwrap();
        tokio::time::sleep(self.hold).await;
        // Only reached if the abort never happened — the assertion catches it.
        Ok(TurnOutcome { message: Some("SLOW DONE".into()), permission: None, ..Default::default()})
    }
    fn reset_session(&mut self) {
        self.was_reset.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The watermark and the idle path in one: a reboot appended *before* the first
/// read is history and never fires; one appended *after* resets the idle quark.
#[tokio::test]
async fn a_reboot_before_the_baseline_is_ignored_but_one_after_it_resets_the_idle_quark() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let was_reset = Arc::new(AtomicBool::new(false));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(ResettableQuark { id: QuarkId::new("q"), was_reset: was_reset.clone() })],
        10,
    );
    let mut in_flight: HashSet<(QuarkId, Lane)> = HashSet::new();
    let mut handles: HashMap<(QuarkId, Lane), AbortHandle> = HashMap::new();

    // Pre-boot reboot: swallowed by the baseline.
    append_event(&field, &Event::new(Actor::Human, Some(QuarkId::new("q")), Kind::Reboot)).unwrap();
    let events = read_events(&field).unwrap();
    engine.service_reboots(&events, &mut in_flight, &mut handles).await.unwrap();
    assert!(
        !was_reset.load(Ordering::SeqCst),
        "a reboot that predates the daemon must be stale-ignored, not serviced"
    );

    // A reboot past the watermark resets the idle quark's session.
    append_event(&field, &Event::new(Actor::Human, Some(QuarkId::new("q")), Kind::Reboot)).unwrap();
    let events = read_events(&field).unwrap();
    engine.service_reboots(&events, &mut in_flight, &mut handles).await.unwrap();
    assert!(
        was_reset.load(Ordering::SeqCst),
        "a reboot past the watermark must reset the idle quark's session"
    );
}

/// `/clear` truncates the field and appends a fresh reboot per quark. The reboot
/// that told this quark to restart must still fire even though every event the
/// baseline recorded was archived out from under the engine — the identity set, not a
/// file position, is what makes that hold. (A positional watermark, its marker gone
/// with the truncation, would re-baseline and service nothing.)
#[tokio::test]
async fn a_reboot_appended_after_a_clear_truncation_still_resets_the_quark() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let was_reset = Arc::new(AtomicBool::new(false));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(ResettableQuark { id: QuarkId::new("q"), was_reset: was_reset.clone() })],
        10,
    );
    let mut in_flight: HashSet<(QuarkId, Lane)> = HashSet::new();
    let mut handles: HashMap<(QuarkId, Lane), AbortHandle> = HashMap::new();

    // Some pre-clear history, then baseline over it (services nothing).
    append_event(&field, &Event::new(Actor::Human, Some(QuarkId::new("q")), Kind::Reboot)).unwrap();
    append_event(&field, &Event::new(Actor::Human, None, Kind::Message { body: "hi".into() })).unwrap();
    let events = read_events(&field).unwrap();
    engine.service_reboots(&events, &mut in_flight, &mut handles).await.unwrap();
    assert!(!was_reset.load(Ordering::SeqCst), "baseline must not service history");

    // `/clear`: the live field is truncated to empty, then a fresh reboot lands (the
    // one `/clear` appends to restart every seated quark).
    std::fs::write(&field, "").unwrap();
    append_event(&field, &Event::new(Actor::Human, Some(QuarkId::new("q")), Kind::Reboot)).unwrap();
    let events = read_events(&field).unwrap();
    engine.service_reboots(&events, &mut in_flight, &mut handles).await.unwrap();
    assert!(
        was_reset.load(Ordering::SeqCst),
        "a post-/clear reboot must reset the quark even though the baseline events were truncated away"
    );
}

/// End-to-end guard for the "`/clear` triggers codex" bug. After `/clear` the field
/// holds only reboots — one per resident quark — and NONE may read as a pending turn.
/// `pending_targets` is what the dispatch loop spawns from, so an empty result here is
/// the proof that a reboot excites nobody (it is a restart, serviced separately). The
/// last-addressed reboot used to come back as pending, handing that quark an empty turn.
#[test]
fn post_clear_reboots_are_not_pending_turns() {
    use std::sync::atomic::AtomicBool;
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    let engine = Engine::new(
        field,
        vec![
            Box::new(ResettableQuark {
                id: QuarkId::new("a"),
                was_reset: Arc::new(AtomicBool::new(false)),
            }),
            Box::new(ResettableQuark {
                id: QuarkId::new("codex"),
                was_reset: Arc::new(AtomicBool::new(false)),
            }),
        ],
        10,
    );
    // Exactly what `/clear` leaves in the (truncated) field: a reboot per quark.
    let events = vec![
        Event::new(Actor::Human, Some(QuarkId::new("a")), Kind::Reboot),
        Event::new(Actor::Human, Some(QuarkId::new("codex")), Kind::Reboot),
    ];
    assert!(
        engine.pending_targets(&events).is_empty(),
        "post-/clear reboots must excite nobody, got: {:?}",
        engine.pending_targets(&events),
    );
}

/// **The discriminating test.** A mid-turn reboot must abort *only* the target's
/// turn — killing its reply, grounding it, resetting its session — and leave a
/// sibling's turn to finish. The sibling assertion is what proves the aborted
/// task's cancelled `JoinError` did not trip the ground-everyone panic path.
#[tokio::test]
async fn a_mid_turn_reboot_aborts_only_that_quark_and_spares_the_sibling() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    // One unaddressed message naming both, so the concurrent dispatch path fans it
    // out and excites both on the same pass (addressed `to=Some` messages go through
    // next_pending, which yields a single target and would not overlap them).
    append_event(
        &field,
        &Event::new(
            Actor::Human,
            None,
            Kind::Message { body: "@slow grind on this and @fast do a quick task".into() },
        ),
    )
    .unwrap();

    let was_reset = Arc::new(AtomicBool::new(false));
    let slow = RebootingSlowQuark {
        id: QuarkId::new("slow"),
        field: field.clone(),
        was_reset: was_reset.clone(),
        hold: Duration::from_secs(30),
    };
    let fast = MockQuark::scripted(QuarkId::new("fast"), Flavor::Worker, vec![Some("FAST DONE".into())]);

    let mut engine =
        Engine::new(field.clone(), vec![Box::new(slow), Box::new(fast)], 20);

    tokio::time::timeout(Duration::from_secs(10), engine.run_until_quiesce())
        .await
        .expect("engine hung — the mid-turn reboot never aborted the slow turn")
        .expect("run_until_quiesce returned an error");

    let events = read_events(&field).unwrap();
    // 1. The slow turn was killed — its reply never reached the field.
    assert!(
        !events
            .iter()
            .any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("SLOW DONE"))),
        "the slow turn completed — it was NOT aborted"
    );
    // 2. It was grounded and its session reset.
    assert!(
        events.iter().any(|e| e.from == Actor::Quark(QuarkId::new("slow"))
            && matches!(e.kind, Kind::Status { state: QuarkState::Ground })),
        "the rebooted quark never got a terminal Ground"
    );
    assert!(
        was_reset.load(Ordering::SeqCst),
        "reset_session was never called on the rebooted quark"
    );
    // 3. The sibling was spared — its turn completed normally.
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("FAST DONE"))),
        "the sibling's turn was lost — the reboot tripped the ground-everyone panic path"
    );
}

#[tokio::test]
async fn gluon_notification_to_orchestrator_resolves_gluon_driver_and_does_not_loop() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");

    // 1. Human message (older)
    append_event(
        &path,
        &Event::new(Actor::Human, None, Kind::Message { body: "initial human request".into() }),
    )
    .unwrap();

    // 2. Worker fails and Gluon posts error message to @orchestrator
    append_event(
        &path,
        &Event::new(Actor::Gluon, None, Kind::Message { body: "@orchestrator Quark worker turn errored: timeout".into() }),
    )
    .unwrap();

    struct MockOrch {
        id: QuarkId,
        turns: Arc<Mutex<Vec<String>>>,
    }
    #[async_trait::async_trait]
    impl crate::quark::Quark for MockOrch {
        fn id(&self) -> QuarkId { self.id.clone() }
        fn flavor(&self) -> Flavor { Flavor::Orchestrator }
        fn energy(&self) -> EnergyState { EnergyState::Available }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            self.turns.lock().unwrap().push(turn.task);
            Ok(TurnOutcome { message: Some("handled error notification".into()), permission: None, ..Default::default()})
        }
    }

    let turns = Arc::new(Mutex::new(vec![]));
    let orch = MockOrch { id: QuarkId::new("orch"), turns: turns.clone() };

    let mut engine = Engine::new(path.clone(), vec![Box::new(orch)], 10);

    // Pass 1: Should excite orchestrator ONCE to handle the Gluon error notification
    engine.run_until_quiesce().await.unwrap();

    let recorded = turns.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1, "orchestrator should take exactly 1 turn for the Gluon notification");

    // Pass 2: Engine should quiesce immediately (0 additional turns)
    engine.run_until_quiesce().await.unwrap();
    assert_eq!(turns.lock().unwrap().len(), 1, "orchestrator must not loop on handled Gluon notification");
}


/// The turn-end `/learn` nudge fires off this heuristic. It is a substring check on the
/// turn's own transcript, so the markers must be the ones a red run actually prints —
/// and a green run must not trip it, or the reminder becomes noise nobody reads.
#[tokio::test]
async fn looks_like_a_debugging_turn_catches_the_known_failure_markers() {
    use super::merge::looks_like_a_debugging_turn as debugged;
    assert!(debugged("thread 'main' panicked at foo.rs:10"));
    assert!(debugged("test worktree::tests::a_held_lock ... FAILED"));
    assert!(debugged("error[E0599]: no method named `foo`"));
    assert!(!debugged("test result: ok. 40 passed; 0 failed"));
    assert!(!debugged("merged `quark/acp-claude/01K` → `main` (fast-forward)."));
}

/// **Bug C.** The human names who they want in one message and says what they want in
/// the next. `unaddressed_message_targets` walks newest-first and hands each seat its
/// most-recent unserved mention, so the workers were dispatched on the *older* naming
/// message while the orchestrator — claimed by the newer, mention-less message via
/// default routing — was the only seat that ever saw the actual ask.
///
/// Mentions decide **who**; the human's latest unaddressed message decides **what**.
#[tokio::test]
async fn a_named_worker_is_dispatched_on_the_humans_latest_message_not_the_naming_one() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    for body in ["@claude @agy", "here is the actual six-item ask"] {
        append_event(&path, &Event::new(Actor::Human, None, Kind::Message { body: body.into() }))
            .unwrap();
    }

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let claude = targets.iter().find(|(q, _)| q.as_str() == "claude").expect("claude pending");
    assert!(
        claude.1.task.contains("six-item ask"),
        "worker was dispatched on the naming message, not the ask: {:?}",
        claude.1.task
    );
    // …but WHO named it is preserved, because the exclusive-seat gate reads that text.
    assert!(claude.1.addressing.contains("@claude"), "the naming text was lost: {:?}", claude.1.addressing);
}

/// The human's caveat: `@mention prompt` keeps working exactly as before. When the
/// naming message IS the newest human message there is nothing newer to substitute,
/// so the body stays its own task — no heuristic needed, it falls out of the rule.
#[tokio::test]
async fn a_mention_that_carries_its_own_prompt_is_unchanged() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    append_event(&path, &Event::new(Actor::Human, None, Kind::Message { body: "@claude do X".into() }))
        .unwrap();

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let claude = targets.iter().find(|(q, _)| q.as_str() == "claude").expect("claude pending");
    assert_eq!(claude.1.task, "@claude do X");
    assert_eq!(claude.1.addressing, "@claude do X");
}

/// A quark→quark delegation keeps its OWN body. Substitution is a human-input rule:
/// a worker told "@peer rebase onto main first" must not have that replaced by
/// whatever the human happened to type last, which names nobody and means nothing
/// to the peer.
#[tokio::test]
async fn a_quark_delegation_is_never_rewritten_by_the_humans_latest_message() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::Message { body: "@claude rebase onto main first".into() },
        ),
    )
    .unwrap();
    append_event(&path, &Event::new(Actor::Human, None, Kind::Message { body: "thanks".into() }))
        .unwrap();

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let claude = targets.iter().find(|(q, _)| q.as_str() == "claude").expect("claude pending");
    assert_eq!(claude.1.task, "@claude rebase onto main first");
}

/// The coupling the substitution could have broken: `has_answered` keys on the id of
/// the message that NAMED the quark, and the turn stamps `answers` with
/// `driver_for`'s assignment — which resolves the same naming message. If those two
/// ever disagreed, a served turn would either be dispatched forever or drop the work
/// silently. Serve the turn and assert the next pass is empty.
#[tokio::test]
async fn a_substituted_task_is_still_marked_served_after_one_turn() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    let naming = Event::new(Actor::Human, None, Kind::Message { body: "@claude".into() });
    let naming_id = naming.id;
    append_event(&path, &naming).unwrap();
    append_event(&path, &Event::new(Actor::Human, None, Kind::Message { body: "the real ask".into() }))
        .unwrap();

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    // The turn is dispatched against `driver_for`'s assignment — that is the ULID
    // stamped into `answers`, so this is the real id, not a guess.
    let driver = engine
        .driver_for(&events, &QuarkId::new("claude"), Some("the real ask"))
        .expect("a driver for the substituted task");
    assert_eq!(driver.assignment, naming_id, "the assignment must be the message that NAMED claude");
    assert_eq!(driver.task, "the real ask", "the task must be the human's latest message");

    append_event(
        &path,
        &Event::new(Actor::Quark(QuarkId::new("claude")), None, Kind::Message { body: "done".into() })
            .answering(Some(driver.assignment)),
    )
    .unwrap();

    let events = read_events(&path).unwrap();
    let served = engine.unaddressed_message_targets(&events);
    let ids: Vec<&str> = served.iter().map(|(q, _)| q.as_str()).collect();
    assert!(!ids.contains(&"claude"), "the served turn was re-dispatched: {ids:?}");
}

/// The over-reach the first cut of Bug C introduced, caught by the existing burst-bug
/// fixture: `"@agy do Y"` then `"@claude do Z"` are two DIFFERENT tasks for two seats.
/// Substituting "the human's latest message" unconditionally handed agy an instruction
/// addressed to claude. A message that names seats steers only those seats; only a
/// mention-less message can stand in for someone else's task.
#[tokio::test]
async fn a_newer_message_naming_another_seat_does_not_steal_this_seats_task() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    for body in ["@claude do Y", "@agy do Z"] {
        append_event(&path, &Event::new(Actor::Human, None, Kind::Message { body: body.into() }))
            .unwrap();
    }

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let claude = targets.iter().find(|(q, _)| q.as_str() == "claude").expect("claude pending");
    assert_eq!(claude.1.task, "@claude do Y", "claude was handed agy's instruction");
}

/// The silent drop the first cut of Bug C would have shipped. `"@claude fix the router"`
/// then `"thanks"` is the commonest two-message shape there is, and substituting the
/// newer body hands claude `"thanks"` with the instruction gone — not stale, destroyed.
/// A correction (`"actually do the README first"`) and an aside (`"thanks"`) are
/// indistinguishable, so the newer body is APPENDED and both readings stay true.
#[tokio::test]
async fn a_mention_less_follow_up_is_appended_and_never_erases_the_instruction() {
    use crate::mock::MockQuark;
    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    for body in ["@claude fix the router", "thanks"] {
        append_event(&path, &Event::new(Actor::Human, None, Kind::Message { body: body.into() }))
            .unwrap();
    }

    let engine = Engine::new(
        path.clone(),
        vec![
            Box::new(MockQuark::repeating(QuarkId::new("claude"), Flavor::Worker, "ok")),
            Box::new(MockQuark::repeating(QuarkId::new("agy"), Flavor::Orchestrator, "ok")),
        ],
        10,
    );

    let events = read_events(&path).unwrap();
    let targets = engine.unaddressed_message_targets(&events);
    let claude = targets.iter().find(|(q, _)| q.as_str() == "claude").expect("claude pending");
    assert!(
        claude.1.task.contains("fix the router"),
        "the instruction was destroyed by the follow-up: {:?}",
        claude.1.task
    );
    assert!(claude.1.task.contains("thanks"), "the follow-up was dropped: {:?}", claude.1.task);
}

/// A dispatch leaves a RECORD in the field, not just a side effect.
///
/// Before this, a task's brief and its result were two unrelated `Message`s with
/// nothing linking them: the engine re-derived "who was asked to do what" from the
/// message text on every pass (`unaddressed_message_targets`) and wrote nothing down.
/// `Kind::Assign` already existed, was already an `is_turn_request`, and was
/// constructed nowhere (`the-assign-invariants-seam-is-dead`). The engine now appends
/// one per assignment, addressed to the quark it dispatched, carrying the resolved
/// task — which is what the chamber's task feed reads as a title (`model/tasks.rs`).
///
/// It is written by `Actor::Gluon` with `answers` set to the assignment it records, so
/// `continued_assignment` recognises it as a CONTINUATION: `driver_for` finds it as the
/// most recent task-bearing event addressed to the quark and must keep the original
/// assignment ULID, or the next turn would cut a fresh branch and strand this one.
#[tokio::test]
async fn a_dispatch_writes_one_assign_record_per_assignment() {
    let dir = tempdir().unwrap();
    let field = dir.path().join("field.jsonl");
    seed_human_message(&field, "agy", "hello");
    let driving_id = read_events(&field).unwrap()[0].id;

    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(MockQuark::scripted(
            QuarkId::new("agy"),
            Flavor::Worker,
            vec![Some("done".into())],
        ))],
        8,
    );
    engine.run_until_quiesce().await.unwrap();

    let assigns: Vec<Event> = read_events(&field)
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e.kind, Kind::Assign { .. }))
        .collect();
    assert_eq!(assigns.len(), 1, "exactly one Assign per assignment: {assigns:?}");
    let a = &assigns[0];
    assert_eq!(a.from, Actor::Gluon, "the record is the engine's, not the quark's");
    assert_eq!(a.to.as_ref(), Some(&QuarkId::new("agy")), "addressed to the dispatched quark");
    assert_eq!(a.answers, Some(driving_id), "answers the assignment, so it continues it");
    match &a.kind {
        Kind::Assign { task, .. } => assert_eq!(task, "hello", "carries the resolved task"),
        other => panic!("not an Assign: {other:?}"),
    }

    // A second quiesce with nothing new must not write a second record for the same
    // assignment — the Assign is itself an `is_turn_request`, so a duplicate per pass
    // would be a dispatch record that re-dispatches.
    engine.run_until_quiesce().await.unwrap();
    let after = read_events(&field)
        .unwrap()
        .iter()
        .filter(|e| matches!(e.kind, Kind::Assign { .. }))
        .count();
    assert_eq!(after, 1, "no second record for the same assignment");
}

/// **Task 4, Step 1 of the responsive-orchestrator plan — the regression
/// guard.** A quark whose transport never fills its [`crate::quark::CancelSlot`]
/// (every CLI seat, and any ACP quark that never overrides
/// [`crate::quark::Quark::attach_cancel_slot`]) is unaffected by Task 4: a
/// second message arriving while it is in flight is still simply queued
/// behind the running turn, exactly as before that task existed. Proven by
/// timing, not by inspecting engine internals: if the first turn were cut
/// short by a phantom cancel, the two sequential holds below would not both
/// run to completion and total elapsed time would fall well short of `2 *
/// hold`.
#[tokio::test]
async fn a_busy_quark_with_no_cancel_slot_is_not_interrupted() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct SlowQuark {
        id: QuarkId,
        excite_count: Arc<AtomicUsize>,
        hold: Duration,
    }

    #[async_trait::async_trait]
    impl crate::quark::Quark for SlowQuark {
        fn id(&self) -> QuarkId {
            self.id.clone()
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        // `attach_cancel_slot` deliberately NOT overridden — this quark relies
        // on the trait's default no-op, so the engine's slot for it is never
        // filled. That is the whole point of this test.
        async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
            self.excite_count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.hold).await;
            Ok(TurnOutcome { message: Some("done".into()), permission: None, ..Default::default()})
        }
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    let hold = Duration::from_millis(300);
    let excite_count = Arc::new(AtomicUsize::new(0));

    append_event(
        &path,
        &Event::new(
            Actor::Quark(QuarkId::new("reporter")),
            Some(QuarkId::new("seat")),
            Kind::Message { body: "first task".into() },
        ),
    )
    .unwrap();

    let mid_flight = {
        let path = path.clone();
        let excite_count = excite_count.clone();
        tokio::spawn(async move {
            for _ in 0..200 {
                if excite_count.load(Ordering::SeqCst) >= 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            seed_unaddressed_mention(&path, "seat", "second, urgent task");
        })
    };

    let mut engine = Engine::new(
        path.clone(),
        vec![Box::new(SlowQuark { id: QuarkId::new("seat"), excite_count: excite_count.clone(), hold })],
        10,
    );

    let started = std::time::Instant::now();
    engine.run_until_quiesce().await.unwrap();
    mid_flight.await.unwrap();

    assert_eq!(excite_count.load(Ordering::SeqCst), 2, "both messages must have run their own full turn");
    assert!(
        started.elapsed() >= 2 * hold - Duration::from_millis(50),
        "elapsed {:?} is well under 2x the hold — the first turn was cut short, \
         meaning something interrupted a quark whose cancel slot was never filled",
        started.elapsed()
    );
}

/// **Task 4, Step 2 of the responsive-orchestrator plan.** A quark whose
/// transport DOES fill its [`crate::quark::CancelSlot`] (the ACP shape, via
/// [`crate::quark::Quark::attach_cancel_slot`]) is interrupted rather than
/// queued: a message arriving while it is mid-turn requests exactly one
/// graceful cancel, the interrupted turn resolves (short of its full hold),
/// and the new message gets its own turn once the cancel lands.
#[tokio::test]
async fn a_busy_quark_with_a_cancel_slot_is_interrupted_by_a_new_message() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct CancellableQuark {
        id: QuarkId,
        cancel_count: Arc<AtomicUsize>,
        cancelled: Arc<AtomicBool>,
        excite_count: Arc<AtomicUsize>,
        hold: Duration,
    }

    #[async_trait::async_trait]
    impl crate::quark::Quark for CancellableQuark {
        fn id(&self) -> QuarkId {
            self.id.clone()
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        fn attach_cancel_slot(&mut self, slot: crate::quark::CancelSlot) {
            let cancel_count = self.cancel_count.clone();
            let cancelled = self.cancelled.clone();
            slot.set(Some(Arc::new(move || {
                cancel_count.fetch_add(1, Ordering::SeqCst);
                cancelled.store(true, Ordering::SeqCst);
                true
            })));
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            self.excite_count.fetch_add(1, Ordering::SeqCst);
            self.cancelled.store(false, Ordering::SeqCst);
            let deadline = tokio::time::Instant::now() + self.hold;
            loop {
                if self.cancelled.load(Ordering::SeqCst) {
                    return Ok(TurnOutcome { message: None, permission: None, ..Default::default()});
                }
                if tokio::time::Instant::now() >= deadline {
                    return Ok(TurnOutcome {
                        message: Some(format!("done: {}", turn.task)),
                        permission: None,
                        ..Default::default()
                    });
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    let hold = Duration::from_millis(1500);

    append_event(
        &path,
        &Event::new(
            Actor::Quark(QuarkId::new("reporter")),
            Some(QuarkId::new("seat")),
            Kind::Message { body: "first task".into() },
        ),
    )
    .unwrap();

    let cancel_count = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(AtomicBool::new(false));
    let excite_count = Arc::new(AtomicUsize::new(0));

    let mid_flight = {
        let path = path.clone();
        let excite_count = excite_count.clone();
        tokio::spawn(async move {
            for _ in 0..200 {
                if excite_count.load(Ordering::SeqCst) >= 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            // Give the first turn a moment to be genuinely mid-flight, not
            // caught at the very instant it started.
            tokio::time::sleep(Duration::from_millis(50)).await;
            seed_unaddressed_mention(&path, "seat", "second, urgent task");
        })
    };

    let mut engine = Engine::new(
        path.clone(),
        vec![Box::new(CancellableQuark {
            id: QuarkId::new("seat"),
            cancel_count: cancel_count.clone(),
            cancelled: cancelled.clone(),
            excite_count: excite_count.clone(),
            hold,
        })],
        10,
    );

    let started = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(8), engine.run_until_quiesce())
        .await
        .expect("the busy turn should have been interrupted well within 8s, not run its full hold twice")
        .unwrap();
    mid_flight.await.unwrap();

    assert_eq!(cancel_count.load(Ordering::SeqCst), 1, "exactly one cancel request for the interrupted turn");
    assert!(
        started.elapsed() < hold + hold / 2,
        "elapsed {:?} suggests the first turn ran to completion instead of being cancelled",
        started.elapsed()
    );

    let events = read_events(&path).unwrap();
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("done: @seat second, urgent task"))),
        "the second message must have been re-dispatched and completed after the cancel resolved: {events:?}"
    );
}

/// **Task 5 of the responsive-orchestrator plan.** A cancel fired against a
/// quark whose worktree has uncommitted edits must not leave those edits
/// stranded — a dirty tree blocks the merge gate with
/// `BlockReason::DirtyWorktree` forever, since nothing else in the dispatch
/// loop ever commits on the quark's behalf (nucleus `uncommitted-work-dies`).
///
/// **Why the assertion is the commit's ORDERING, not just its existence.**
/// `finish_turn`'s own `commit_turn` call (`engine/turn.rs:299`) already
/// commits whatever is dirty once a turn resolves — including a gracefully
/// cancelled one, whose `outcome.message` is `None`. So "is the tree clean
/// after the interrupt" is true even on `main` *before* this task, purely
/// because that fallback commit runs, and a test that only checked
/// `is_dirty()` would never go red. What Task 5 actually changes is WHEN the
/// commit happens: the plan calls for it **before** the cancel is requested,
/// so this test has the cancel closure itself check whether the tree was
/// already clean at the moment it fires — a fact only true once the engine
/// commits ahead of `request_cancel`, not after.
#[tokio::test]
async fn an_interrupted_turn_commits_its_dirty_worktree_before_the_cancel_is_requested() {
    use std::sync::atomic::{AtomicBool, Ordering};

    struct WritesThenCancellableQuark {
        id: QuarkId,
        cwd: Arc<Mutex<Option<PathBuf>>>,
        cancelled: Arc<AtomicBool>,
        tree_was_clean_when_cancel_requested: Arc<AtomicBool>,
        interrupted_branch: Arc<Mutex<Option<String>>>,
        excite_count: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl crate::quark::Quark for WritesThenCancellableQuark {
        fn id(&self) -> QuarkId {
            self.id.clone()
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        fn attach_cancel_slot(&mut self, slot: crate::quark::CancelSlot) {
            let cancelled = self.cancelled.clone();
            let cwd = self.cwd.clone();
            let tree_was_clean = self.tree_was_clean_when_cancel_requested.clone();
            let interrupted_branch = self.interrupted_branch.clone();
            slot.set(Some(Arc::new(move || {
                if let Some(path) = cwd.lock().unwrap().clone() {
                    if let Ok(dirty) = crate::worktree::is_dirty(&path) {
                        tree_was_clean.store(!dirty, Ordering::SeqCst);
                    }
                    *interrupted_branch.lock().unwrap() = crate::worktree::current_branch(&path);
                }
                cancelled.store(true, Ordering::SeqCst);
                true
            })));
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            *self.cwd.lock().unwrap() = Some(turn.cwd.clone());
            // Only the FIRST call is the long, interruptible one — a resumed
            // turn (dispatched after the cancel) must finish quickly so the
            // test does not wait out a second full hold with nothing left to
            // interrupt it.
            let n = self.excite_count.fetch_add(1, Ordering::SeqCst) + 1;
            if n > 1 {
                return Ok(TurnOutcome {
                    message: Some(format!("resumed, diff has wip.txt: {}", turn.git_diff.contains("wip.txt"))),
                    permission: None,
                    ..Default::default()
                });
            }
            std::fs::write(turn.cwd.join("wip.txt"), "half-finished work\n")?;
            self.cancelled.store(false, Ordering::SeqCst);
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                if self.cancelled.load(Ordering::SeqCst) {
                    return Ok(TurnOutcome { message: None, permission: None, ..Default::default()});
                }
                if tokio::time::Instant::now() >= deadline {
                    return Ok(TurnOutcome {
                        message: Some("finished uninterrupted".into()),
                        permission: None,
                        ..Default::default()
                    });
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }

    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    append_event(
        &field,
        &Event::new(
            Actor::Quark(QuarkId::new("reporter")),
            Some(QuarkId::new("seat")),
            Kind::Message { body: "first task".into() },
        ),
    )
    .unwrap();

    let cwd = Arc::new(Mutex::new(None));
    let tree_was_clean_when_cancel_requested = Arc::new(AtomicBool::new(false));

    let mid_flight = {
        let field = field.clone();
        let cwd = cwd.clone();
        tokio::spawn(async move {
            for _ in 0..400 {
                if cwd.lock().unwrap().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            // Give `excite` a moment past writing the file before interrupting.
            tokio::time::sleep(Duration::from_millis(30)).await;
            seed_unaddressed_mention(&field, "seat", "second, urgent task");
        })
    };

    let interrupted_branch = Arc::new(Mutex::new(None));
    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(WritesThenCancellableQuark {
            id: QuarkId::new("seat"),
            cwd: cwd.clone(),
            cancelled: Arc::new(AtomicBool::new(false)),
            tree_was_clean_when_cancel_requested: tree_was_clean_when_cancel_requested.clone(),
            interrupted_branch: interrupted_branch.clone(),
            excite_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })],
        10,
    )
    .with_git(root.clone());

    tokio::time::timeout(Duration::from_secs(8), engine.run_until_quiesce())
        .await
        .expect("the interrupted turn should resolve well within 8s")
        .unwrap();
    mid_flight.await.unwrap();

    assert!(
        tree_was_clean_when_cancel_requested.load(Ordering::SeqCst),
        "the worktree was still dirty at the moment the cancel was requested — the WIP \
         snapshot must be committed BEFORE `request_cancel`, not left to `finish_turn`'s \
         own fallback commit after the turn resolves"
    );

    let trees = crate::worktree::list(&root).unwrap();
    let wt = trees.iter().find(|w| w.quark == QuarkId::new("seat")).expect("seat's worktree must still exist");
    assert!(!crate::worktree::is_dirty(&wt.path).unwrap(), "the worktree must end up clean");

    // Step 3: the snapshot is durably reachable, not merely uncommitted-then-
    // discarded. A second, unrelated message drives a genuinely NEW
    // assignment (its own ULID, its own branch — correct: "second, urgent
    // task" is not a continuation of "first task"), so the interrupted
    // branch is no longer the worktree's checked-out one by the time this
    // runs. That is a separate question from Task 5's own scope (does an
    // interrupt ever strand dirty work) — the answer here is no: the commit
    // survives as a real ref, independently of what the next dispatch does.
    let old_branch = interrupted_branch.lock().unwrap().clone().expect("cancel must have observed a branch");
    let out = std::process::Command::new("git")
        .args(["-C", root.to_str().unwrap(), "show", &format!("{old_branch}:wip.txt")])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "the WIP snapshot on the interrupted branch `{old_branch}` must remain reachable: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// **Task 9, Defect 1.** A turn that resolves with `TurnOutcome.cancelled == true`
/// must never reach the merge gate — even though its `message` is `None`, which
/// is the exact shape `assignment_complete` reads as "hands back to the human,
/// done". Before the fix, an interrupted turn's dirty-but-now-committed branch
/// (Task 5) would be gated, tested, and — with a green suite — fast-forwarded
/// onto `base`, landing work whose own content was discarded by the cancel
/// (Task 2's probe). Asserted on the gate never running (`OrderRecordingRunner`
/// stays empty) and the branch never landing (`base` HEAD unmoved), not on a
/// log line.
#[tokio::test]
async fn a_cancelled_turn_never_reaches_the_merge_gate() {
    struct CancelledOutcomeQuark {
        id: QuarkId,
    }

    #[async_trait::async_trait]
    impl crate::quark::Quark for CancelledOutcomeQuark {
        fn id(&self) -> QuarkId {
            self.id.clone()
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            // Simulates exactly what `AcpSession::run_turn` now reports for a
            // real `StopReason::Cancelled` resolution: no message, and the fact
            // stated explicitly rather than left to be inferred.
            std::fs::write(turn.cwd.join("half-done.txt"), "cut off mid-work\n")?;
            Ok(TurnOutcome { message: None, cancelled: true, ..Default::default() })
        }
    }

    let repo = git_init_repo();
    let root = repo.path().to_path_buf();
    std::fs::create_dir_all(root.join(".hadron")).unwrap();
    let field = root.join(".hadron").join("field.jsonl");
    seed_mode(&field, Some("seat"), Mode::Bypass);
    append_event(
        &field,
        &Event::new(Actor::Human, None, Kind::Message { body: "@seat do something".into() }),
    )
    .unwrap();

    let base_before = crate::snapshot::git(&root, &["rev-parse", "HEAD"]).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));

    let mut engine = Engine::new(
        field.clone(),
        vec![Box::new(CancelledOutcomeQuark { id: QuarkId::new("seat") })],
        10,
    )
    .with_git(root.clone())
    .with_merge_gate(Arc::new(OrderRecordingRunner(calls.clone())));

    engine.run_until_quiesce().await.unwrap();

    assert!(
        calls.lock().unwrap().is_empty(),
        "the merge gate ran ({:?}) on a turn that resolved cancelled",
        calls.lock().unwrap()
    );
    let base_after = crate::snapshot::git(&root, &["rev-parse", "HEAD"]).unwrap();
    assert_eq!(base_before, base_after, "a cancelled turn's branch must never land");
}

/// **Task 9, Defect 2.** A graceful cancel that never resolves in time must
/// still ground the abandoned turn when the deadline fallback aborts it —
/// otherwise `next_pending` keeps finding the old dispatch record unanswered
/// and re-excites the quark onto the STALE task, instead of the message that
/// actually interrupted it. Uses `with_cancel_deadline` to make the fallback
/// fire in milliseconds rather than the real 5s.
#[tokio::test]
async fn a_deadline_abandoned_turn_is_grounded_not_left_stale() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct IgnoresCancelQuark {
        id: QuarkId,
        excite_count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl crate::quark::Quark for IgnoresCancelQuark {
        fn id(&self) -> QuarkId {
            self.id.clone()
        }
        fn flavor(&self) -> Flavor {
            Flavor::Worker
        }
        fn energy(&self) -> EnergyState {
            EnergyState::Available
        }
        fn attach_cancel_slot(&mut self, slot: crate::quark::CancelSlot) {
            // Acknowledges the request (`true`) but nothing in `excite` below
            // ever checks it — simulating a transport whose graceful cancel
            // never actually lands, forcing the CANCEL_DEADLINE fallback.
            slot.set(Some(Arc::new(|| true)));
        }
        async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
            let n = self.excite_count.fetch_add(1, Ordering::SeqCst) + 1;
            if n == 1 {
                tokio::time::sleep(Duration::from_secs(30)).await;
                return Ok(TurnOutcome { message: Some("finished uninterrupted".into()), ..Default::default() });
            }
            Ok(TurnOutcome { message: Some(format!("resumed on: {}", turn.task)), ..Default::default() })
        }
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("field.jsonl");
    append_event(
        &path,
        &Event::new(
            Actor::Quark(QuarkId::new("reporter")),
            Some(QuarkId::new("seat")),
            Kind::Message { body: "first task".into() },
        ),
    )
    .unwrap();

    let excite_count = Arc::new(AtomicUsize::new(0));
    let mid_flight = {
        let path = path.clone();
        let excite_count = excite_count.clone();
        tokio::spawn(async move {
            for _ in 0..400 {
                if excite_count.load(Ordering::SeqCst) >= 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            seed_unaddressed_mention(&path, "seat", "second, urgent task");
        })
    };

    let mut engine = Engine::new(
        path.clone(),
        vec![Box::new(IgnoresCancelQuark { id: QuarkId::new("seat"), excite_count: excite_count.clone() })],
        10,
    )
    .with_cancel_deadline(Duration::from_millis(50));

    tokio::time::timeout(Duration::from_secs(8), engine.run_until_quiesce())
        .await
        .expect("the deadline fallback should have unstuck this well within 8s")
        .unwrap();
    mid_flight.await.unwrap();

    let events = read_events(&path).unwrap();
    assert!(
        !events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("resumed on: first task"))),
        "the quark was re-excited onto the ABANDONED assignment before the interrupting \
         message — without grounding, `next_pending` still finds the old dispatch record \
         unanswered and redispatches it first: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(&e.kind, Kind::Message { body }
            if body.contains("resumed on: @seat second, urgent task"))),
        "the quark was never re-excited onto the interrupting message at all: {events:?}"
    );
}
