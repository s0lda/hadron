//! A live projection of the field into the swarm's outstanding/finished tasks. Pure
//! over `&[Event]`, same shape as the rest of `model` — no GPUI here.

use chrono::{DateTime, Utc};
use hadron_lattice::{Actor, Event, Kind};

/// A dispatch and, once answered, its completion — derived from the field, not stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SwarmTask {
    pub to: String,
    pub from: String,
    pub title: String,
    pub state: TaskState,
    pub asked_at: DateTime<Utc>,
    pub done_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Working,
    Done,
}

/// A title is the body's first sentence, trimmed to this many **characters** (never
/// bytes — invariant: *Char Boundary Safety*) with an ellipsis when it was cut.
const TITLE_MAX_CHARS: usize = 80;

/// Project the field into the swarm's live task list, newest-first.
///
/// A turn-request addressed to a quark opens a task for it; that quark's own
/// turn-completion closes the most recently opened still-open task for it. Uses
/// `hadron_gluon::router::{is_turn_request, is_turn_completion}` rather than
/// re-deriving "the quark finished" — that predicate is shared with `next_pending`
/// and the engine's own completion check, and must never drift from them (SSOT).
pub fn swarm_tasks(events: &[Event]) -> Vec<SwarmTask> {
    let mut tasks: Vec<SwarmTask> = Vec::new();
    // The still-open task index per addressee, so a completion closes the most
    // recently opened one without a linear rescan of `tasks` on every event.
    let mut open: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for e in events {
        if hadron_gluon::router::is_turn_request(e) {
            if let Some(to) = &e.to {
                let to_str = to.as_str().to_string();
                open.insert(to_str.clone(), tasks.len());
                tasks.push(SwarmTask {
                    to: to_str,
                    from: actor_label(&e.from),
                    title: title_from(&e.kind),
                    state: TaskState::Working,
                    asked_at: e.ts,
                    done_at: None,
                });
            }
        }
        if let Actor::Quark(q) = &e.from {
            if hadron_gluon::router::is_turn_completion(e, q) {
                if let Some(ix) = open.remove(q.as_str()) {
                    tasks[ix].state = TaskState::Done;
                    tasks[ix].done_at = Some(e.ts);
                }
            }
        }
    }

    tasks.reverse();
    tasks
}

fn actor_label(a: &Actor) -> String {
    match a {
        Actor::Human => "human".to_string(),
        Actor::Gluon => "gluon".to_string(),
        Actor::Quark(q) => q.as_str().to_string(),
    }
}

fn title_from(kind: &Kind) -> String {
    let raw = match kind {
        Kind::Message { body } => body.as_str(),
        Kind::Assign { task, .. } => task.as_str(),
        _ => "",
    };
    trim_title(raw)
}

/// The first sentence of `raw`, trimmed to [`TITLE_MAX_CHARS`] **characters**.
fn trim_title(raw: &str) -> String {
    let sentence = match raw.find('.') {
        Some(byte_ix) => &raw[..=byte_ix],
        None => raw,
    };
    if sentence.chars().count() <= TITLE_MAX_CHARS {
        return sentence.to_string();
    }
    let truncated: String = sentence.chars().take(TITLE_MAX_CHARS).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::QuarkId;

    fn msg(from: Actor, to: Option<&str>, body: &str) -> Event {
        Event::new(from, to.map(QuarkId::new), Kind::Message { body: body.to_string() })
    }

    #[test]
    fn a_request_opens_a_task_and_the_reply_closes_it() {
        let evs = vec![
            msg(Actor::Human, Some("sonnet"), "Research the jail. Then report."),
            msg(Actor::Quark(QuarkId::new("sonnet")), None, "Done: the jail is structural."),
        ];
        let tasks = swarm_tasks(&evs);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].to, "sonnet");
        assert_eq!(tasks[0].title, "Research the jail.");
        assert!(matches!(tasks[0].state, TaskState::Done));
        assert!(tasks[0].done_at.is_some());
    }

    #[test]
    fn an_unanswered_request_is_still_working() {
        let evs = vec![msg(Actor::Human, Some("sonnet"), "Go.")];
        let tasks = swarm_tasks(&evs);
        assert!(matches!(tasks[0].state, TaskState::Working));
        assert!(tasks[0].done_at.is_none());
    }

    #[test]
    fn a_second_request_does_not_overwrite_the_first() {
        let evs = vec![
            msg(Actor::Human, Some("sonnet"), "First job."),
            msg(Actor::Human, Some("sonnet"), "Second job."),
        ];
        assert_eq!(swarm_tasks(&evs).len(), 2);
    }

    #[test]
    fn a_long_body_is_trimmed_to_a_title() {
        let long = "x".repeat(200);
        let evs = vec![msg(Actor::Human, Some("sonnet"), &long)];
        assert!(swarm_tasks(&evs)[0].title.chars().count() <= 81);
    }
}
