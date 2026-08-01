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
    /// The dispatch text this row was opened by, verbatim. Kept because the title it
    /// yields is only a fallback: once the plan the dispatch names is on disk, the
    /// caller upgrades the title with [`retitle_from_plan`]. The projection is pure
    /// over `&[Event]` and never opens a file, so the plan arrives from outside.
    pub body: String,
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
                        tasks[ix].title = extracted_title(task);
                        tasks[ix].body = task.clone();
                        continue;
                    }
                }
                let Some(raw) = raw_task_text(&e.kind) else { continue };
                let title = extracted_title(raw);
                if title.trim().is_empty() {
                    continue;
                }
                let to_str = to.as_str().to_string();
                open.insert(to_str.clone(), tasks.len());
                by_event.insert(e.id.to_string(), tasks.len());
                tasks.push(SwarmTask {
                    to: to_str,
                    from: actor_label(&e.from),
                    title,
                    body: raw.to_string(),
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

/// Filter `tasks` to those that were in flight at `at`: `asked_at <= at` and
/// (`done_at.is_none() || done_at > at`).
pub fn tasks_at(tasks: &[SwarmTask], at: DateTime<Utc>) -> Vec<&SwarmTask> {
    tasks
        .iter()
        .filter(|t| t.asked_at <= at && t.done_at.map_or(true, |ended| ended > at))
        .collect()
}

/// Merge-gate heartbeats, projected into the same row shape `swarm_tasks` yields so the
/// Tasks tab can render both through one `task_row`. Unlike the rest of this module,
/// NOT pure over `&[Event]`: a gate publishes to `live::gates_dir`, never to the field
/// (see the Flight Recorder plan's "never write mid-gate progress to `field.jsonl`"),
/// so `gates_dir` — the directory itself, not the field path — is read directly here.
/// Only fresh heartbeats are returned (`live::gates`'s own `is_fresh` guard); a gate
/// whose heartbeat has stopped must not read as one still running.
pub fn live_rows(gates_dir: &std::path::Path, now: DateTime<Utc>) -> Vec<SwarmTask> {
    hadron_lattice::live::gates(gates_dir, now)
        .into_iter()
        .map(|a| SwarmTask {
            to: a.quark.as_str().to_string(),
            from: "gate".to_string(),
            title: a.detail.clone(),
            body: a.detail,
            state: TaskState::Working,
            asked_at: a.started.unwrap_or(a.at),
            done_at: None,
        })
        .collect()
}

/// The timeline boundaries spanning `tasks`: earliest `asked_at` to latest of (`done_at` or `now`).
/// Returns `None` when `tasks` is empty.
pub fn span(tasks: &[SwarmTask], now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    if tasks.is_empty() {
        return None;
    }
    let start = tasks.iter().map(|t| t.asked_at).min()?;
    let end = tasks
        .iter()
        .map(|t| t.done_at.unwrap_or(now))
        .chain(std::iter::once(now))
        .max()?;
    Some((start, end.max(start)))
}

/// The instant `fraction` of the way along a track spanning `start`..`end`.
///
/// The scrubber's only geometry rule, kept here so the click path and the drag path
/// cannot disagree about where a pixel lands. `fraction` is clamped, so a drag that
/// leaves the track sideways pins to an end rather than naming a time off the timeline.
pub fn instant_at_fraction(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    fraction: f64,
) -> DateTime<Utc> {
    let span_ms = (end - start).num_milliseconds().max(0) as f64;
    start + chrono::Duration::milliseconds((fraction.clamp(0.0, 1.0) * span_ms) as i64)
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

/// Retitle every row whose dispatch names a task of `plan_path` with that task's own
/// heading, taken from `plan_headings` — the `task_name`s of
/// `app::mentions::parse_plan_tasks`, parsed by the caller and threaded in.
///
/// Two things this deliberately does not do. It does not read the plan: `model` is pure
/// over `&[Event]` and the only caller (`Chamber::update_active_plan`) already has the
/// content in hand. And it does not parse the plan itself — `parse_plan_tasks` is the
/// one plan parser in the chamber and lives behind the `gui` feature, which `model` is
/// not compiled under, so its *output* crosses the boundary rather than a second copy
/// of it (Rule 2).
///
/// A row whose dispatch names no plan, names a *different* plan, or names no `Task N`
/// keeps the title extracted from its prose. That last condition is what keeps a plan
/// **discussion** from being retitled: mentioning a plan path is not naming a task in
/// it (nucleus `plan-ref-discussion-masks-active-plans`).
pub fn retitle_from_plan(tasks: &mut [SwarmTask], plan_path: &str, plan_headings: &[String]) {
    for task in tasks.iter_mut() {
        if let Some(heading) = plan_heading_for(&task.body, plan_path, plan_headings) {
            task.title = trim_title(heading);
        }
    }
}

/// The heading of the plan task `body` dispatches, if it dispatches one of `plan_path`'s.
fn plan_heading_for<'h>(
    body: &str,
    plan_path: &str,
    plan_headings: &'h [String],
) -> Option<&'h str> {
    // Compare by file name: a dispatch may name the plan absolutely
    // (`/home/Jake/dev/hadron/.hadron/docs/plans/….md`) while the active path is
    // repo-relative, and both are the same file.
    let named = hadron_gluon::skills::plan_ref(body)?;
    if file_name_of(&named) != file_name_of(plan_path) {
        return None;
    }
    let n = task_number(body)?;
    plan_headings
        .iter()
        .find(|h| heading_is_task(h, n))
        .map(String::as_str)
}

fn file_name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The `N` of the first `Task N` in `body`, ignoring the markdown around it.
fn task_number(body: &str) -> Option<u32> {
    let mut rest = body;
    while let Some(ix) = rest.find("Task") {
        let after = rest[ix + "Task".len()..].trim_start_matches(['*', '`', ' ']);
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
        rest = &rest[ix + "Task".len()..];
    }
    None
}

/// Does this plan heading (`Task 4: a task title that means something`) name task `n`?
/// The digit run must end, or `Task 1` would answer for `Task 10`.
fn heading_is_task(heading: &str, n: u32) -> bool {
    task_number(heading) == Some(n) && heading.trim_start().starts_with("Task")
}

/// The row's title as read out of the dispatch prose itself — the fallback used until
/// (and when) [`retitle_from_plan`] finds nothing better.
fn extracted_title(raw: &str) -> String {
    trim_title(&strip_dispatch_scaffolding(raw))
}

/// The text of a turn-request, or `None` for one that carries none — a
/// `PermissionGrant`, which is a turn-request to the router but has nothing to show a
/// human, and used to render as an empty line.
fn raw_task_text(kind: &Kind) -> Option<&str> {
    match kind {
        Kind::Message { body } => Some(body.as_str()),
        Kind::Assign { task, .. } => Some(task.as_str()),
        _ => None,
    }
}

/// Drop the parts of a dispatch that say who and where rather than what: the leading
/// `@mention` the router already turned into the row's `to`, the `Execute`/`Take` verb,
/// and the `of <plan path>` that the plan column says anyway.
fn strip_dispatch_scaffolding(raw: &str) -> String {
    let mut s = raw.trim_start();
    if let Some(rest) = s.strip_prefix('@') {
        s = rest.split_once(char::is_whitespace).map_or("", |(_, tail)| tail).trim_start();
    }
    for verb in ["Execute ", "Take "] {
        if let Some(rest) = s.strip_prefix(verb) {
            s = rest.trim_start();
            break;
        }
    }
    let Some(path) = hadron_gluon::skills::plan_ref(s) else { return s.to_string() };
    // The path as it was written, backticks and all — `plan_ref` trims those off.
    let Some(at) = s.find(&path) else { return s.to_string() };
    let head = s[..at].trim_end().trim_end_matches('`');
    let tail = s[at + path.len()..].trim_start().trim_start_matches('`').trim_start();
    let head = head.trim_end().strip_suffix(" of").unwrap_or(head).trim_end();
    format!("{head} {tail}").trim().to_string()
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

    /// The headings `app::mentions::parse_plan_tasks` yields for the plan the bodies
    /// below actually name — copied from its `### Task N: …` lines, `#`s stripped.
    fn plan_headings() -> Vec<String> {
        vec![
            "Task 1: `hadron_lattice::term` — one home for console output".to_string(),
            "Task 2: convert `hadron-gluon` to it".to_string(),
            "Task 8: the addressed message".to_string(),
            "Task 10: the far one".to_string(),
        ]
    }

    const PLAN: &str = ".hadron/docs/plans/2026-08-01-chamber-hover-tasks-console.md";

    #[test]
    fn a_dispatch_naming_a_plan_task_is_titled_by_that_tasks_heading() {
        // A real dispatch body, as the field stores it.
        let evs = vec![msg(
            Actor::Quark(QuarkId::new("acp-claude")),
            Some("cli-agy"),
            "@cli-agy Execute **Task 8** of `.hadron/docs/plans/2026-08-01-chamber-hover-tasks-console.md` \
             — new, appended to the end of the plan, and it is the last one.",
        )];
        let mut tasks = swarm_tasks(&evs);
        retitle_from_plan(&mut tasks, PLAN, &plan_headings());
        assert_eq!(tasks[0].title, "Task 8: the addressed message");
    }

    #[test]
    fn a_dispatch_with_no_resolvable_plan_keeps_its_prose_stripped_of_scaffolding() {
        let evs = vec![msg(
            Actor::Quark(QuarkId::new("acp-claude")),
            Some("cli-agy"),
            "@cli-agy Execute **Task 8** of `.hadron/docs/plans/2026-08-01-chamber-hover-tasks-console.md` \
             — new, appended to the end of the plan",
        )];
        let mut tasks = swarm_tasks(&evs);
        // A different plan is the active one, so nothing resolves.
        retitle_from_plan(&mut tasks, "docs/plans/other.md", &plan_headings());
        assert_eq!(
            tasks[0].title,
            "**Task 8** — new, appended to the end of the plan",
            "the leading @mention, the verb and the plan path all come off"
        );
    }

    #[test]
    fn a_plan_discussion_is_not_retitled_as_one_of_its_tasks() {
        // Names the plan, dispatches none of its tasks — nucleus
        // `plan-ref-discussion-masks-active-plans`.
        let evs = vec![msg(
            Actor::Human,
            Some("acp-claude"),
            "I did not tick the boxes in \
             `.hadron/docs/plans/2026-08-01-chamber-hover-tasks-console.md` — they are stale",
        )];
        let mut tasks = swarm_tasks(&evs);
        let before = tasks[0].title.clone();
        retitle_from_plan(&mut tasks, PLAN, &plan_headings());
        assert_eq!(tasks[0].title, before);
    }

    #[test]
    fn task_one_does_not_answer_for_task_ten() {
        let evs = vec![msg(Actor::Human, Some("sonnet"), "Execute **Task 10** of `{P}`".replace("{P}", PLAN).as_str())];
        let mut tasks = swarm_tasks(&evs);
        retitle_from_plan(&mut tasks, PLAN, &plan_headings());
        assert_eq!(tasks[0].title, "Task 10: the far one");
    }

    #[test]
    fn a_plan_heading_longer_than_the_limit_is_cut_on_a_char_boundary() {
        // Invariant: *Char Boundary Safety* — the plan path still goes through
        // `trim_title`, so a multi-byte heading cannot be sliced mid-character.
        let heading = format!("Task 2: {}", "é".repeat(TITLE_MAX_CHARS + 20));
        let evs = vec![msg(Actor::Human, Some("sonnet"), &format!("Execute **Task 2** of `{PLAN}`"))];
        let mut tasks = swarm_tasks(&evs);
        retitle_from_plan(&mut tasks, PLAN, &[heading]);
        assert_eq!(tasks[0].title.chars().count(), TITLE_MAX_CHARS + 1, "cut plus the ellipsis");
        assert!(tasks[0].title.ends_with('…'));
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

    #[test]
    fn tasks_at_filters_to_in_flight_tasks_at_timestamp() {
        let t0 = Utc::now();
        let t1 = t0 + chrono::Duration::seconds(10);
        let t2 = t0 + chrono::Duration::seconds(20);
        let t3 = t0 + chrono::Duration::seconds(30);

        let ended_before = SwarmTask {
            to: "a".into(),
            from: "human".into(),
            title: "ended before".into(),
            body: "".into(),
            state: TaskState::Done,
            asked_at: t0,
            done_at: Some(t1),
        };
        let straddling = SwarmTask {
            to: "b".into(),
            from: "human".into(),
            title: "straddling".into(),
            body: "".into(),
            state: TaskState::Done,
            asked_at: t1,
            done_at: Some(t3),
        };
        let started_after = SwarmTask {
            to: "c".into(),
            from: "human".into(),
            title: "started after".into(),
            body: "".into(),
            state: TaskState::Working,
            asked_at: t3,
            done_at: None,
        };

        let tasks = vec![ended_before, straddling, started_after];

        // At t2: ended_before is finished (t1 < t2), started_after is in future (t3 > t2).
        let in_flight = tasks_at(&tasks, t2);
        assert_eq!(in_flight.len(), 1);
        assert_eq!(in_flight[0].title, "straddling");

        // Boundary: asked_at == at (t1) -> straddling is in_flight (t1 <= t1 and t3 > t1)
        let at_start = tasks_at(&tasks, t1);
        assert!(at_start.iter().any(|t| t.title == "straddling"));
        assert!(
            !at_start.iter().any(|t| t.title == "ended before"),
            "done_at == at is not in_flight"
        );

        // Boundary: done_at == at (t1 for ended_before) -> not in_flight
        assert_eq!(tasks_at(&[tasks[0].clone()], t1).len(), 0);
    }

    #[test]
    fn span_returns_timeline_bounds_or_none() {
        let now = Utc::now();
        assert_eq!(span(&[], now), None);

        let t0 = now - chrono::Duration::seconds(100);
        let t1 = now - chrono::Duration::seconds(50);
        let task1 = SwarmTask {
            to: "a".into(),
            from: "human".into(),
            title: "t1".into(),
            body: "".into(),
            state: TaskState::Done,
            asked_at: t0,
            done_at: Some(t1),
        };
        let task2 = SwarmTask {
            to: "b".into(),
            from: "human".into(),
            title: "t2".into(),
            body: "".into(),
            state: TaskState::Working,
            asked_at: t1,
            done_at: None,
        };

        let (start, end) = span(&[task1, task2], now).unwrap();
        assert_eq!(start, t0);
        assert_eq!(end, now);
    }

    fn gates_tmp() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "hadron-chamber-tasks-live-rows-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_fresh_gate_heartbeat_becomes_one_running_row() {
        let dir = gates_tmp();
        let now = Utc::now();
        let started = now - chrono::Duration::seconds(12);
        let activity = hadron_lattice::Activity::gating(
            QuarkId::new("sonnet"),
            "quark/sonnet/some-branch",
            started,
        );
        hadron_lattice::live::publish_gate(&dir, "quark/sonnet/some-branch", &activity).unwrap();

        let rows = live_rows(&dir, now);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].to, "sonnet");
        assert_eq!(rows[0].title, "quark/sonnet/some-branch");
        assert_eq!(rows[0].state, TaskState::Working);
        assert_eq!(rows[0].asked_at, started);
        assert_eq!(rows[0].done_at, None);
    }

    #[test]
    fn a_stale_gate_heartbeat_becomes_no_row() {
        let dir = gates_tmp();
        let started = Utc::now();
        let activity =
            hadron_lattice::Activity::gating(QuarkId::new("agy"), "quark/agy/old", started);
        hadron_lattice::live::publish_gate(&dir, "quark/agy/old", &activity).unwrap();

        let long_after = started + chrono::Duration::seconds(200);
        assert!(live_rows(&dir, long_after).is_empty());
    }

    #[test]
    fn an_empty_gates_dir_is_no_rows() {
        let dir = gates_tmp();
        assert!(live_rows(&dir, Utc::now()).is_empty());
    }

    #[test]
    fn a_fraction_of_the_track_names_an_instant_on_it() {
        let start = Utc::now() - chrono::Duration::seconds(100);
        let end = start + chrono::Duration::seconds(100);

        // (fraction, expected offset from `start`, in seconds)
        let cases = [
            (0.0, 0),
            (0.5, 50),
            (1.0, 100),
            // A drag that leaves the track sideways pins to an end, never past it.
            (-2.0, 0),
            (7.5, 100),
        ];
        for (fraction, want_secs) in cases {
            assert_eq!(
                instant_at_fraction(start, end, fraction),
                start + chrono::Duration::seconds(want_secs),
                "fraction {fraction}"
            );
        }
    }

    #[test]
    fn a_zero_width_span_names_its_own_instant() {
        // A single task that has not finished yet can make `span` degenerate. Dividing by
        // that width is what a naive mapping would do; this must not panic or drift.
        let t = Utc::now();
        assert_eq!(instant_at_fraction(t, t, 0.7), t);
    }
}
