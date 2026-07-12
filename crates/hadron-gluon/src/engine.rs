use std::collections::HashMap;
use std::path::PathBuf;

use hadron_lattice::{Actor, Event, Kind, Projection, QuarkCard, QuarkId, QuarkState};

use crate::field::{append_event, read_events};
use crate::quark::Quark;
use crate::router::{next_pending, parse_addressee};
use std::fs;

fn build_invariants(workspace_root: &std::path::Path, requested: &[String]) -> (String, Vec<String>) {
    let mut combined = String::new();
    let invariants_dir = workspace_root.join(".hadron").join("nucleus").join("invariants");
    let mut available = Vec::new();
    
    // Always include standard_model.md if it exists
    let sm_path = invariants_dir.join("standard_model.md");
    if sm_path.exists() {
        match fs::read_to_string(&sm_path) {
            Ok(content) => {
                combined.push_str(&content);
                combined.push('\n');
            }
            Err(e) => {
                eprintln!("warning: requested invariant file exists but could not be read: {} - {}", sm_path.display(), e);
            }
        }
    }

    if invariants_dir.exists() {
        if let Ok(entries) = fs::read_dir(&invariants_dir) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.ends_with(".md") && name != "standard_model.md" {
                        available.push(name.trim_end_matches(".md").to_string());
                    }
                }
            }
        }
    }
    
    available.sort();
    
    // Sort requested invariants to ensure deterministic cache hits
    let mut requested_sorted = requested.to_vec();
    requested_sorted.sort();

    for req in requested_sorted {
        let req_path = invariants_dir.join(format!("{}.md", req));
        if req_path.exists() {
            match fs::read_to_string(&req_path) {
                Ok(content) => {
                    combined.push_str(&format!("\n# Rule: {}\n", req));
                    combined.push_str(&content);
                    combined.push('\n');
                }
                Err(e) => {
                    eprintln!("warning: requested invariant file exists but could not be read: {} - {}", req_path.display(), e);
                }
            }
        }
    }

    (combined.trim().to_string(), available)
}

/// Drives the sequential coordination loop over a single field file.
pub struct Engine {
    field_path: PathBuf,
    quarks: HashMap<QuarkId, Box<dyn Quark>>,
    roster: Vec<QuarkCard>,
    max_exchanges: usize,
    /// Opt-in git safety: target project repo to snapshot/diff. `None` = off.
    repo_root: Option<PathBuf>,
    /// Opt-in nucleus context: pre-rendered digest injected into projections.
    nucleus_digest: String,
    ledger: Option<crate::ledger::Ledger>,
    energy_limit: u32,
}

impl Engine {
    pub fn new(
        field_path: PathBuf,
        quarks: Vec<Box<dyn Quark>>,
        max_exchanges: usize,
    ) -> Self {
        let roster = quarks
            .iter()
            .map(|q| QuarkCard {
                id: q.id(),
                flavor: q.flavor(),
                energy: q.energy(),
                // Populated from the team config in the daemon bin (Task 6);
                // empty here keeps the pure engine independent of seating.
                provider: String::new(),
                model: String::new(),
            })
            .collect();
        let quarks = quarks.into_iter().map(|q| (q.id(), q)).collect();
        Engine {
            field_path,
            quarks,
            roster,
            max_exchanges,
            repo_root: None,
            nucleus_digest: String::new(),
            ledger: None,
            energy_limit: 0,
        }
    }

    /// The field file this engine reads and appends to.
    pub(crate) fn field_path(&self) -> &std::path::Path {
        &self.field_path
    }

    /// Opt in to git safety: snapshot the target repo before each excite and feed
    /// the working diff into the projection. Additive — off by default.
    pub fn with_git(mut self, repo_root: PathBuf) -> Self {
        self.repo_root = Some(repo_root);
        self
    }

    pub fn with_ledger(mut self, ledger: crate::ledger::Ledger, limit: u32) -> Self {
        self.ledger = Some(ledger);
        self.energy_limit = limit;
        self
    }

    /// Opt in to nucleus context: the pre-rendered digest (built by the daemon
    /// via `nucleus::load` → `nucleus::digest`) is injected into every projection.
    pub fn with_nucleus(mut self, digest: String) -> Self {
        self.nucleus_digest = digest;
        self
    }

    /// Excite quarks one at a time until no addressee is pending (quiesce) or the
    /// per-human-turn exchange budget is exhausted (backstop).
    pub async fn run_until_quiesce(&mut self) -> anyhow::Result<()> {
        let mut exchanges = 0usize;
        loop {
            let events = read_events(&self.field_path)?;

            let target = match next_pending(&events) {
                Some(q) => q,
                None => return Ok(()), // quiesce: control returns to the human
            };

            if exchanges >= self.max_exchanges {
                append_event(
                    &self.field_path,
                    &Event::new(
                        Actor::Gluon,
                        None,
                        Kind::Message {
                            body: format!(
                                "⚠️ backstop reached ({} exchanges); returning control to the human.",
                                self.max_exchanges
                            ),
                        },
                    ),
                )?;
                return Ok(());
            }

            if let Some(ledger) = &self.ledger {
                if ledger.is_depleted(&target, self.energy_limit)? {
                    let msg = format!("⚠️ Quark {} is depleted (exceeded {} tokens).", target.as_str(), self.energy_limit);
                    append_event(
                        &self.field_path,
                        &Event::new(Actor::Gluon, None, Kind::Message { body: msg }),
                    )?;
                    append_event(
                        &self.field_path,
                        &Event::new(Actor::Quark(target.clone()), None, Kind::Status { state: QuarkState::Blocked }),
                    )?;
                    continue; // Reroute: skip this quark and process the next pending event
                }
            }

            let git_diff = if let Some(root) = &self.repo_root {
                let snap =
                    crate::snapshot::create(root, &format!("before {}", target.as_str()))?;
                append_event(
                    &self.field_path,
                    &Event::new(
                        Actor::Gluon,
                        None,
                        Kind::Snapshot { git: snap.commit.clone(), label: snap.label.clone() },
                    ),
                )?;
                crate::snapshot::working_diff(root)?
            } else {
                String::new()
            };

            let mut requested_invariants = vec![];
            let mut task_desc = String::new();
            
            // Find the most recent *task-bearing* event targeting this quark. Skip
            // non-task events like a PermissionGrant (also addressed to the quark, to
            // re-trigger it) — otherwise a resumed quark would get an empty task.
            if let Some(trigger) = events.iter().rev().find(|e| {
                e.to.as_ref() == Some(&target)
                    && matches!(e.kind, Kind::Assign { .. } | Kind::Message { .. })
            }) {
                match &trigger.kind {
                    Kind::Assign { task, invariants } => {
                        task_desc = task.clone();
                        requested_invariants = invariants.clone();
                    }
                    Kind::Message { body } => {
                        task_desc = body.clone();
                        // For a follow-up message, scan further backward for the most recent Assign to inherit invariants
                        if let Some(assign_event) = events.iter().rev().find(|e| {
                            e.to.as_ref() == Some(&target) && matches!(e.kind, Kind::Assign { .. })
                        }) {
                            if let Kind::Assign { invariants, .. } = &assign_event.kind {
                                requested_invariants = invariants.clone();
                            }
                        }
                    }
                    _ => {}
                }
            }
            
            let workspace_root = self.field_path.ancestors()
                .find(|p| p.join(".hadron").exists())
                .unwrap_or_else(|| self.field_path.parent().unwrap_or_else(|| std::path::Path::new("")));
                
            let (invariants_text, available_invariants) = build_invariants(workspace_root, &requested_invariants);

            // Resolve the quark's effective mode from the field before the turn:
            // real adapters translate it into the CLI's permission posture, so the
            // mode must ride along on the projection (not just gate a post-turn ask).
            let turn_mode = hadron_gatekeeper::resolve_mode(&events, &target);

            let projection = Projection {
                task: task_desc,
                invariants: invariants_text,
                available_invariants,
                nucleus_digest: self.nucleus_digest.clone(),
                roster: self.roster.clone(),
                field_window: events.clone(),
                git_diff,
                mode: turn_mode,
            };

            let quark = self
                .quarks
                .get_mut(&target)
                .ok_or_else(|| anyhow::anyhow!("no such quark on roster: {}", target.as_str()))?;
            let outcome = quark.excite(projection).await?;

            if outcome.used_tokens > 0 {
                if let Some(ledger) = &self.ledger {
                    ledger.record_usage(&target, outcome.used_tokens)?;
                }
                append_event(
                    &self.field_path,
                    &Event::new(Actor::Quark(target.clone()), None, Kind::EnergyReport { used_tokens: outcome.used_tokens }),
                )?;
            }

            if let Some(body) = outcome.message {
                let to = parse_addressee(&body, &self.roster);
                append_event(
                    &self.field_path,
                    &Event::new(Actor::Quark(target.clone()), to, Kind::Message { body }),
                )?;
            }

            // A self-declared permission ask: record it, then let the effective
            // mode decide. The mode + allow-list are folded from the field's
            // prior ModeSet / remembered-grant events (the `events` binding above
            // already holds them — the just-appended req doesn't affect either).
            // A grant is addressed to the quark so `next_pending` re-selects it
            // (the resume path); the task survives via the task-bearing
            // trigger-finder above.
            if let Some(ask) = outcome.permission {
                let risk = ask.risk;
                let op = ask.description.clone();
                append_event(
                    &self.field_path,
                    &Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::PermissionReq { risk, description: ask.description },
                    ),
                )?;
                let mode = hadron_gatekeeper::resolve_mode(&events, &target);
                let rules = hadron_gatekeeper::allow_rules(&events);
                match hadron_gatekeeper::decide(mode, risk, &op, &target, &rules) {
                    hadron_gatekeeper::Decision::AutoApprove => {
                        // Pre-authorized by the mode: the gluon grants on the
                        // orchestrator's / human's standing authority.
                        append_event(
                            &self.field_path,
                            &Event::new(
                                Actor::Gluon,
                                Some(target.clone()),
                                Kind::PermissionGrant { approved: true, remember: false },
                            ),
                        )?;
                        exchanges += 1;
                        continue;
                    }
                    hadron_gatekeeper::Decision::AskHuman => {
                        // Pause: mark the quark waiting and quiesce until a human
                        // PermissionGrant (addressed to the quark) resumes it.
                        append_event(
                            &self.field_path,
                            &Event::new(
                                Actor::Quark(target.clone()),
                                None,
                                Kind::Status { state: QuarkState::Waiting },
                            ),
                        )?;
                        return Ok(());
                    }
                }
            }

            append_event(
                &self.field_path,
                &Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::Status { state: QuarkState::Ground },
                ),
            )?;

            exchanges += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{append_event, read_events};
    use crate::mock::MockQuark;
    use hadron_lattice::{Actor, EnergyState, Flavor, Kind, PermissionAsk, Projection, QuarkId, TurnOutcome};
    use std::sync::{Arc, Mutex};
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
    }

    #[async_trait::async_trait]
    impl crate::quark::Quark for PermissionQuark {
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
            self.tasks.lock().unwrap().push(turn.task.clone());
            self.calls += 1;
            if self.calls == 1 {
                Ok(TurnOutcome { message: None, used_tokens: 0, permission: Some(self.ask.clone()) })
            } else {
                Ok(TurnOutcome {
                    message: Some(self.reply.clone()),
                    used_tokens: 0,
                    permission: None,
                })
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
        }
    }

    fn has_kind(events: &[Event], pred: impl Fn(&Kind) -> bool) -> bool {
        events.iter().any(|e| pred(&e.kind))
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
        assert!(hadron_gatekeeper::pending_permission(&events).is_some(), "chamber can surface the request");
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
        run(&["init", "-q"]);
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
                Ok(TurnOutcome { message: Some("done".into()), used_tokens: 0, permission: None })
            }
        }

        let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10)
            .with_nucleus("## map.md\nthe project map".into());
        engine.run_until_quiesce().await.unwrap();
    }

    #[tokio::test]
    async fn orchestrated_handoff_runs_then_quiesces() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        seed_human_message(&path, "orch", "Build the thing. @worker will help.");

        let orch = MockQuark::scripted(
            QuarkId::new("orch"),
            Flavor::Orchestrator,
            vec![
                Some("Starting. @worker please build the UI.".into()),
                Some("All done. Handing back to the human.".into()),
            ],
        );
        let worker = MockQuark::scripted(
            QuarkId::new("worker"),
            Flavor::Worker,
            vec![Some("UI complete. @orch back to you.".into())],
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
                Ok(hadron_lattice::TurnOutcome { message: None, used_tokens: 100, permission: None })
            }
        }

        let ledger = Ledger::open_in_memory().unwrap();
        let mut engine = Engine::new(path.clone(), vec![Box::new(HeavyQuark)], 5)
            .with_ledger(ledger, 150);

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

    #[tokio::test]
    async fn engine_injects_invariants() {
        use std::fs;
        let fdir = tempdir().unwrap();
        
        // Setup .hadron/nucleus/invariants structure
        let invariants_dir = fdir.path().join(".hadron").join("nucleus").join("invariants");
        fs::create_dir_all(&invariants_dir).unwrap();
        fs::write(invariants_dir.join("standard_model.md"), "Be nice.").unwrap();
        fs::write(invariants_dir.join("rust_style.md"), "Use camelCase... wait no.").unwrap();

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
                assert!(turn.invariants.contains("Be nice."));
                assert!(turn.invariants.contains("# Rule: rust_style"));
                assert!(turn.invariants.contains("Use camelCase... wait no."));
                assert_eq!(turn.available_invariants, vec!["rust_style".to_string()]);
                Ok(TurnOutcome { message: Some("done".into()), used_tokens: 0, permission: None })
            }
        }

        let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10);
        engine.run_until_quiesce().await.unwrap();
    }
}
