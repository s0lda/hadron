use std::collections::HashMap;
use std::path::PathBuf;

use hadron_lattice::{Actor, Event, Kind, Projection, QuarkCard, QuarkId, QuarkState};

use crate::field::{append_event, read_events};
use crate::quark::Quark;
use crate::router::{current_task, next_pending, parse_addressee};

/// Drives the sequential coordination loop over a single field file.
pub struct Engine {
    field_path: PathBuf,
    quarks: HashMap<QuarkId, Box<dyn Quark>>,
    roster: Vec<QuarkCard>,
    invariants: String,
    max_exchanges: usize,
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
        }
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

            let projection = Projection {
                task: current_task(&events, &target),
                invariants: self.invariants.clone(),
                nucleus_digest: String::new(),
                roster: self.roster.clone(),
                field_window: events.clone(),
                git_diff: String::new(),
            };

            let quark = self
                .quarks
                .get_mut(&target)
                .ok_or_else(|| anyhow::anyhow!("no such quark on roster: {}", target.as_str()))?;
            let outcome = quark.excite(projection).await?;

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
}
