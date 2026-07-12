use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hadron_lattice::{
    Actor, Event, Flavor, Kind, Projection, QuarkCard, QuarkId, QuarkState, TurnOutcome,
};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinSet;

use crate::field::{append_event, read_events};
use crate::quark::Quark;
use crate::router::{human_mentions, next_pending, parse_addressee};
use std::fs;

/// A quark, shareable across concurrent turns. The `Mutex` is what lets a single
/// quark's `&mut self` turn move into a spawned task while the dispatch loop keeps
/// running — and it is *also* the belt to the `in_flight` set's braces: a quark can
/// only ever run one turn at a time.
type SharedQuark = Arc<AsyncMutex<Box<dyn Quark>>>;

/// How often the dispatch loop re-reads the field while turns are in flight, so a
/// message arriving mid-turn reaches a free quark instead of queueing behind the
/// running one. It bounds how long a quark sits unexcited, not how long a turn takes.
const FIELD_POLL: std::time::Duration = std::time::Duration::from_millis(150);

/// The event that drives a turn: the *assignment*. Its ULID names the quark's
/// branch (`quark/<id>/<assignment>`), and its body is the task on the projection —
/// one event, one source of truth, so the branch a quark commits to and the task it
/// was handed can never drift apart.
#[derive(Debug, Clone)]
struct Driver {
    assignment: ulid::Ulid,
    task: String,
    invariants: Vec<String>,
}

/// The worktree a turn ran in, carried from dispatch through to `finish_turn` so the
/// turn can end on a commit in the right branch. `head_before` is HEAD at dispatch:
/// the turn's commit is detected by **comparing HEAD**, not by assuming we were the
/// ones who committed — a `Bypass`-mode CLI may have run `git commit` itself, leaving
/// a clean tree behind an advanced branch.
#[derive(Debug, Clone)]
struct TurnTree {
    wt: crate::worktree::Worktree,
    base: String,
    head_before: Option<String>,
    assignment: ulid::Ulid,
}

/// The workspace directory that owns `field_path` — the directory every quark's CLI
/// is spawned in, and the root the nucleus/invariants are read from.
///
/// `base` is the directory a *relative* `field_path` is resolved against (the
/// daemon's cwd in production). Taking it as a parameter rather than reading
/// `current_dir()` inline is what makes this testable at all: the bug it exists to
/// prevent is only reachable via a relative path, and a test cannot chdir a shared
/// process without racing every other test.
pub(crate) fn workspace_root_of(field_path: &Path, base: &Path) -> PathBuf {
    let field_path = if field_path.is_absolute() {
        field_path.to_path_buf()
    } else {
        base.join(field_path)
    };

    field_path
        .ancestors()
        .find(|p| p.join(".hadron").exists())
        .unwrap_or_else(|| field_path.parent().unwrap_or_else(|| Path::new("")))
        .to_path_buf()
}

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

/// How many bytes of *rendered field transcript* a projection may carry.
///
/// Two hard reasons, not a taste:
///
/// 1. **`execve` rejects a long argv element.** `agy` has no stdin in print mode
///    and no `--prompt-file`, so its whole prompt is one argv element — and Linux
///    caps a single element at `MAX_ARG_STRLEN` = 128 KiB, unraisable. The field
///    window used to be `events.to_vec()` (the *entire* field), which grew past
///    that in normal use and killed every agy turn with E2BIG in ~0.7 ms, before
///    any subprocess started.
/// 2. **Tokens are money.** Re-sending the whole field on every turn of every quark
///    is quadratic in the swarm's lifetime, and the oldest events are the least
///    useful.
///
/// 48 KiB keeps a generous multi-turn transcript while leaving room under the
/// adapter's own [safety net](crate::adapter::agy) for the diff, nucleus and task.
/// A *byte* budget, not an event count: one long message can blow an event count.
pub const FIELD_WINDOW_BUDGET_BYTES: usize = 48 * 1024;

/// What one event costs the rendered prompt. Only `Message` bodies are rendered
/// (`prompt::build`), plus a small allowance for the `**from → to:**` prefix.
fn event_cost(e: &Event) -> usize {
    let body = match &e.kind {
        Kind::Message { body } => body.len(),
        _ => 0,
    };
    body + 32
}

/// The most recent events that fit in `budget` bytes, in field order.
///
/// Most-recent-wins: the driving message and the freshest context are the ones a
/// quark actually needs; the oldest are the ones to drop. Always yields at least
/// the single newest event, even if that one event is itself over budget — a quark
/// with no transcript at all cannot act, and the adapter's guard bounds the final
/// argv anyway.
fn bounded_window(events: &[Event], budget: usize) -> Vec<Event> {
    let mut spent = 0usize;
    let mut keep = 0usize;
    for e in events.iter().rev() {
        let cost = event_cost(e);
        if keep > 0 && spent + cost > budget {
            break;
        }
        spent += cost;
        keep += 1;
    }
    events[events.len().saturating_sub(keep)..].to_vec()
}

/// Drives the concurrent coordination loop over a single field file.
///
/// Turns run *in parallel*: every pending target found in one read of the field is
/// dispatched at once (one turn per quark, never two), and the engine only quiesces
/// when the field has no pending work **and** no turn is still in flight.
pub struct Engine {
    field_path: PathBuf,
    quarks: HashMap<QuarkId, SharedQuark>,
    roster: Vec<QuarkCard>,
    max_exchanges: usize,
    /// Opt-in git safety: target project repo to snapshot/diff. `None` = off.
    repo_root: Option<PathBuf>,
    /// Opt-in nucleus context: pre-rendered digest injected into projections.
    nucleus_digest: String,
    ledger: Option<crate::ledger::Ledger>,
    energy_limit: u32,
    /// Opt-in merge gate. `None` = off, and off is the default *on purpose*: the
    /// production runner shells out to `cargo test --workspace`, which a unit test
    /// must never reach (it would recurse into this very suite). Engine tests inject
    /// a fake; only the daemon bin seats the real one.
    merge: Option<Arc<dyn crate::merge::MergeRunner>>,
    /// Serializes every field append the engine makes. `append_event` re-opens the
    /// file O_APPEND each call, so a single line can't tear — but two concurrent
    /// turns finishing at once could still interleave their *sequences* of events.
    /// Holding this across each append keeps the JSONL a clean, totally-ordered log.
    field_lock: Arc<AsyncMutex<()>>,
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
        let quarks = quarks
            .into_iter()
            .map(|q| (q.id(), Arc::new(AsyncMutex::new(q)) as SharedQuark))
            .collect();
        Engine {
            field_path,
            quarks,
            roster,
            max_exchanges,
            repo_root: None,
            nucleus_digest: String::new(),
            ledger: None,
            energy_limit: 0,
            merge: None,
            field_lock: Arc::new(AsyncMutex::new(())),
        }
    }

    /// Opt in to the merge gate: when an assignment completes, run the workspace tests
    /// in the quark's worktree and — with a human's approval, asked for over the
    /// existing permission channel — land the branch on the default branch.
    ///
    /// Requires [`with_git`](Self::with_git); without a repo root there are no branches
    /// to gate.
    pub fn with_merge_gate(mut self, runner: Arc<dyn crate::merge::MergeRunner>) -> Self {
        self.merge = Some(runner);
        self
    }

    /// The one way the engine writes to the field: serialized behind `field_lock`,
    /// so concurrent turns can never interleave their event sequences.
    async fn append(&self, event: Event) -> anyhow::Result<()> {
        let _guard = self.field_lock.lock().await;
        Ok(append_event(&self.field_path, &event)?)
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

    /// Who a human message addresses: every quark it `@mentions` (anywhere, in
    /// order — the multi-dispatch case, "@opus X and @agy Y"), or, if it mentions
    /// no one, the orchestrator (default-routing, so the human can "just type").
    /// An empty result means no one can field it (e.g. no orchestrator on the
    /// roster and no valid mention).
    fn human_addressees(&self, body: &str) -> Vec<QuarkId> {
        let mut addressees = human_mentions(body, &self.roster);
        if addressees.is_empty() {
            if let Some(orch) = self.roster.iter().find(|c| c.flavor == Flavor::Orchestrator) {
                addressees.push(orch.id.clone());
            }
        }
        addressees
    }

    /// Route the human's latest UNADDRESSED (`to == None`) message. The chamber
    /// writes human messages with the mentions left in the body (not stripped into
    /// `to`), so one message can name several quarks. This now fans out *in
    /// parallel*: it returns EVERY addressee that hasn't answered yet, each handed
    /// the full message, and the dispatch loop excites them all at once. Addressed
    /// messages and quark hand-offs are `next_pending`'s job. An empty result means
    /// the message is fully served (or no one can field it).
    ///
    /// "Answered" means the quark has authored *any* event since the message — which
    /// includes the `Status{Excited}` the engine appends before a turn. That is what
    /// keeps an in-flight quark from being re-dispatched on the next read.
    fn human_message_targets(&self, events: &[Event]) -> Vec<(QuarkId, String)> {
        let Some(idx) = events
            .iter()
            .rposition(|e| e.from == Actor::Human && matches!(e.kind, Kind::Message { .. }))
        else {
            return Vec::new();
        };
        if events[idx].to.is_some() {
            return Vec::new(); // addressed message → next_pending owns it
        }
        let Kind::Message { body } = &events[idx].kind else {
            return Vec::new();
        };
        self.human_addressees(body)
            .into_iter()
            .filter(|addressee| {
                !events[idx + 1..]
                    .iter()
                    .any(|e| e.from == Actor::Quark(addressee.clone()))
            })
            .map(|addressee| (addressee, body.clone()))
            .collect()
    }

    /// Everyone the field is currently waiting on, in dispatch order: the explicit
    /// addressee / hand-off first (`next_pending`), then every unserved addressee of
    /// the latest unaddressed human message. The `String` is the `fallback_task` —
    /// the human message body, carried along because it is `to: None` and so cannot
    /// be found by the `to == target` trigger-finder.
    fn pending_targets(&self, events: &[Event]) -> Vec<(QuarkId, Option<String>)> {
        let mut targets: Vec<(QuarkId, Option<String>)> = Vec::new();
        if let Some(q) = next_pending(events) {
            targets.push((q, None));
        }
        for (q, task) in self.human_message_targets(events) {
            if !targets.iter().any(|(id, _)| id == &q) {
                targets.push((q, Some(task)));
            }
        }
        targets
    }

    /// The event that drives this turn — the *assignment*. Its `Ulid` names the
    /// quark's branch, so every turn of one assignment (including a turn resumed
    /// after a permission pause) resolves the **same** ULID and lands back in the
    /// same worktree on the same branch. A resumed quark that cut a fresh branch
    /// would orphan the uncommitted work it paused with; that is the exact inverse
    /// of the intent, and this function is what prevents it.
    ///
    /// The resolution order is the one `projection_for` has always used, now with
    /// the driving event's identity kept instead of thrown away. `None` = no
    /// task-bearing driver at all.
    fn driver_for(
        &self,
        events: &[Event],
        target: &QuarkId,
        fallback_task: Option<&str>,
    ) -> Option<Driver> {
        // 1. An unaddressed human message (single- or multi-mention, or default
        //    routing): the task is that message itself — there is no `to == target`
        //    event to recover it from.
        if let Some(task) = fallback_task {
            let ev = events.iter().rev().find(|e| {
                matches!(&e.kind, Kind::Message { body } if e.from == Actor::Human
                    && e.to.is_none()
                    && self.human_addressees(body).contains(target))
            })?;
            return Some(Driver {
                assignment: ev.id,
                task: task.to_string(),
                invariants: vec![],
            });
        }

        // 2. The most recent *task-bearing* event addressed to this quark. Skip
        //    non-task events like a PermissionGrant (also addressed to the quark, to
        //    re-trigger it) — otherwise a resumed quark would get an empty task, and
        //    (now) a branch named for the grant rather than the assignment.
        if let Some(trigger) = events.iter().rev().find(|e| {
            e.to.as_ref() == Some(target)
                && matches!(e.kind, Kind::Assign { .. } | Kind::Message { .. })
        }) {
            return Some(match &trigger.kind {
                Kind::Assign { task, invariants } => Driver {
                    assignment: trigger.id,
                    task: task.clone(),
                    invariants: invariants.clone(),
                },
                Kind::Message { body } => {
                    // A follow-up message inherits the invariants of the most recent
                    // Assign to this quark.
                    let invariants = events
                        .iter()
                        .rev()
                        .find_map(|e| match (&e.to, &e.kind) {
                            (Some(to), Kind::Assign { invariants, .. }) if to == target => {
                                Some(invariants.clone())
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    Driver { assignment: trigger.id, task: body.clone(), invariants }
                }
                _ => unreachable!("the find matched Assign | Message"),
            });
        }

        // 3. No event is addressed `to == target` — this is a quark resuming after a
        //    permission grant whose DRIVING message is an unaddressed (`to: None`)
        //    human message that named this quark in its body (a mention, or an
        //    unmentioned message the quark orchestrates). Recover the task from that
        //    message so the resumed turn isn't handed "". Resolution matches
        //    `human_message_targets` exactly, so both agree which message drives it.
        let driving = events.iter().rev().find(|e| {
            matches!(&e.kind, Kind::Message { body } if e.from == Actor::Human && self.human_addressees(body).contains(target))
        })?;
        let Kind::Message { body } = &driving.kind else {
            return None;
        };
        Some(Driver { assignment: driving.id, task: body.clone(), invariants: vec![] })
    }

    /// Build the projection handed to `target` for this turn, from the field as read
    /// at dispatch time and the already-resolved [`Driver`] (so the projection's task
    /// and the quark's branch name cannot disagree — they come from one event).
    ///
    /// `cwd` is where the quark works: its own worktree when worktree discipline is
    /// on, else the workspace root.
    fn projection_for(
        &self,
        events: &[Event],
        target: &QuarkId,
        driver: Option<&Driver>,
        git_diff: String,
        cwd: Option<PathBuf>,
    ) -> Projection {
        let task_desc = driver.map(|d| d.task.clone()).unwrap_or_default();
        let requested_invariants = driver.map(|d| d.invariants.clone()).unwrap_or_default();

        // Resolved against the daemon's cwd: a *relative* field path (`.hadron/field.jsonl`,
        // exactly how the daemon is launched) used to bottom out on the empty ancestor —
        // `"".join(".hadron")` exists, so the search "succeeded" with a root of "". That
        // empty root became the CLI's `cwd`, and `current_dir("")` is ENOENT, which the
        // spawn error then blamed on the program: `failed to spawn claude`.
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let workspace_root = workspace_root_of(&self.field_path, &base);

        let (invariants_text, available_invariants) = build_invariants(&workspace_root, &requested_invariants);

        // Resolve the quark's effective mode from the field before the turn:
        // real adapters translate it into the CLI's permission posture, so the
        // mode must ride along on the projection (not just gate a post-turn ask).
        let turn_mode = hadron_gatekeeper::resolve_mode(events, target);

        Projection {
            task: task_desc,
            invariants: invariants_text,
            available_invariants,
            nucleus_digest: self.nucleus_digest.clone(),
            roster: self.roster.clone(),
            // NOT `events.to_vec()`. The whole field is unbounded; it grew past the
            // kernel's 128 KiB single-argv limit and killed every agy turn with
            // E2BIG. Keep the most recent events that fit the byte budget.
            field_window: bounded_window(events, FIELD_WINDOW_BUDGET_BYTES),
            git_diff,
            // The quark's own worktree when worktree discipline is on. Without it,
            // the workspace root — the pre-worktree behaviour, kept for the mock
            // daemon and every test that doesn't opt into git.
            cwd: cwd.unwrap_or(workspace_root),
            mode: turn_mode,
        }
    }

    /// Everything that happens *after* a turn returns: energy, the reply (routed by
    /// its line-leading `@mention`), the permission ask, and the terminal status.
    ///
    /// Grounding is skipped on both permission paths, exactly as before: an
    /// auto-approved quark is re-dispatched by the grant (`to == quark`, so
    /// `next_pending` re-selects it) and grounds at the end of its *next* turn, and
    /// an ask-the-human quark ends `Waiting` until a human grant resumes it.
    /// Refuse to excite `target`, loudly and without stranding it: a Gluon message
    /// explaining why, then `Status{Blocked}` for the quark. The dispatch loop then
    /// `continue`s, so the quark's *siblings* still run — the reroute property.
    ///
    /// One shape for every refusal (energy depletion, an unusable worktree, a turn
    /// with no assignment) rather than a new mechanism per reason.
    async fn reroute_blocked(&self, target: &QuarkId, why: &str) -> anyhow::Result<()> {
        self.append(Event::new(Actor::Gluon, None, Kind::Message { body: why.to_string() }))
            .await?;
        self.append(Event::new(
            Actor::Quark(target.clone()),
            None,
            Kind::Status { state: QuarkState::Blocked },
        ))
        .await?;
        Ok(())
    }

    async fn finish_turn(
        &self,
        target: &QuarkId,
        outcome: TurnOutcome,
        tree: Option<&TurnTree>,
    ) -> anyhow::Result<()> {
        // Kept before the reply is moved into the field: the commit message names the
        // quark, its first line, and the assignment — greppable back to the event.
        let headline = outcome
            .message
            .as_deref()
            .and_then(|m| m.lines().find(|l| !l.trim().is_empty()))
            .unwrap_or("work")
            .trim()
            .chars()
            .take(72)
            .collect::<String>();
        let handed_back = outcome
            .message
            .as_deref()
            .map(|body| parse_addressee(body, &self.roster, Some(target)).is_none())
            .unwrap_or(true);

        if outcome.used_tokens > 0 {
            if let Some(ledger) = &self.ledger {
                ledger.record_usage(target, outcome.used_tokens)?;
            }
            self.append(Event::new(
                Actor::Quark(target.clone()),
                None,
                Kind::EnergyReport { used_tokens: outcome.used_tokens },
            ))
            .await?;
        }

        if let Some(body) = outcome.message {
            let to = parse_addressee(&body, &self.roster, Some(target));
            self.append(Event::new(Actor::Quark(target.clone()), to, Kind::Message { body }))
                .await?;
        }

        // A self-declared permission ask: record it, then let the effective mode
        // decide. The mode + allow-list are folded from the field as it stands
        // *before* the req is appended (the req itself must not become its own
        // remembered rule), but re-read here rather than reused from dispatch time —
        // a concurrent turn may have moved the field on since.
        if let Some(ask) = outcome.permission {
            let events = read_events(&self.field_path)?;
            let risk = ask.risk;
            let op = ask.description.clone();
            self.append(Event::new(
                Actor::Quark(target.clone()),
                None,
                Kind::PermissionReq { risk, description: ask.description },
            ))
            .await?;
            let mode = hadron_gatekeeper::resolve_mode(&events, target);
            let rules = hadron_gatekeeper::allow_rules(&events);
            match hadron_gatekeeper::decide(mode, risk, &op, target, &rules) {
                hadron_gatekeeper::Decision::AutoApprove => {
                    // Pre-authorized by the mode: the gluon grants on the
                    // orchestrator's / human's standing authority.
                    self.append(Event::new(
                        Actor::Gluon,
                        Some(target.clone()),
                        Kind::PermissionGrant { approved: true, remember: false },
                    ))
                    .await?;
                    return Ok(());
                }
                hadron_gatekeeper::Decision::AskHuman => {
                    // Pause: mark the quark waiting. The dispatch loop no longer has
                    // any pending work for it, so once every *other* in-flight turn
                    // finishes the engine quiesces and the human is asked.
                    //
                    // NOTE: deliberately NO commit here. A quark pausing mid-assignment
                    // for permission is not done — its uncommitted work must still be
                    // sitting in its worktree when the grant resumes it. That is exactly
                    // why `worktree::ensure` is idempotent for the same assignment.
                    self.append(Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::Status { state: QuarkState::Waiting },
                    ))
                    .await?;
                    return Ok(());
                }
            }
        }

        // The turn ends on a COMMIT in the quark's branch. Only here, on the Ground
        // path: both permission branches above return early, uncommitted, on purpose.
        if let Some(t) = tree {
            let message = format!("{}: {headline} [{}]", target.as_str(), t.assignment);
            crate::worktree::commit_turn(&t.wt, &message)?;

            // Compare HEAD; don't assume WE committed. A Bypass-mode CLI may have run
            // `git commit` itself, leaving a clean tree behind an advanced branch —
            // `commit_turn` returns `Ok(None)` there, but the work still happened.
            let head_now = crate::worktree::head(&t.wt.path);
            if head_now.is_some() && head_now != t.head_before {
                let git = head_now.unwrap_or_default();
                let paths = crate::worktree::changed_paths(&t.wt, &t.base)?;
                // `Kind::Edit` has existed in the lattice since day one and was emitted
                // by nobody. This is what it was for: a quark's work, attributed.
                self.append(Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::Edit { paths, git, summary: headline.clone() },
                ))
                .await?;
            }

            // The assignment is COMPLETE when the quark hands control back (its reply
            // carries no `@mention`). That is when the merge gate fires — mid-chain
            // hand-offs keep working on the same branch and are not gated.
            if handed_back && self.merge.is_some() && self.merge_gate(target, t).await? {
                return Ok(()); // the gate parked the quark (Waiting / Blocked)
            }
        }

        self.append(Event::new(
            Actor::Quark(target.clone()),
            None,
            Kind::Status { state: QuarkState::Ground },
        ))
        .await?;
        Ok(())
    }

    /// The merge gate, fired when an assignment completes. Returns `true` if it parked
    /// the quark (Waiting on a human, or Blocked on red tests), in which case the
    /// caller must NOT append `Ground`.
    ///
    /// The DECISION is pure and lives in `hadron-gatekeeper` (that crate is
    /// side-effect-free by contract). Only the EFFECTS — `cargo test`, `git merge` —
    /// live here, behind the [`MergeRunner`](crate::merge::MergeRunner) seam.
    ///
    /// Human approval reuses the EXISTING permission channel: a `PermissionReq` from
    /// the quark, surfaced by `gatekeeper::pending_permission`, rendered by the chamber
    /// the human already has, answered by the same `PermissionGrant`. No second
    /// approval mechanism.
    async fn merge_gate(&self, target: &QuarkId, t: &TurnTree) -> anyhow::Result<bool> {
        use hadron_gatekeeper::{BlockReason, BranchState, MergeVerdict};
        let Some(runner) = &self.merge else { return Ok(false) };
        let Some(root) = &self.repo_root else { return Ok(false) };

        let state = BranchState {
            commits: crate::worktree::commits_ahead(&t.wt, &t.base)?,
            dirty: crate::worktree::is_dirty(&t.wt.path)?,
            is_default_branch: t.wt.branch == t.base,
        };
        // Nothing to land (a pure-conversation turn): quiesce normally, silently.
        if state.commits == 0 {
            return Ok(false);
        }

        let events = read_events(&self.field_path)?;
        let op = hadron_gatekeeper::merge_op(&t.wt.branch, &t.base);

        // Approval: an explicit human grant for THIS branch, or the mode ladder saying
        // the human already delegated it. A merge is BashExec-class, so `decide` gives
        // Bypass ⇒ auto-merge for free, and Ask/Write/Auto ⇒ ask. (Auto never remembers
        // a merge: the op string contains the assignment ULID, so it is never the same
        // op twice — which is the right answer. You should not blanket-trust merges.)
        let mode = hadron_gatekeeper::resolve_mode(&events, target);
        let rules = hadron_gatekeeper::allow_rules(&events);
        let delegated = matches!(
            hadron_gatekeeper::decide(mode, hadron_gatekeeper::Risk::BashExec, &op, target, &rules),
            hadron_gatekeeper::Decision::AutoApprove
        );
        let approved = delegated || hadron_gatekeeper::merge_approved(&events, target, &op);

        // Tests run IN the quark's worktree, on the branch as it now stands — so we
        // never land untested commits, even on the re-asked second pass.
        let (tests_passed, tail) = runner.tests(&t.wt).await?;

        match hadron_gatekeeper::merge_decision(tests_passed, approved, &state) {
            MergeVerdict::Merge => {
                if delegated {
                    // Bypass: record req + grant for audit, exactly as the existing
                    // permission path does, then land without asking.
                    self.append(Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::PermissionReq {
                            risk: hadron_gatekeeper::Risk::BashExec,
                            description: op.clone(),
                        },
                    ))
                    .await?;
                    self.append(Event::new(
                        Actor::Gluon,
                        Some(target.clone()),
                        Kind::PermissionGrant { approved: true, remember: false },
                    ))
                    .await?;
                }
                let landed = runner.land(root, &t.wt, &t.base)?;
                self.append(Event::new(
                    Actor::Gluon,
                    None,
                    Kind::Message { body: landed.describe(&t.wt.branch, &t.base) },
                ))
                .await?;
                Ok(false) // landed → the quark grounds normally
            }
            MergeVerdict::Block(BlockReason::NotApproved) => {
                // Idempotent: if the ask is already outstanding for this branch, stay
                // Waiting rather than appending a second request.
                let already_asked = hadron_gatekeeper::pending_permission(&events)
                    .is_some_and(|p| p.quark == *target && p.description == op);
                if !already_asked {
                    self.append(Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::PermissionReq {
                            risk: hadron_gatekeeper::Risk::BashExec,
                            description: op,
                        },
                    ))
                    .await?;
                }
                self.append(Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::Status { state: QuarkState::Waiting },
                ))
                .await?;
                Ok(true)
            }
            MergeVerdict::Block(reason) => {
                // Red tests / a dirty tree / a branch that is somehow the default one.
                // The branch STAYS. Nothing is deleted — the work is evidence.
                self.reroute_blocked(
                    target,
                    &format!(
                        "⚠️ merge of `{}` blocked: {}. The branch is preserved at `{}`.\n\n{tail}",
                        t.wt.branch,
                        reason.describe(),
                        t.wt.path.display()
                    ),
                )
                .await?;
                Ok(true)
            }
        }
    }

    /// Dispatch every pending quark turn CONCURRENTLY until the field has no pending
    /// work **and** no turn is still in flight (quiesce), or the exchange budget is
    /// exhausted (backstop).
    ///
    /// Each pass re-reads the field, computes every pending target, and spawns a turn
    /// for each one that is not already running — so "@a do X and @b do Y" excites a
    /// and b at the same time instead of making b wait out a's whole turn. A quark
    /// only ever runs one turn at a time (`in_flight` + its own `Mutex`); a target
    /// that is already running is simply left for a later pass.
    ///
    /// Quiesce is the *conjunction*: an engine that has nothing pending but is still
    /// waiting on a running turn must not return, or the daemon would report the team
    /// idle while it is mid-thought.
    pub async fn run_until_quiesce(&mut self) -> anyhow::Result<()> {
        let mut exchanges = 0usize;
        let mut in_flight: HashSet<QuarkId> = HashSet::new();
        let mut turns: JoinSet<(QuarkId, Option<TurnTree>, anyhow::Result<TurnOutcome>)> =
            JoinSet::new();
        // The first turn error wins; siblings still run to completion (and still get
        // their terminal status) so a single failure can't strand the rest as
        // forever-Excited in the field.
        let mut first_err: Option<anyhow::Error> = None;
        let mut backstop = false;

        loop {
            let mut spawned_any = false;

            // Stop *starting* work once we're aborting or out of budget — but keep
            // looping, so already-running turns are drained rather than dropped.
            if first_err.is_none() && !backstop {
                let events = read_events(&self.field_path)?;

                for (target, fallback_task) in self.pending_targets(&events) {
                    // One turn per quark at a time. A quark that becomes pending again
                    // while it is running is picked up on a later pass (its reply, or
                    // the event that re-addressed it, is still in the field).
                    if in_flight.contains(&target) {
                        continue;
                    }

                    if exchanges >= self.max_exchanges {
                        backstop = true;
                        break;
                    }

                    if let Some(ledger) = &self.ledger {
                        if ledger.is_depleted(&target, self.energy_limit)? {
                            let msg = format!("⚠️ Quark {} is depleted (exceeded {} tokens).", target.as_str(), self.energy_limit);
                            self.reroute_blocked(&target, &msg).await?;
                            continue; // Reroute: skip this quark, dispatch the rest
                        }
                    }

                    let Some(quark) = self.quarks.get(&target).cloned() else {
                        first_err =
                            Some(anyhow::anyhow!("no such quark on roster: {}", target.as_str()));
                        break;
                    };

                    // The assignment that drives this turn. Its ULID names the branch,
                    // and its body is the task — resolved ONCE, so both agree.
                    let driver = self.driver_for(&events, &target, fallback_task.as_deref());

                    // Worktree discipline (on iff `with_git`): the quark works in its
                    // own checkout, on its own branch, and never in the human's tree.
                    let mut tree: Option<TurnTree> = None;
                    let mut git_diff = String::new();
                    if let Some(root) = self.repo_root.clone() {
                        // No task-bearing driver ⇒ no assignment ⇒ no branch to cut.
                        // Refuse rather than commit a quark's work to an unnamed branch.
                        let Some(driver) = driver.as_ref() else {
                            self.reroute_blocked(
                                &target,
                                &format!(
                                    "⚠️ {} has no assignment to work on (no task-bearing event drives this turn); refusing to excite it.",
                                    target.as_str()
                                ),
                            )
                            .await?;
                            continue;
                        };

                        // THE RULE, in the engine and not in a prompt: `ensure` refuses
                        // any tree whose HEAD is the default branch (or detached) as a
                        // post-condition, and refuses to cut a new branch from a dirty
                        // tree. A refusal blocks THIS quark and reroutes — its siblings
                        // still run — reusing the exact shape of the depletion branch.
                        let wt = match crate::worktree::ensure(
                            &root,
                            &target,
                            &driver.assignment.to_string(),
                        ) {
                            Ok(wt) => wt,
                            Err(e) => {
                                self.reroute_blocked(
                                    &target,
                                    &format!(
                                        "⚠️ refusing to excite {}: its worktree is not usable — {e:#}",
                                        target.as_str()
                                    ),
                                )
                                .await?;
                                continue;
                            }
                        };

                        // The snapshot is the pre-turn escape hatch (undo). It now points
                        // at the QUARK'S tree, so "before <quark>" means what it says.
                        let snap = crate::snapshot::create(
                            &wt.path,
                            &format!("before {}", target.as_str()),
                        )?;
                        self.append(Event::new(
                            Actor::Gluon,
                            None,
                            Kind::Snapshot { git: snap.commit.clone(), label: snap.label.clone() },
                        ))
                        .await?;

                        // Attribution comes from the BRANCH, not the working diff: once a
                        // turn ends on a commit, `git diff HEAD` is empty by construction.
                        // `<base>...HEAD` is "everything you have done on this assignment",
                        // and under concurrency it cannot show a sibling's edits.
                        let base = crate::worktree::default_branch(&root);
                        git_diff = crate::worktree::branch_diff(&wt, &base)?;
                        tree = Some(TurnTree {
                            head_before: crate::worktree::head(&wt.path),
                            wt,
                            base,
                            assignment: driver.assignment,
                        });
                    }

                    let projection = self.projection_for(
                        &events,
                        &target,
                        driver.as_ref(),
                        git_diff,
                        tree.as_ref().map(|t| t.wt.path.clone()),
                    );

                    // Announce the excitation *before* the turn runs, so the chamber can
                    // show the quark working while it works. The adapter only returns at
                    // the end of a turn, so without this the field is silent for the whole
                    // duration and the quark reads as ignoring the human. Appended after
                    // the projection is built, so the quark never sees its own status.
                    // It doubles as the in-flight marker in the field itself: `next_pending`
                    // and `human_message_targets` both count it as the quark having
                    // "authored since", so a running quark is never re-selected.
                    self.append(Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::Status { state: QuarkState::Excited },
                    ))
                    .await?;

                    let turn_id = target.clone();
                    let turn_tree = tree.clone();
                    turns.spawn(async move {
                        let mut quark = quark.lock().await;
                        let outcome = quark.excite(projection).await;
                        (turn_id, turn_tree, outcome)
                    });
                    in_flight.insert(target);
                    exchanges += 1;
                    spawned_any = true;
                }
            }

            // Quiesce is the conjunction: nothing new to start AND nothing running.
            if turns.is_empty() && !spawned_any {
                break;
            }

            // Something is running. Wait for the next turn to land — but do NOT wait
            // only on that: a message can arrive in the field *while* a turn grinds,
            // addressed to a quark that is free. Blocking solely on `join_next` would
            // queue it behind the running turn, so handing one quark a long task would
            // freeze the conversation with every other quark. So we race the join
            // against a poll tick, and on a tick we loop back to re-read the field and
            // dispatch anything newly pending.
            let joined = tokio::select! {
                joined = turns.join_next() => joined,
                _ = tokio::time::sleep(FIELD_POLL) => continue,
            };
            let Some(joined) = joined else {
                continue; // everything we spawned was already drained
            };

            match joined {
                Ok((target, tree, Ok(outcome))) => {
                    in_flight.remove(&target);
                    if let Err(err) = self.finish_turn(&target, outcome, tree.as_ref()).await {
                        if first_err.is_none() {
                            first_err = Some(err);
                        }
                    }
                }
                Ok((target, _, Err(err))) => {
                    // A failed turn must still leave a terminal status behind, or the
                    // quark reads as forever-working. Its siblings keep running.
                    in_flight.remove(&target);
                    let grounded = self
                        .append(Event::new(
                            Actor::Quark(target.clone()),
                            None,
                            Kind::Status { state: QuarkState::Error },
                        ))
                        .await;
                    if first_err.is_none() {
                        first_err = Some(err);
                        if let Err(io_err) = grounded {
                            first_err = Some(io_err);
                        }
                    }
                }
                Err(join_err) => {
                    // A panicking turn: we cannot tell which quark it was from the
                    // JoinError alone, so ground every quark still in flight rather than
                    // strand one Excited, and abort.
                    for target in std::mem::take(&mut in_flight) {
                        let _ = self
                            .append(Event::new(
                                Actor::Quark(target),
                                None,
                                Kind::Status { state: QuarkState::Error },
                            ))
                            .await;
                    }
                    turns.abort_all();
                    if first_err.is_none() {
                        first_err = Some(anyhow::anyhow!("a quark turn panicked: {join_err}"));
                    }
                }
            }
        }

        if let Some(err) = first_err {
            return Err(err);
        }

        if backstop {
            self.append(Event::new(
                Actor::Gluon,
                None,
                Kind::Message {
                    body: format!(
                        "⚠️ backstop reached ({} exchanges); returning control to the human.",
                        self.max_exchanges
                    ),
                },
            ))
            .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{append_event, read_events};
    use crate::mock::MockQuark;

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
                Ok(TurnOutcome { message: None, used_tokens: 0, permission: Some(self.ask.clone()), usage: Default::default() })
            } else {
                Ok(TurnOutcome {
                    message: Some(self.reply.clone()),
                    used_tokens: 0,
                    permission: None,
                    usage: Default::default(),
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
            Ok(TurnOutcome { message: Some("ok".into()), used_tokens: 0, permission: None, usage: Default::default() })
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
                Ok(TurnOutcome { message: Some(format!("{} done", self.id)), used_tokens: 0, permission: None, usage: Default::default() })
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
                Ok(TurnOutcome { message: Some("done".into()), used_tokens: 0, permission: None, usage: Default::default() })
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
                Ok(TurnOutcome { message: Some("I've got it.".into()), used_tokens: 0, permission: None, usage: Default::default() })
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
                Ok(hadron_lattice::TurnOutcome { message: None, used_tokens: 100, permission: None, usage: Default::default() })
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
                Ok(TurnOutcome { message: Some("done".into()), used_tokens: 0, permission: None, usage: Default::default() })
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
            Ok(TurnOutcome { message: Some("done".into()), used_tokens: 0, permission: None, usage: Default::default() })
        }
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
                used_tokens: 0,
                permission: None,
                usage: Default::default(),
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
}
