//! A live projection of the field into the swarm's outstanding/finished tasks. Pure
//! over `&[Event]`, same shape as the rest of `model` — no GPUI here.

use chrono::{DateTime, Utc};
use hadron_lattice::{Actor, Event, Kind, QuarkState};

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

impl SwarmTask {
    /// How long this took, or — while it is still in flight — how long it has been
    /// waiting. `now` is passed in rather than read here so the projection stays pure
    /// and the caller (a render pass) owns the clock.
    pub fn elapsed_secs(&self, now: DateTime<Utc>) -> i64 {
        self.done_at.unwrap_or(now).signed_duration_since(self.asked_at).num_seconds().max(0)
    }
}

/// How a dispatch ended. `Done` is a reply; the other two are the terminal statuses
/// [`hadron_gluon::router::is_turn_completion`] also accepts, and they used to be
/// folded into `Done` — a turn that errored out or was parked by the merge gate read
/// as a green "Done" chip, which is precisely the case a human needs to see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Working,
    Done,
    /// Parked: `Status{Blocked}` (a gate refusal, a permission refusal) or
    /// `Status{Waiting}`. The turn ended without the work being finished.
    Blocked,
    /// `Status{Error}` — the turn died.
    Failed,
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
///
/// Two of `is_turn_request`'s three kinds are NOT rows of their own, because the list
/// is read by a human rather than by the router:
///
/// * A `PermissionGrant` carries no task text at all, so it used to open a row with a
///   blank title — a line with nothing on it.
/// * An `Assign` is the engine's **dispatch record** for a request that is already
///   here: `engine/run.rs` stamps it with `answers` = the ULID of the message it
///   dispatches (`with_answers(driver.assignment)`). Every human message therefore
///   produced two rows, the second one a duplicate of the first. It is folded into the
///   task it names, and its resolved `task` string replaces the raw body as the title —
///   that string is what the quark was actually told to do.
pub fn swarm_tasks(events: &[Event]) -> Vec<SwarmTask> {
    let mut tasks: Vec<SwarmTask> = Vec::new();
    // The still-open task index per addressee, so a completion closes the most
    // recently opened one without a linear rescan of `tasks` on every event.
    let mut open: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    // Task index by the ULID of the event that opened it, so a dispatch record can
    // find the request it belongs to. Keyed by the ULID's string: `ulid` is a
    // dependency of `hadron-lattice`, not of the chamber, and one map is not worth
    // taking on the crate for.
    let mut by_event: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for e in events {
        if hadron_gluon::router::is_turn_request(e) {
            if let Some(to) = &e.to {
                // The dispatch record of a request we already have a row for.
                if let Kind::Assign { task, .. } = &e.kind {
                    if let Some(&ix) =
                        e.answers.as_ref().and_then(|a| by_event.get(&a.to_string()))
                    {
                        tasks[ix].title = trim_title(task);
                        continue;
                    }
                }
                let Some(title) = titled(&e.kind) else { continue };
                let to_str = to.as_str().to_string();
                open.insert(to_str.clone(), tasks.len());
                by_event.insert(e.id, tasks.len());
                tasks.push(SwarmTask {
                    to: to_str,
                    from: actor_label(&e.from),
                    title,
                    state: TaskState::Working,
                    asked_at: e.ts,
                    done_at: None,
                });
            }
        }
        if let Actor::Quark(q) = &e.from {
            if hadron_gluon::router::is_turn_completion(e, q) {
                if let Some(ix) = open.remove(q.as_str()) {
                    tasks[ix].state = outcome_of(&e.kind);
                    tasks[ix].done_at = Some(e.ts);
                }
            }
        }
    }

    tasks.reverse();
    tasks
}

/// What the completing event says the turn's outcome was. A `Message` is a reply, so
/// it is `Done`; the terminal statuses carry their own verdict.
fn outcome_of(kind: &Kind) -> TaskState {
    match kind {
        Kind::Status { state: QuarkState::Error } => TaskState::Failed,
        Kind::Status { state: QuarkState::Blocked | QuarkState::Waiting } => TaskState::Blocked,
        _ => TaskState::Done,
    }
}

fn actor_label(a: &Actor) -> String {
    match a {
        Actor::Human => "human".to_string(),
        Actor::Gluon => "gluon".to_string(),
        Actor::Quark(q) => q.as_str().to_string(),
    }
}

/// The row's title, or `None` for a request that carries no task text — a
/// `PermissionGrant`, which is a turn-request to the router but has nothing to show a
/// human, and used to render as an empty line.
fn titled(kind: &Kind) -> Option<String> {
    let raw = match kind {
        Kind::Message { body } => body.as_str(),
        Kind::Assign { task, .. } => task.as_str(),
        _ => return None,
    };
    let title = trim_title(raw);
    (!title.trim().is_empty()).then_some(title)
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

    /// The engine writes a `Kind::Assign` per dispatch (`engine/run.rs`), addressed to
    /// the quark and carrying the resolved task. It is the dispatch record, so its
    /// `task` — not a message body — is what the feed titles the row with.
    #[test]
    fn an_assign_record_titles_the_task_from_its_own_task_string() {
        let evs = vec![
            Event::new(
                Actor::Gluon,
                Some(QuarkId::new("sonnet")),
                Kind::Assign {
                    task: "Build the Tasks tab. Reuse the widgets.".to_string(),
                    invariants: vec![],
                },
            ),
            msg(Actor::Quark(QuarkId::new("sonnet")), None, "Done."),
        ];
        let tasks = swarm_tasks(&evs);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Build the Tasks tab.");
        assert!(matches!(tasks[0].state, TaskState::Done));
    }

    /// The gate parks a refused branch with `Status{Blocked}` (`engine/turn.rs::park_blocked`)
    /// and a died turn ends `Status{Error}`. Both are turn-COMPLETIONS, so both used to
    /// close the row as a green "Done" — the one outcome a human most needs to see,
    /// rendered as the one that says nothing is wrong.
    #[test]
    fn a_parked_or_errored_turn_is_not_reported_as_done() {
        let park = |state| {
            let evs = vec![
                msg(Actor::Human, Some("sonnet"), "Go."),
                Event::new(
                    Actor::Quark(QuarkId::new("sonnet")),
                    None,
                    Kind::Status { state },
                ),
            ];
            swarm_tasks(&evs)[0].state
        };
        assert_eq!(park(QuarkState::Blocked), TaskState::Blocked);
        assert_eq!(park(QuarkState::Waiting), TaskState::Blocked);
        assert_eq!(park(QuarkState::Error), TaskState::Failed);
        assert_eq!(park(QuarkState::Ground), TaskState::Done);
    }

    /// `engine/run.rs` writes one `Assign` per dispatch stamped `answers` = the message
    /// it dispatches, so a request and its dispatch record are the SAME task. Listing
    /// both put a duplicate under every single thing the human ever asked for.
    #[test]
    fn a_dispatch_record_folds_into_the_request_it_names() {
        let ask = msg(Actor::Human, Some("sonnet"), "Please rework the tasks list.");
        let record = Event::new(
            Actor::Gluon,
            Some(QuarkId::new("sonnet")),
            Kind::Assign { task: "Rework the tasks list.".to_string(), invariants: vec![] },
        )
        .with_answers(ask.id);

        let tasks = swarm_tasks(&[ask, record]);
        assert_eq!(tasks.len(), 1, "the dispatch record is not a second task");
        // The resolved task string wins: it is what the quark was actually told to do.
        assert_eq!(tasks[0].title, "Rework the tasks list.");
    }

    /// A `PermissionGrant` is an `is_turn_request` — it does dispatch a turn — but it
    /// carries no task text, so as a ROW it was a blank line with a chip on it.
    #[test]
    fn a_permission_grant_is_not_a_row() {
        let evs = vec![Event::new(
            Actor::Human,
            Some(QuarkId::new("sonnet")),
            Kind::PermissionGrant { approved: true, remember: false },
        )];
        assert!(swarm_tasks(&evs).is_empty());
    }

    #[test]
    fn an_open_task_measures_elapsed_against_now() {
        let evs = vec![msg(Actor::Human, Some("sonnet"), "Go.")];
        let t = &swarm_tasks(&evs)[0];
        assert_eq!(t.elapsed_secs(t.asked_at + chrono::Duration::seconds(90)), 90);
    }

    #[test]
    fn a_long_body_is_trimmed_to_a_title() {
        let long = "x".repeat(200);
        let evs = vec![msg(Actor::Human, Some("sonnet"), &long)];
        assert!(swarm_tasks(&evs)[0].title.chars().count() <= 81);
    }
}
