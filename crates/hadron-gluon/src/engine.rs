use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hadron_lattice::{
    Actor, EnergyState, Event, Flavor, Kind, Projection, QuarkCard, QuarkId, QuarkState,
    TurnOutcome,
};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinSet;

use crate::field::{append_event, read_events};
use crate::quark::Quark;
use crate::router::{human_mentions, next_pending, parse_addressee};
use crate::skills;
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

/// How long a single turn may run before the engine declares it dead and writes a
/// terminal status for the quark itself.
///
/// This exists because a turn can end **without ever returning**. The observed
/// failure: a quark went `Excited`, its CLI process died (or orphaned its stdout
/// pipe to a grandchild, so `wait_with_output` never saw EOF), and the turn future
/// simply never resolved. `run_until_quiesce` cannot quiesce while a turn is in
/// flight, so the dispatch loop sat in `select!` for 56 minutes with no `Ground`,
/// no `Error`, and no re-dispatch. A quark whose turn dies without writing a
/// terminal status is lost forever.
///
/// **30 minutes.** Sized to be *loose*, not tight: a real coding turn here
/// legitimately runs for many minutes (a quark that reads a crate, edits, and runs
/// `cargo test --workspace` can burn ten-plus), and killing a healthy turn is
/// strictly worse than the wedge this guards — the work is lost AND the human is
/// lied to. 30 min is comfortably past any turn we have observed while still
/// bounding the wedge to something a human notices once, not something that eats an
/// afternoon. Override per-engine with [`Engine::with_turn_deadline`].
pub const TURN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30 * 60);

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

/// The Standard Model, **compiled into the binary**.
///
/// Not read from disk, and deliberately so. The rules that stop a quark
/// confabulating a result are worthless if they can be lost by a fresh clone, a
/// `.gitignore`, or a user deleting a directory — a swarm whose invariants
/// silently vanish is a swarm that silently gets worse. This tier is handed to
/// every quark on every turn, always, with no file to go missing.
pub const STANDARD_MODEL: &str = include_str!("../invariants/standard_model.md");

/// The swarm's memory INDEX for this project — one file, shared by every quark.
///
/// It was one file *per quark*, which meant a lesson agy paid for in blood was one
/// opus would pay for again. Shared, so the swarm learns once.
fn memory_index_path(workspace_root: &std::path::Path) -> std::path::PathBuf {
    memory_dir(workspace_root).join("index.md")
}

/// Where the long-form notes live: `.hadron/memory/notes/<slug>.md`. The index names
/// them; the engine never loads them. That is the whole token argument — an index of
/// one-liners stays cheap forever, and the detail is paid for only on the turn a quark
/// actually opens it.
fn memory_notes_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    memory_dir(workspace_root).join("notes")
}

fn memory_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    workspace_root.join(".hadron").join("memory")
}

/// The index is in **every** prompt of **every** turn, so its size is a tax paid
/// forever — and the tax is *context*, not money. Prompt caching makes re-sending it
/// cheap; it does not make it free, because every token here is a token the model is
/// not spending on the task. It is also the wrong thing to grow: the index is a
/// routing table (one line per lesson) and the detail belongs in `notes/`, which the
/// engine never loads. A file that outgrows this has stopped being an index.
const MEMORY_INDEX_BUDGET: usize = 32 * 1024;

/// Read the memory index, capped. A missing file is the normal first-run case, not an
/// error — it simply means the swarm has learned nothing here yet.
///
/// Returns the text and whether it was cut. Cutting silently is the one thing we do
/// not do: a quark that cannot see a lesson, and cannot tell that it cannot see it,
/// acts confidently on a partial picture.
///
/// **What we cut matters as much as that we cut.** Slicing the first N bytes keeps the
/// OLDEST lessons and throws away the newest — exactly backwards, since the index is
/// appended to and the freshest lesson is the one just paid for. So we keep the header
/// (it defines the format a quark must write back in) and then the most recent lines
/// that fit, dropping the middle.
fn read_memory_index(path: &std::path::Path) -> (String, bool) {
    let raw = fs::read_to_string(path).unwrap_or_default();
    if raw.len() <= MEMORY_INDEX_BUDGET {
        return (raw, false);
    }

    let lines: Vec<&str> = raw.lines().collect();
    // The header is everything before the first lesson line: it carries the format.
    let first_lesson = lines
        .iter()
        .position(|l| l.trim_start().starts_with("- **"))
        .unwrap_or(0);
    let (header, lessons) = lines.split_at(first_lesson);

    let header_text = header.join("\n");
    // Reserve room for the header; if the header alone blows the budget the file is
    // not an index at all, and the old head-slice is the only honest thing left.
    if header_text.len() >= MEMORY_INDEX_BUDGET {
        let mut end = MEMORY_INDEX_BUDGET;
        while end > 0 && !raw.is_char_boundary(end) {
            end -= 1;
        }
        return (raw[..end].to_string(), true);
    }

    // Take lessons from the END backwards — newest first — until the budget is spent.
    let mut kept: Vec<&str> = Vec::new();
    let mut used = header_text.len();
    for line in lessons.iter().rev() {
        let cost = line.len() + 1; // +1 for the newline
        if used + cost > MEMORY_INDEX_BUDGET {
            break;
        }
        used += cost;
        kept.push(line);
    }
    kept.reverse();

    let mut out = header_text;
    out.push('\n');
    out.push_str(&kept.join("\n"));
    out.push('\n');
    (out, true)
}

/// Where a user's own always-on rules live, under their home directory. Their
/// preferences, across every project they run Hadron in.
fn global_invariants_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(
        std::path::PathBuf::from(home)
            .join(".hadron")
            .join("nucleus")
            .join("invariants"),
    )
}

/// Read every `*.md` in a directory, sorted by name so the prompt is deterministic
/// (a projection that reorders itself between turns busts every prompt cache).
/// A directory that isn't there is not an error — the tier is simply absent.
fn read_invariant_dir(dir: &std::path::Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };

    let mut found: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".md")?.to_string();
            match fs::read_to_string(e.path()) {
                Ok(body) => Some((stem, body)),
                Err(err) => {
                    // Loud, not silent: an unreadable rule file is a rule the quark
                    // is not being given, and nobody would otherwise ever know.
                    eprintln!(
                        "warning: invariant exists but could not be read: {} — {err}",
                        e.path().display()
                    );
                    None
                }
            }
        })
        .collect();

    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Assemble the three tiers of working protocol handed to a quark.
///
/// 1. **Hardcoded** — [`STANDARD_MODEL`], compiled in, always present, not optional.
/// 2. **Global** — `~/.hadron/nucleus/invariants/`, the user's own standing rules.
/// 3. **Repo** — `<workspace>/.hadron/nucleus/invariants/`, this project's rules.
///
/// Ordered narrowest-last, and the prompt *says* which tier each block came from:
/// a quark that cannot tell a shipped rule from a project rule cannot reason about
/// which one to question when they conflict. `requested` names extra repo rules to
/// pull in for this specific turn.
fn build_invariants(workspace_root: &std::path::Path, requested: &[String]) -> (String, Vec<String>) {
    let mut combined = String::new();

    // Tier 1 — always, whatever else is or isn't on disk.
    combined.push_str(STANDARD_MODEL.trim());
    combined.push('\n');

    // Tier 2 — the user's preferences, across all their projects.
    if let Some(global_dir) = global_invariants_dir() {
        for (name, body) in read_invariant_dir(&global_dir) {
            combined.push_str(&format!("\n# Your rule: {name}\n{}\n", body.trim()));
        }
    }

    // Tier 3 — this project. A cybersecurity repo and an indie game do not want the
    // same rules, so the repo tier is where the domain gets to speak.
    let repo_dir = workspace_root.join(".hadron").join("nucleus").join("invariants");
    let repo_rules = read_invariant_dir(&repo_dir);

    let mut available: Vec<String> = repo_rules.iter().map(|(n, _)| n.clone()).collect();
    available.sort();

    let mut requested_sorted = requested.to_vec();
    requested_sorted.sort();

    for (name, body) in &repo_rules {
        // Repo rules named `always.md` load unconditionally; the rest load when the
        // turn asks for them by name, so a big rulebook doesn't blow the budget.
        if name == "always" || requested_sorted.contains(name) {
            combined.push_str(&format!("\n# Project rule: {name}\n{}\n", body.trim()));
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
    /// The watchdog: how long an excited turn may run before the engine writes its
    /// terminal status *for* it. See [`TURN_DEADLINE`].
    turn_deadline: std::time::Duration,
    /// Quarks that are seated but **switched off**. They keep their instance (an ACP
    /// seat keeps its resident subprocess and its conversation); they are simply never
    /// excited. Absent from this set = participating, so a quark seated by any path
    /// that never heard of disabling is on, which is the safe default for a *reduction*
    /// of authority.
    disabled: HashSet<QuarkId>,
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
                display_name: None,
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
            turn_deadline: TURN_DEADLINE,
            disabled: HashSet::new(),
        }
    }

    /// Seat a quark on the **live** roster, replacing any quark already holding its id.
    ///
    /// Both `quarks` and `roster` are updated here, together, because they are the
    /// textbook two-fields-that-must-never-disagree: a quark in the map but not the
    /// roster is unaddressable, and one in the roster but not the map is a name that
    /// resolves to a turn the engine cannot run. There is no way to mutate one without
    /// the other.
    ///
    /// **Only ever call this at a quiescent point.** `&mut self` is what enforces that:
    /// [`Engine::run_until_quiesce`] borrows the engine mutably for its entire duration
    /// and only returns once every spawned turn has been *joined*, so a re-seat racing
    /// a running turn does not compile.
    pub fn seat(&mut self, quark: Box<dyn Quark>) {
        let id = quark.id();
        let card = QuarkCard {
            id: id.clone(),
            display_name: None,
            flavor: quark.flavor(),
            energy: quark.energy(),
            // Left empty exactly as `new` leaves it — the daemon owns legibility, and
            // a seat added at runtime must not acquire fields a booted one lacks.
            provider: String::new(),
            model: String::new(),
        };
        self.roster.retain(|c| c.id != id);
        self.roster.push(card);
        self.quarks.insert(id, Arc::new(AsyncMutex::new(quark)));
    }

    /// Remove a quark from the live roster. `true` if it was seated.
    ///
    /// Dropping the last `Arc` to its `SharedQuark` drops the adapter, which for an ACP
    /// seat takes its resident subprocess down with it. That is safe only because no
    /// turn holds a clone of that `Arc` at a quiescent point — see [`Engine::seat`].
    pub fn unseat(&mut self, id: &QuarkId) -> bool {
        self.roster.retain(|c| &c.id != id);
        self.quarks.remove(id).is_some()
    }

    /// Switch a seated quark's **participation** on or off.
    ///
    /// This is NOT [`Engine::unseat`], and the difference is the whole point: the quark
    /// keeps its entry in `quarks`, so its adapter — and for an ACP seat, its resident
    /// subprocess and its whole conversation — is never dropped. It keeps its row in
    /// `roster`, so `@mentions` of it still *resolve* rather than falling through to
    /// the orchestrator, which is what lets the engine say "that quark is disabled"
    /// instead of quietly answering as somebody else.
    ///
    /// All it changes is whether the dispatch loop will excite it.
    ///
    /// `&mut self` for the same reason as [`Engine::seat`]: only safe at a quiescent
    /// point, and the borrow checker is what enforces that rather than a comment.
    pub fn set_enabled(&mut self, id: &QuarkId, enabled: bool) {
        if enabled {
            self.disabled.remove(id);
        } else {
            self.disabled.insert(id.clone());
        }
    }

    /// Whether a seated quark will take turns. Unknown ids read as enabled — absence
    /// from the disabled set is what "on" means.
    pub fn is_enabled(&self, id: &QuarkId) -> bool {
        !self.disabled.contains(id)
    }

    /// How many quarks are seated. The daemon refuses a re-seat that would take this
    /// to zero: a swarm with nobody in it cannot be talked to, and the human would have
    /// no way to undo it from the chamber.
    pub fn seated_count(&self) -> usize {
        self.quarks.len()
    }

    /// The ids actually seated right now — the engine's own answer, not a caller's
    /// shadow copy of what it believes it seated. The daemon needs this to evict the
    /// mock quarks it booted with when a real `team.json` finally appears: those mocks
    /// correspond to no `Seat`, so no team-vs-team diff can ever see them.
    pub fn seated_ids(&self) -> Vec<QuarkId> {
        self.quarks.keys().cloned().collect()
    }

    /// Override the turn watchdog's [deadline](TURN_DEADLINE). Tests use a tiny one;
    /// production takes the default. A deadline of zero is not special-cased — an
    /// engine configured that way would time every turn out immediately, which is a
    /// misconfiguration, not a feature.
    pub fn with_turn_deadline(mut self, deadline: std::time::Duration) -> Self {
        self.turn_deadline = deadline;
        self
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
        let msg_id = events[idx].id;
        self.human_addressees(body)
            .into_iter()
            .filter(|addressee| !Self::has_answered(&events[idx + 1..], addressee, msg_id))
            .map(|addressee| (addressee, body.clone()))
            .collect()
    }

    /// Has `addressee` answered the human message `msg_id`?
    ///
    /// The obvious reading — *"has it authored anything since?"* — is **wrong the moment
    /// the human speaks while the quark is already working.** The quark finishes the turn
    /// it was on, its reply lands after the newer message, and that reply gets counted as
    /// an answer to a message it could not possibly have seen. The newer message is then
    /// dropped, silently and permanently. Jake hit exactly this by typing twice.
    ///
    /// So an event answers a message only if it **says** it does: the engine stamps
    /// `answers` with the assignment the turn was dispatched for.
    ///
    /// The `answers.is_none()` arm is not a loophole, it is the legacy reading, and it
    /// has to stay: every event written before this field existed carries `None`, and
    /// treating those as "has not answered" would re-excite a quark for every historical
    /// message in the field the next time the daemon starts. Absent is unknown, and for
    /// unknown we keep the old, order-based answer. New events are precise.
    fn has_answered(after: &[Event], addressee: &QuarkId, msg_id: ulid::Ulid) -> bool {
        after.iter().any(|e| {
            e.from == Actor::Quark(addressee.clone())
                && match e.answers {
                    Some(a) => a == msg_id,
                    None => true, // legacy event: fall back to "it spoke after the message"
                }
        })
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

        let (mut invariants_text, available_invariants) =
            build_invariants(&workspace_root, &requested_invariants);

        // The skill for THIS turn, appended after the static tiers so the long,
        // byte-stable prefix (Standard Model → global → repo) keeps its prompt cache
        // and only the tail varies per task.
        //
        // The engine picks it, and the engine computes who could take the next step —
        // both on purpose. A model asked to decide whether to follow a process skips it
        // under pressure, and a model asked who is available guesses; a disabled seat
        // keeps its roster card (`disable-is-not-unseat`), so naming it as a reviewer
        // routes the work into a void. Here, "available" is checked at dispatch.
        if let Some(m) = skills::select(&task_desc) {
            let peers = self
                .roster
                .iter()
                .filter(|c| &c.id != target)
                .filter(|c| c.energy != EnergyState::Depleted)
                .filter(|c| self.is_enabled(&c.id))
                .map(|c| c.id.clone())
                .collect();

            // Provenance read off DISK, not taken from the turn's word for it: this is
            // the one part of separation-of-duties the engine can actually prove.
            let plan_author = skills::plan_ref(&task_desc)
                .map(|rel| workspace_root.join(rel))
                .and_then(|path| fs::read_to_string(path).ok())
                .and_then(|md| skills::plan_author(&md));

            invariants_text.push_str(&skills::render(
                &m,
                target,
                &skills::Handoff { peers, plan_author },
            ));
        }

        // Resolve the quark's effective mode from the field before the turn:
        // real adapters translate it into the CLI's permission posture, so the
        // mode must ride along on the projection (not just gate a post-turn ask).
        let turn_mode = hadron_gatekeeper::resolve_mode(events, target);

        // Truncation must be *observable*, not just performed: a quark that cannot
        // see an earlier instruction, and is not told so, acts on a partial field
        // as confidently as on a whole one.
        let window = bounded_window(events, FIELD_WINDOW_BUDGET_BYTES);
        let truncated = window.len() < events.len();

        let memory_path = memory_index_path(&workspace_root);
        let (memory, memory_truncated) = read_memory_index(&memory_path);

        Projection {
            memory,
            memory_truncated,
            memory_path,
            memory_notes_dir: memory_notes_dir(&workspace_root),
            task: task_desc,
            invariants: invariants_text,
            available_invariants,
            nucleus_digest: self.nucleus_digest.clone(),
            roster: self.roster.clone(),
            // NOT `events.to_vec()`. The whole field is unbounded; it grew past the
            // kernel's 128 KiB single-argv limit and killed every agy turn with
            // E2BIG. Keep the most recent events that fit the byte budget.
            field_window: window,
            field_truncated: truncated,
            git_diff,
            // The quark's own worktree when worktree discipline is on. Without it,
            // the workspace root — the pre-worktree behaviour, kept for the mock
            // daemon and every test that doesn't opt into git.
            isolated: cwd.is_some(),
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
        // The message this turn was dispatched to answer. Stamped onto everything the
        // turn emits, so a reader can ask "is the human still waiting?" and get a fact
        // rather than an inference from what happens to come after what.
        assignment: Option<ulid::Ulid>,
    ) -> anyhow::Result<()> {
        // ONE id for everything this turn emits. The reply and the energy report are
        // separate events, and turns run in parallel — so without this a reader trying
        // to say "this reply cost X" has only ADJACENCY to join on, and adjacency is
        // wrong the first time two quarks answer at once. Stamped here, at the single
        // place a turn's events are written, so they cannot disagree.
        let turn = ulid::Ulid::new();
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

        // THE ONE TOTALLER. No adapter computes this; they report components and
        // `TokenSpend::fresh` sums the comparable ones (input + output, cache
        // excluded). `None` means the provider said nothing about tokens — unknown,
        // which is not the same as zero, so we report nothing rather than a hollow 0.
        let fresh = outcome.usage.spend.fresh();

        // The event fires when there is *anything* to say — spend, context, or quota.
        // It used to fire only on `used_tokens > 0`, which meant a provider that
        // reported context but no tokens had its telemetry silently dropped.
        if fresh.unwrap_or(0) > 0 || !outcome.usage.is_empty() {
            if let Some(ledger) = &self.ledger {
                // The depletion gate reads this ledger (`is_depleted`). It is fed the
                // cache-excluded figure on purpose: the old cross-provider sum would
                // have tripped an ACP quark ~200x early on cache reads it never paid
                // for. See `the-depletion-gate-is-a-loaded-gun` in the shared memory.
                if let Some(t) = fresh.filter(|t| *t > 0) {
                    ledger.record_usage(target, t)?;
                }
            }
            // `used_tokens` stays a `u32` on the Kind because the chamber matches
            // `Kind` exhaustively with no wildcard arm (see the note on `Event.usage`
            // in `event.rs`): widening the variant would break a crate this one does
            // not own. The components ride on the envelope's `usage` instead, which is
            // additive and which every existing reader already ignores safely.
            self.append(
                Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::EnergyReport { used_tokens: fresh.unwrap_or(0) },
                )
                .with_usage(outcome.usage.clone())
                .with_turn(turn)
                .answering(assignment),
            )
            .await?;
        }

        if let Some(body) = outcome.message {
            let to = parse_addressee(&body, &self.roster, Some(target));
            self.append(
                Event::new(Actor::Quark(target.clone()), to, Kind::Message { body })
                    .with_turn(turn)
                    .answering(assignment),
            )
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
                    self.append(
                        Event::new(
                            Actor::Quark(target.clone()),
                            None,
                            Kind::Status { state: QuarkState::Waiting },
                        )
                        .with_turn(turn)
                        .answering(assignment),
                    )
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
                self.append(
                    Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::Edit { paths, git, summary: headline.clone() },
                    )
                    .with_turn(turn)
                    .answering(assignment),
                )
                .await?;
            }

            // The assignment is COMPLETE when the quark hands control back (its reply
            // carries no `@mention`). That is when the merge gate fires — mid-chain
            // hand-offs keep working on the same branch and are not gated.
            if handed_back && self.merge.is_some() && self.merge_gate(target, t).await? {
                return Ok(()); // the gate parked the quark (Waiting / Blocked)
            }
        }

        self.append(
            Event::new(
                Actor::Quark(target.clone()),
                None,
                Kind::Status { state: QuarkState::Ground },
            )
            .with_turn(turn)
            .answering(assignment),
        )
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
        // The assignment rides along with the turn so that `finish_turn` can stamp
        // `answers` on what the turn emits. Without it, "has this quark answered the
        // human?" degenerates into "has it said anything since?", which silently eats
        // any message the human sends while the quark is already working.
        let mut turns: JoinSet<(
            QuarkId,
            Option<TurnTree>,
            Option<ulid::Ulid>,
            anyhow::Result<TurnOutcome>,
        )> = JoinSet::new();
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

                    // Switched off by the human. The quark is still seated and still
                    // resolves, so we SAY SO in the field rather than dropping the
                    // mention: a message that goes nowhere with no trace is the failure
                    // mode this codebase keeps rediscovering. `reroute_blocked` is the
                    // existing mechanism for exactly this (it also marks the quark
                    // Blocked, so the roster does not show it as forever-Excited).
                    if !self.is_enabled(&target) {
                        let msg = format!(
                            "⚠️ @{} is disabled and will not take this turn. Enable it in the roster to reach it.",
                            target.as_str()
                        );
                        self.reroute_blocked(&target, &msg).await?;
                        continue;
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
                    let assignment = driver.as_ref().map(|d| d.assignment);
                    let deadline = self.turn_deadline;
                    turns.spawn(async move {
                        let mut quark = quark.lock().await;
                        // THE WATCHDOG. A turn that never resolves — its CLI process
                        // died, or orphaned its stdout pipe to a grandchild so the
                        // adapter waits forever on an EOF that will never come — would
                        // otherwise keep this quark in `in_flight` for good: no terminal
                        // status, no quiesce, no re-dispatch, the quark simply gone.
                        // On expiry we DROP the turn future (which drops the adapter's
                        // `Child`, killing a still-live process — see `ProcessRunner`'s
                        // `kill_on_drop`) and return an error, which lands in the
                        // existing failed-turn arm below: `Status{Error}`, out of
                        // `in_flight`, excitable again by the next message.
                        //
                        // The lock is acquired OUTSIDE the timeout on purpose: the
                        // deadline measures the turn, not the wait for a turn slot.
                        let outcome = match tokio::time::timeout(
                            deadline,
                            quark.excite(projection),
                        )
                        .await
                        {
                            Ok(outcome) => outcome,
                            Err(_) => Err(anyhow::anyhow!(
                                "turn exceeded deadline with no terminal status: {} was excited \
                                 for {}s and its turn never returned (process gone, or hung with \
                                 no outcome); the engine is ending the turn on its behalf",
                                turn_id.as_str(),
                                deadline.as_secs(),
                            )),
                        };
                        (turn_id, turn_tree, assignment, outcome)
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
                Ok((target, tree, assignment, Ok(outcome))) => {
                    in_flight.remove(&target);
                    if let Err(err) =
                        self.finish_turn(&target, outcome, tree.as_ref(), assignment).await
                    {
                        if first_err.is_none() {
                            first_err = Some(err);
                        }
                    }
                }
                Ok((target, _, _, Err(err))) => {
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
                Ok(TurnOutcome { message: None, permission: Some(self.ask.clone()), usage: Default::default() })
            } else {
                Ok(TurnOutcome {
                    message: Some(self.reply.clone()),
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
            Ok(TurnOutcome { message: Some("ok".into()), permission: None, usage: Default::default() })
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
                    usage: Default::default(),
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
                Ok(TurnOutcome { message: Some(format!("{} done", self.id)), permission: None, usage: Default::default() })
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
                Ok(TurnOutcome { message: Some("done".into()), permission: None, usage: Default::default() })
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
                Ok(TurnOutcome { message: Some("I've got it.".into()), permission: None, usage: Default::default() })
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
                    usage: Default::default(),
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
                })
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

    /// An over-budget index must lose its OLDEST lessons, never its newest. The index
    /// is appended to, so a head-slice throws away the lesson a quark just paid for and
    /// keeps the one from a month ago — and it silently truncated mid-sentence, leaving
    /// a half-written lesson that reads as a whole one.
    #[test]
    fn an_over_budget_index_drops_the_oldest_lessons_and_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.md");

        let mut raw = String::from("# Memory index\n\nFormat: `- **<slug>** — <lesson>`\n\n");
        // Enough padded lessons to blow the budget several times over.
        for i in 0..400 {
            raw.push_str(&format!(
                "- **lesson-{i}** — {}\n",
                "x".repeat(200) // padding, so the budget is exceeded by bulk
            ));
        }
        raw.push_str("- **the-newest-lesson** — the one just paid for\n");
        assert!(raw.len() > MEMORY_INDEX_BUDGET, "the fixture must overflow");
        fs::write(&path, &raw).unwrap();

        let (out, truncated) = read_memory_index(&path);
        assert!(truncated, "an over-budget index must report that it was cut");
        assert!(out.len() <= MEMORY_INDEX_BUDGET);

        assert!(
            out.contains("the-newest-lesson"),
            "the newest lesson is the one just paid for — it must survive the cut"
        );
        assert!(
            out.contains("# Memory index") && out.contains("Format:"),
            "the header defines the format a quark must write back in; it must survive"
        );
        assert!(
            !out.contains("**lesson-0**"),
            "the oldest lesson is what should be dropped"
        );
    }

    /// An index that fits is handed over whole, and is not reported as cut.
    #[test]
    fn an_index_within_budget_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.md");
        let raw = "# Memory index\n\n- **a** — one\n- **b** — two\n";
        fs::write(&path, raw).unwrap();

        let (out, truncated) = read_memory_index(&path);
        assert_eq!(out, raw);
        assert!(!truncated);
    }

    /// The prompt tests prove `prompt.rs` *renders* memory. They prove nothing about
    /// whether the engine ever *reads* it — which is the exact gap ("correct" vs "runs")
    /// that cost us a whole session. This is the caller test: put a real file on disk at
    /// the real path, drive a real turn, and assert the quark received it.
    ///
    /// The index is SHARED: the file is `index.md`, not `worker.md`. A lesson one quark
    /// paid for has to reach the others, or the swarm learns nothing as a swarm.
    #[tokio::test]
    async fn the_shared_memory_index_actually_reaches_a_quarks_projection() {
        use std::fs;
        let ws = tempdir().unwrap();
        let mem_dir = ws.path().join(".hadron").join("memory");
        fs::create_dir_all(&mem_dir).unwrap();
        fs::write(mem_dir.join("index.md"), "The forge crate is unwired.").unwrap();

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
                    turn.memory.trim(),
                    "The forge crate is unwired.",
                    "the engine must load the shared memory index from disk"
                );
                assert!(
                    turn.memory_path.ends_with("memory/index.md"),
                    "one index for the whole swarm, not one file per quark, got {:?}",
                    turn.memory_path
                );
                assert!(
                    turn.memory_notes_dir.ends_with("memory/notes"),
                    "and it must know where the long-form notes live, got {:?}",
                    turn.memory_notes_dir
                );
                assert!(!turn.memory_truncated, "this index is two lines long");
                Ok(TurnOutcome {
                    message: Some("done".into()),
                    permission: None,
                    usage: Default::default(),
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
    fn an_oversized_memory_index_is_cut_and_says_so() {
        use std::fs;
        let ws = tempdir().unwrap();
        let path = memory_index_path(ws.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        // Multi-byte on purpose: cutting a UTF-8 file at a fixed byte offset is a panic
        // unless the cut walks back to a char boundary. Same crash family as the emoji bug.
        let fat = "é".repeat(MEMORY_INDEX_BUDGET);
        fs::write(&path, &fat).unwrap();

        let (text, truncated) = read_memory_index(&path);
        assert!(truncated, "an index over budget must report that it was cut");
        assert!(text.len() <= MEMORY_INDEX_BUDGET);
        assert!(!text.is_empty(), "cut, not discarded");

        // A small index is passed through whole and NOT flagged.
        fs::write(&path, "- **a** — a lesson.").unwrap();
        let (text, truncated) = read_memory_index(&path);
        assert_eq!(text, "- **a** — a lesson.");
        assert!(!truncated);

        // A missing index is the first-run case, not an error.
        let empty = tempdir().unwrap();
        assert_eq!(read_memory_index(&memory_index_path(empty.path())), (String::new(), false));
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
                    prompt.contains("Read it critically, before you touch anything"),
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
                    usage: Default::default(),
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
                    usage: Default::default(),
                })
            }
        }

        let mut engine =
            Engine::new(path.clone(), vec![Box::new(Probe), Box::new(Peer)], 10);
        engine.run_until_quiesce().await.unwrap();
    }

    /// A turn that is not plan work must be byte-for-byte what it was before skills
    /// existed. A router that fires on everything is a tax on every turn.
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
                    usage: Default::default(),
                })
            }
        }

        let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], 10);
        engine.run_until_quiesce().await.unwrap();
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
                Ok(TurnOutcome { message: Some("done".into()), permission: None, usage: Default::default() })
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
            Ok(TurnOutcome { message: Some("done".into()), permission: None, usage: Default::default() })
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

        let opus_before = engine.quarks.get(&QuarkId::new("opus")).unwrap().clone();
        let agy_before = engine.quarks.get(&QuarkId::new("agy")).unwrap().clone();

        engine.seat(Box::new(MockQuark::scripted(
            QuarkId::new("acp-claude"),
            Flavor::Worker,
            vec![None],
        )));

        assert_eq!(engine.seated_count(), 3, "the new seat joined the live roster");
        assert!(
            Arc::ptr_eq(&opus_before, engine.quarks.get(&QuarkId::new("opus")).unwrap()),
            "opus was rebuilt by a re-seat that had nothing to do with it"
        );
        assert!(
            Arc::ptr_eq(&agy_before, engine.quarks.get(&QuarkId::new("agy")).unwrap()),
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
            events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("disabled"))),
            "the field must SAY the quark is disabled, not silently swallow the mention"
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
        let before = engine.quarks.get(&id).unwrap().clone();

        engine.set_enabled(&id, false);
        assert!(!engine.is_enabled(&id));
        assert_eq!(engine.seated_count(), 1, "disabling must not unseat");
        assert!(
            Arc::ptr_eq(&before, engine.quarks.get(&id).unwrap()),
            "the instance was rebuilt by a mere disable — an ACP session would have died here"
        );
        assert!(engine.roster.iter().any(|c| c.id == id), "still on the roster, so @mentions still resolve");

        // Switched back on, it answers — and it is still the SAME quark, which is why
        // its scripted reply (consumed by nobody, because it never ran) is still queued.
        engine.set_enabled(&id, true);
        assert!(Arc::ptr_eq(&before, engine.quarks.get(&id).unwrap()));

        seed_human_message(&field, "agy", "you there?");
        engine.run_until_quiesce().await.unwrap();
        let events = read_events(&field).unwrap();
        assert!(
            events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body.contains("I ANSWERED"))),
            "re-enabled, it must take its turn"
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
        let before = engine.quarks.get(&QuarkId::new("agy")).unwrap().clone();

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
            !Arc::ptr_eq(&before, engine.quarks.get(&QuarkId::new("agy")).unwrap()),
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
}
