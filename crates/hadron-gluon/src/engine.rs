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
        if let Ok(content) = fs::read_to_string(&sm_path) {
            combined.push_str(&content);
            combined.push('\n');
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
            if let Ok(content) = fs::read_to_string(&req_path) {
                combined.push_str(&format!("\n# Rule: {}\n", req));
                combined.push_str(&content);
                combined.push('\n');
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
    invariants: String,
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
        invariants: String,
        max_exchanges: usize,
    ) -> Self {
        let roster = quarks
            .iter()
            .map(|q| QuarkCard {
                id: q.id(),
                flavor: q.flavor(),
                energy: q.energy(),
            })
            .collect();
        let quarks = quarks.into_iter().map(|q| (q.id(), q)).collect();
        Engine {
            field_path,
            quarks,
            roster,
            invariants,
            max_exchanges,
            repo_root: None,
            nucleus_digest: String::new(),
            ledger: None,
            energy_limit: 0,
        }
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
            
            // Find the most recent event targeting this quark to get its task context
            if let Some(trigger) = events.iter().rev().find(|e| e.to.as_ref() == Some(&target)) {
                match &trigger.kind {
                    Kind::Assign { task, invariants } => {
                        task_desc = task.clone();
                        requested_invariants = invariants.clone();
                    }
                    Kind::Message { body } => {
                        task_desc = body.clone();
                    }
                    _ => {}
                }
            }
            
            let workspace_root = self.field_path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .unwrap_or_else(|| self.field_path.parent().unwrap_or_else(|| std::path::Path::new("")));
                
            let (mut invariants_text, available_invariants) = build_invariants(workspace_root, &requested_invariants);
            
            // Fallback to initial engine invariants if no dynamic ones are found
            if invariants_text.is_empty() {
                invariants_text = self.invariants.clone();
            } else if !self.invariants.is_empty() {
                invariants_text = format!("{}\n\n{}", self.invariants, invariants_text);
            }

            let projection = Projection {
                task: task_desc,
                invariants: invariants_text,
                available_invariants,
                nucleus_digest: self.nucleus_digest.clone(),
                roster: self.roster.clone(),
                field_window: events.clone(),
                git_diff,
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
    use crate::field::append_event;
    use crate::mock::MockQuark;
    use hadron_lattice::{Actor, Flavor, Kind, QuarkId};
    use tempfile::tempdir;

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
        let mut engine = Engine::new(path.clone(), vec![Box::new(orch)], "x".into(), 10)
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
                Ok(TurnOutcome { message: Some("done".into()), used_tokens: 0 })
            }
        }

        let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], "x".into(), 10)
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
            "Coordinate via @mentions.".into(),
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
            "x".into(),
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
                Ok(hadron_lattice::TurnOutcome { message: None, used_tokens: 100 })
            }
        }

        let ledger = Ledger::open_in_memory().unwrap();
        let mut engine = Engine::new(path.clone(), vec![Box::new(HeavyQuark)], "".into(), 5)
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
}
