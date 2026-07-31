use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hadron_lattice::{
    Event, QuarkCard, QuarkId,
};
use tokio::sync::Mutex as AsyncMutex;
use ulid::Ulid;

use crate::field::append_event;
use crate::preons::{self, Preon};
use crate::quark::Quark;

// `pub(crate)`, not private: the prompt builder reads `tag_manifest` from here, and
// tests read `NUCLEUS_INDEX_BUDGET` — the default budget, re-exported from
// `nucleus_status` so it has ONE home. The RESOLVED (possibly configured) budget the
// prompt actually enforces travels on `Projection::nucleus_index_budget_bytes`
// instead — see `Engine::nucleus_index_budget_bytes`.
pub(crate) mod nucleus;
mod routing;
mod turn;
mod merge;
mod reboot;
mod run;
#[cfg(test)]
mod tests;

/// A quark, shareable across concurrent turns. The `Mutex` is what lets a single
/// quark's `&mut self` turn move into a spawned task while the dispatch loop keeps
/// running — and it is *also* the belt to the `in_flight` set's braces: a lane can
/// only ever run one turn at a time.
type SharedQuark = Arc<AsyncMutex<Box<dyn Quark>>>;

/// Which of an orchestrator seat's two turn slots a dispatch targets. Every
/// non-orchestrator seat only ever runs on `Work` — a seat with no chat lane
/// always resolves to `Work` regardless of who sent the message. See Task 6
/// Step 4 of `.hadron/docs/plans/2026-07-31-responsive-orchestrator.md` for
/// why a seat keeps ONE `QuarkId` and up to two lanes instead of two ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Lane {
    Work,
    Chat,
}

/// One seat's runtime instances: always a work lane, and a chat lane only for
/// an orchestrator-flavoured seat that has been given one via
/// [`Engine::seat_chat_lane`]. Two lanes, one `QuarkId` — every id-keyed
/// lookup (roster, mention parsing, the energy ledger, `live/<id>.json`)
/// stays untouched by the chat lane's existence.
///
/// **How the chat lane learns what the work lane is doing (Task 6 Step 6):**
/// it doesn't, specially — both lanes are separate `Quark` instances with
/// separate conversations/context, but they read the identical `field.jsonl`
/// through `projection_for` like every other seat, so the chat lane sees the
/// work lane's `Excited`/reply events (and everyone else's) the normal way.
/// No side-channel between the two instances exists or is planned; the field
/// is the one shared source of truth.
#[derive(Clone)]
struct Lanes {
    work: SharedQuark,
    chat: Option<SharedQuark>,
}

impl Lanes {
    fn get(&self, lane: Lane) -> Option<&SharedQuark> {
        match lane {
            Lane::Work => Some(&self.work),
            Lane::Chat => self.chat.as_ref(),
        }
    }
}

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
/// **It measures SILENCE, not elapsed time.** 30 minutes of a turn producing no sign
/// of life, where a sign of life is a fresh `<field-dir>/live/<quark>.json` — the file
/// the adapter already overwrites on every thought chunk and tool call. A turn that is
/// visibly working is never reaped, however long it runs; `run::until_silent` is the
/// clock and its doc explains why.
///
/// It was a flat wall-clock cap until 2026-07-30, and the paragraph that used to sit
/// here argued 30 minutes was "comfortably past any turn we have observed" — then a
/// real `acp-claude` turn running a long comparison suite was killed at exactly 1800s
/// while still working, losing the work and reporting it as a hang. Its own next
/// sentence had already named that outcome as strictly worse than the wedge it guards.
/// Sizing a wall-clock number is the wrong lever: no value is both loose enough for
/// the longest healthy turn and tight enough to notice a dead one.
///
/// **Two things this deliberately does not do.** A quark whose transport publishes no
/// activity at all — every CLI seat, since `build_seat_watched` only calls
/// `.watching()` on the ACP branch — still expires at exactly this duration, identical
/// to the old behaviour; the change can only extend a turn, never shorten one. And a
/// turn that keeps publishing forever is never reaped: a quark in a visible loop is a
/// human's `/clear`, not the watchdog's business. Override per-engine with
/// [`Engine::with_turn_deadline`].
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

/// What an unaddressed human/quark message hands a seat, kept as two fields because
/// the two questions have different answers: **who** was addressed and **what** the
/// work is.
///
/// They used to be one `String`, which was fine only while they were always the same
/// message. They are not: the human names the seats they want and then types the ask
/// (Bug C), so `task` is the human's latest word while `addressing` stays the message
/// that actually named this seat. Both are load-bearing — `addressing` is what the
/// exclusive-seat gate reads (`exclusive_task_names_target`), and handing it `task`
/// instead would make an exclusive seat fail a gate about text it was never given.
/// One field could not carry both meanings without one of the two call sites being
/// silently wrong.
#[derive(Debug, Clone)]
struct HumanTask {
    /// The body the quark is dispatched on.
    task: String,
    /// The body that named this quark — the eligibility text, never substituted.
    addressing: String,
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

/// Read `<workspace_root>/.hadron/nucleus/features.md` into the nucleus
/// digest. This is the reader Standard Model rule 9 promises — a missing
/// file is the normal first-run case (a fresh project has no feature map
/// yet), not an error.
///
/// It reads the file as an **index**, not whole: see [`features_index`].
pub fn build_nucleus_digest(workspace_root: &Path) -> String {
    let path = workspace_root.join(".hadron").join("nucleus").join("features.md");
    features_index(&std::fs::read_to_string(path).unwrap_or_default())
}

/// The feature map, reduced to what is worth sending on **every** turn.
///
/// `features.md` was force-loaded whole into every prompt of every quark, and it
/// is a document that grows: our own is ~3,000 words of per-component prose, all
/// of it re-read by every seat on every turn whether the turn touches a feature
/// or not. The same argument that keeps the lessons index one line per lesson
/// applies here — an index is force-loaded, a body is opened on demand.
///
/// What survives: the map **table** (name, description, status, entrypoint files —
/// the routing information, which is the point) and one line per `###` component
/// carrying its `Status`. What is dropped: the `Logic` prose under each component.
/// Rule 9's text tells a quark to open the file before touching a feature, which
/// is where that prose is paid for.
pub fn features_index(features_md: &str) -> String {
    let mut out = String::new();
    // A `###` section's name is emitted immediately; its `- **Status**:` line
    // arrives a line or two later, so the section stays "open" until the next
    // heading and the status is appended to the line already written.
    let mut open_section = false;
    for line in features_md.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("### ") {
            out.push_str(&format!("- {name}"));
            open_section = true;
        } else if trimmed.starts_with('#') {
            if open_section {
                out.push('\n');
                open_section = false;
            }
            out.push_str(trimmed);
            out.push('\n');
        } else if trimmed.starts_with('|') {
            if open_section {
                out.push('\n');
                open_section = false;
            }
            out.push_str(trimmed);
            out.push('\n');
        } else if open_section {
            if let Some(status) = trimmed
                .strip_prefix("- **Status**:")
                .or_else(|| trimmed.strip_prefix("**Status**:"))
            {
                out.push_str(&format!(" — {}", status.trim()));
                out.push('\n');
                open_section = false;
            }
        }
    }
    if open_section {
        out.push('\n');
    }
    out
}

/// One-time, idempotent migration of the legacy `.hadron/memory/` lessons
/// ledger into `.hadron/nucleus/`, the swarm's single knowledge root.
///
/// `.hadron/` is gitignored, so this is the ONLY thing that can relocate a
/// user's real on-disk lessons — a quark-worktree `mv` would be invisible to
/// the daemon that actually reads them. Called once at daemon boot, before
/// any turn can run, so there is no race with a quark writing a fresh
/// `nucleus/index.md` before migration sees the legacy one.
pub fn migrate_legacy_memory(workspace_root: &Path) -> std::io::Result<bool> {
    let nucleus_dir = workspace_root.join(".hadron").join("nucleus");
    let legacy_dir = workspace_root.join(".hadron").join("memory");
    if nucleus_dir.join("index.md").exists() {
        return Ok(false);
    }
    if !legacy_dir.join("index.md").exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(&nucleus_dir)?;
    std::fs::rename(legacy_dir.join("index.md"), nucleus_dir.join("index.md"))?;
    let legacy_notes = legacy_dir.join("notes");
    if legacy_notes.exists() {
        std::fs::rename(legacy_notes, nucleus_dir.join("notes"))?;
    }
    Ok(true)
}

#[cfg(test)]
mod nucleus_tests {
    use super::*;

    #[test]
    fn build_nucleus_digest_reads_features_md() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();
        std::fs::write(nucleus.join("features.md"), "## Widget\nstatus: shipped\n").unwrap();
        let digest = build_nucleus_digest(dir.path());
        assert!(digest.contains("Widget"));
    }

    /// The map table is the routing information — name, status and the entrypoint
    /// files a quark needs to find the feature at all — so it survives whole. The
    /// per-component `Logic` prose is the bulk, and it is dropped: rule 9 tells a
    /// quark to open `features.md` before touching a feature, which is where the
    /// detail gets paid for, on the turns that actually need it.
    #[test]
    fn the_features_index_keeps_the_map_and_drops_the_prose() {
        let md = "\
# Hadron Workspace Feature Map

| Feature | Status | Entrypoint |
|---|---|---|
| **Chamber GUI** | Active | `app/mod.rs` |

## Component Logic & Details

### PTY Terminal
- **Status**: Live
- **Files**: `pty.rs`
- **Logic**: a long paragraph nobody needs on a turn that does not touch the PTY.
";
        let index = features_index(md);
        assert!(index.contains("| **Chamber GUI** | Active | `app/mod.rs` |"), "{index}");
        assert!(index.contains("- PTY Terminal — Live"), "{index}");
        assert!(!index.contains("a long paragraph"), "{index}");
        // The property that matters is not "smaller" but "does not grow with the
        // prose" — that is what makes it safe to force-load forever.
        let with_more_prose = format!("{md}- **Logic**: {}\n", "more detail. ".repeat(500));
        assert_eq!(features_index(&with_more_prose), index);
    }

    /// A section with no `Status` line still gets its name — a feature that exists
    /// but is undocumented must not vanish from the map entirely.
    #[test]
    fn a_section_without_a_status_still_appears() {
        assert_eq!(features_index("### Orphan\n- **Files**: x.rs\n"), "- Orphan\n");
    }

    #[test]
    fn build_nucleus_digest_is_empty_when_no_features_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(build_nucleus_digest(dir.path()), "");
    }

    #[test]
    fn migrates_legacy_index_and_notes_into_nucleus() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(".hadron").join("memory");
        std::fs::create_dir_all(legacy.join("notes")).unwrap();
        std::fs::write(legacy.join("index.md"), "- **x** — real content\n").unwrap();
        std::fs::write(legacy.join("notes").join("x.md"), "the long version").unwrap();

        let moved = migrate_legacy_memory(dir.path()).unwrap();
        assert!(moved);
        let nucleus = dir.path().join(".hadron").join("nucleus");
        assert_eq!(
            std::fs::read_to_string(nucleus.join("index.md")).unwrap(),
            "- **x** — real content\n"
        );
        assert_eq!(
            std::fs::read_to_string(nucleus.join("notes").join("x.md")).unwrap(),
            "the long version"
        );
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(".hadron").join("memory");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("index.md"), "old").unwrap();
        assert!(migrate_legacy_memory(dir.path()).unwrap());
        // Second boot: nucleus/index.md now exists — must not touch anything again.
        std::fs::write(legacy.join("index.md"), "should be ignored now").unwrap();
        assert!(!migrate_legacy_memory(dir.path()).unwrap());
        let nucleus = dir.path().join(".hadron").join("nucleus");
        assert_eq!(std::fs::read_to_string(nucleus.join("index.md")).unwrap(), "old");
    }

    #[test]
    fn fresh_project_has_nothing_to_migrate() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!migrate_legacy_memory(dir.path()).unwrap());
    }
}

/// Drives the concurrent coordination loop over a single field file.
///
/// Turns run *in parallel*: every pending target found in one read of the field is
/// dispatched at once (one turn per quark, never two), and the engine only quiesces
/// when the field has no pending work **and** no turn is still in flight.
pub struct Engine {
    field_path: PathBuf,
    quarks: HashMap<QuarkId, Lanes>,
    roster: Vec<QuarkCard>,
    max_exchanges: usize,
    /// Resolved from repo policy (`Team::nucleus_index_budget_kb`) the same way
    /// `max_exchanges` is — read once at boot and on every live team reload, never
    /// re-resolved per turn. Defaults to `nucleus_status::BUDGET_BYTES`.
    nucleus_index_budget_bytes: usize,
    /// Opt-in git safety: target project repo to snapshot/diff. `None` = off.
    repo_root: Option<PathBuf>,
    /// Opt-in nucleus context: pre-rendered digest injected into projections.
    nucleus_digest: String,
    ledger: Option<crate::ledger::Ledger>,
    /// Swarm-wide fallback ceiling for the depletion gate. `None` = **no gate**,
    /// and that is the default: a seat is only cut off when the human set a
    /// limit on it. `Option` rather than `0`-means-off because `is_depleted`
    /// reads `used >= limit`, so a `0` ceiling depletes every quark instantly.
    default_energy_limit: Option<u32>,
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
    /// Quarks whose transport is **resident** (an ACP session that persists across
    /// turns), captured from [`Quark::resident`] at seat time — because the quark then
    /// lives behind an async `Mutex` that a synchronous `projection_for` cannot lock.
    /// Maintained beside `roster`/`quarks` at every seating path (`new`, `seat`,
    /// `unseat`). `projection_for` no longer branches on this for skill injection —
    /// every quark, resident or one-shot, now gets the same index + active-skill-body
    /// shape (WS4 §5, prompt-bloat trim) — but the set is kept for whatever else needs
    /// to know a seat's transport shape.
    resident: HashSet<QuarkId>,
    /// Bookkeeping for [`Kind::Reboot`] servicing: the set of reboot-event **identities**
    /// (ULIDs) this engine has already acted on. `None` until the first field read
    /// baselines it — that first read stamps *every* reboot then present as already-seen,
    /// so reboots that predate the daemon's boot (no live session to kill) are
    /// stale-ignored. Thereafter every read services each `Reboot` whose id is not yet in
    /// the set — exactly once each — and adds it.
    ///
    /// A **set of identities**, deliberately not a positional watermark: `/clear` archives
    /// then truncates the field, so a marker recorded pre-clear vanishes from the file and
    /// a position-based scheme would re-baseline past the fresh post-clear reboots and
    /// service none (the very reboots `/clear` appends to restart every quark). Unique ids
    /// survive truncation: a post-clear reboot has an id not in the set, so it fires
    /// regardless of what the field length did. Growth is one entry per reboot ever
    /// serviced; reboots are human-initiated and rare, so the set stays tiny.
    serviced_reboots: Option<HashSet<Ulid>>,
    /// The **DO-NOT-ACTIVATE** No-Human-Mode toggle (spec §2 D). OFF by default
    /// (see [`env_no_human_mode`]) — when off, every gate call site below passes
    /// `no_human = false`, which `hadron_gatekeeper::decide`/`effective_mode`
    /// guarantee is byte-for-byte today's behavior: `AskOrchestrator` can never
    /// be returned and no worker is ever clamped. Only when this is `true` (env
    /// `HADRON_NO_HUMAN_MODE=1`/`true`, or [`Engine::with_no_human`]) does the
    /// suspend → adjudicate-by-orchestrator → resume loop become reachable.
    no_human: bool,
    /// The **global** custom-skills directory (`~/.hadron/skills` in production),
    /// injected rather than resolved inline — the same seam as [`merge`](Self::merge):
    /// `None` is the hermetic default so `Engine::new` (what every unit test calls)
    /// never touches the real `$HOME`, and only the daemon bin wires the real path via
    /// [`Engine::with_global_skills_dir`]. The **repo** skills dir is not stored here:
    /// it is derived from `workspace_root` fresh in `projection_for`, which is already
    /// tempdir-controlled by every test that sets up a field under a `tempdir()`.
    global_skills_dir: Option<PathBuf>,
    /// The **global** preons directory (`~/.hadron/preons` in production),
    /// injected via the identical seam as [`Engine::global_skills_dir`] just
    /// above, for the identical reason: `None` is the hermetic default, so
    /// `Engine::new` never reads the real `$HOME`, and only the daemon bin opts
    /// a running instance in via [`Engine::with_global_preons_dir`]. The
    /// **repo** preons dir is likewise not stored here — it is derived from
    /// `workspace_root` fresh at each call site that needs it
    /// ([`Engine::loaded_preons`]), same as the repo skills dir.
    global_preons_dir: Option<PathBuf>,
}

/// Parse the DO-NOT-ACTIVATE toggle from `HADRON_NO_HUMAN_MODE`. Read ONCE, at
/// [`Engine::new`] — nothing re-reads the environment after boot, so the toggle
/// cannot flip mid-run out from under an in-flight decision. Default OFF: unset,
/// empty, or anything other than `"1"`/`"true"` (case-insensitive) is OFF. A
/// pure function so it is testable without mutating the process environment
/// (tests that need the toggle ON use [`Engine::with_no_human`] instead, which
/// avoids the cross-test env-var race a `set_var` would create).
fn env_no_human_mode() -> bool {
    std::env::var("HADRON_NO_HUMAN_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
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
                // The @mention name the router matches. Carried on the quark (resolved
                // from the team config at build time), so a re-seat rebuilds the card
                // with the right name instead of silently dropping to id-only.
                display_name: q.display_name(),
                flavor: q.flavor(),
                energy: q.energy(),
                // Populated from the team config in the daemon bin (Task 6);
                // empty here keeps the pure engine independent of seating.
                provider: String::new(),
                model: String::new(),
                // The `@role` roles, carried on the quark exactly like `display_name`
                // above (resolved from the seat at build time), so a re-seat rebuilds
                // the card with the right roles instead of silently dropping them.
                roles: q.roles(),
                exclusive: q.exclusive(),
                // The per-seat command allow/deny lists, carried on the quark exactly
                // like `roles`/`exclusive` above (resolved from the seat at build
                // time), so `Engine::commands_for` can fold them at the `decide()`
                // call sites without re-reading `team.json`.
                commands: q.commands().clone(),
                energy_limit: q.energy_limit(),
                deny_skills: q.deny_skills(),
                has_forge_tools: q.has_forge_tools(),
            })
            .collect();
        let resident = quarks
            .iter()
            .filter(|q| q.resident())
            .map(|q| q.id())
            .collect();
        let quarks = quarks
            .into_iter()
            .map(|q| (q.id(), Lanes { work: Arc::new(AsyncMutex::new(q)) as SharedQuark, chat: None }))
            .collect();
        Engine {
            field_path,
            quarks,
            roster,
            max_exchanges,
            nucleus_index_budget_bytes: crate::nucleus_status::BUDGET_BYTES,
            repo_root: None,
            nucleus_digest: String::new(),
            ledger: None,
            default_energy_limit: None,
            merge: None,
            field_lock: Arc::new(AsyncMutex::new(())),
            turn_deadline: TURN_DEADLINE,
            disabled: HashSet::new(),
            resident,
            serviced_reboots: None,
            no_human: env_no_human_mode(),
            // Hermetic default: `Engine::new` never reads the real `~/.hadron` — only
            // `with_global_skills_dir` (called by the daemon bin) opts a running
            // instance into it. See the field doc for why this mirrors `merge`.
            global_skills_dir: None,
            // Same hermetic default, same reason — see `global_preons_dir`'s field doc.
            global_preons_dir: None,
        }
    }

    /// Explicitly set the No-Human-Mode toggle, overriding whatever
    /// [`env_no_human_mode`] read at construction. The daemon bin can wire this
    /// to a `team.json`/config field later; tests use it to exercise the ON path
    /// deterministically, without the cross-test race a shared-process
    /// `std::env::set_var` would introduce.
    pub fn with_no_human(mut self, on: bool) -> Self {
        self.no_human = on;
        self
    }

    /// Whether the No-Human-Mode toggle is on. The daemon bin uses this purely
    /// to print a loud startup warning — a SECURITY-SENSITIVE mode being on
    /// should never be silent.
    pub fn no_human(&self) -> bool {
        self.no_human
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
        let resident = quark.resident();
        let card = QuarkCard {
            id: id.clone(),
            // The @mention name, carried on the quark (see `Engine::new`) so a runtime
            // re-seat keeps the seat routable by name, not only by id.
            display_name: quark.display_name(),
            flavor: quark.flavor(),
            energy: quark.energy(),
            // Left empty exactly as `new` leaves it — the daemon owns legibility, and
            // a seat added at runtime must not acquire fields a booted one lacks.
            provider: String::new(),
            model: String::new(),
            // Carried on the quark exactly as `Engine::new` reads it (see above).
            roles: quark.roles(),
            exclusive: quark.exclusive(),
            commands: quark.commands().clone(),
            energy_limit: quark.energy_limit(),
            deny_skills: quark.deny_skills(),
            has_forge_tools: quark.has_forge_tools(),
        };
        self.roster.retain(|c| c.id != id);
        self.roster.push(card);
        // Track residency beside the roster (a replacement may flip transport).
        if resident {
            self.resident.insert(id.clone());
        } else {
            self.resident.remove(&id);
        }
        // A replacement's chat lane, if it has one, survives — only the work
        // lane instance changes. A brand-new seat starts with no chat lane;
        // `seat_chat_lane` is how one is attached.
        let work: SharedQuark = Arc::new(AsyncMutex::new(quark));
        match self.quarks.get_mut(&id) {
            Some(lanes) => lanes.work = work,
            None => {
                self.quarks.insert(id, Lanes { work, chat: None });
            }
        }
    }

    /// Give an already-seated seat a second, **chat-only** lane instance —
    /// the ACP-orchestrator-specific half of Task 6 (Step 4). Built through
    /// the same construction path as the work lane (the caller, `cli.rs`'s
    /// seating loop, calls `registry::build_seat_watched` a second time for
    /// an `Flavor::Orchestrator` card); this method only attaches the result.
    ///
    /// Deliberately does NOT touch `roster` or `resident` — the chat lane is
    /// invisible to every id-keyed lookup by design (Task 6 Step 4's whole
    /// point). A no-op (`false`) for an id that is not seated: a chat lane
    /// with no work lane to route `Work` traffic to makes no sense.
    pub fn seat_chat_lane(&mut self, id: &QuarkId, quark: Box<dyn Quark>) -> bool {
        let Some(lanes) = self.quarks.get_mut(id) else {
            return false;
        };
        lanes.chat = Some(Arc::new(AsyncMutex::new(quark)));
        true
    }

    /// Remove a quark from the live roster. `true` if it was seated.
    ///
    /// Dropping the last `Arc` to its `SharedQuark` drops the adapter, which for an ACP
    /// seat takes its resident subprocess down with it. That is safe only because no
    /// turn holds a clone of that `Arc` at a quiescent point — see [`Engine::seat`].
    pub fn unseat(&mut self, id: &QuarkId) -> bool {
        self.roster.retain(|c| &c.id != id);
        self.resident.remove(id);
        self.quarks.remove(id).is_some()
    }

    /// Set the maximum number of exchanges allowed before triggering the backstop.
    pub fn set_max_exchanges(&mut self, max: usize) {
        self.max_exchanges = max;
    }

    /// Set the resolved nucleus index budget (bytes) that every projection from
    /// this point on carries. Same live-reload seam as [`Engine::set_max_exchanges`]:
    /// the daemon bin calls this when `team.json` changes, so a repo policy edit
    /// takes effect on the very next turn without a restart.
    pub fn set_nucleus_index_budget_bytes(&mut self, bytes: usize) {
        self.nucleus_index_budget_bytes = bytes;
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

    /// Update a seated quark's `@mention` name **without rebuilding it**. A rename is pure
    /// metadata on the roster card the router reads; the quark instance — and, for an ACP
    /// seat, its live session — is untouched. Only the label the router matches changes, so
    /// `@NewName` starts resolving on the next tick. Unknown ids are a no-op.
    pub fn rename(&mut self, id: &QuarkId, display_name: Option<String>) {
        if let Some(card) = self.roster.iter_mut().find(|c| &c.id == id) {
            card.display_name = display_name;
        }
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

    /// Opt in to loading custom skills from a **global** directory (production:
    /// `~/.hadron/skills`, via [`hadron_lattice::user_hadron_dir`]). `None` — the
    /// default from [`Engine::new`] — means no global directory is consulted, so a
    /// test-constructed engine can never read the real `$HOME`. Only the daemon bin
    /// calls this, with the real path; every engine test either leaves it unset or
    /// passes a tempdir, exactly like [`Engine::with_merge_gate`] and
    /// [`Engine::with_no_human`] above.
    pub fn with_global_skills_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.global_skills_dir = dir;
        self
    }

    /// Opt in to loading custom preons from a **global** directory (production:
    /// `~/.hadron/preons`, via [`hadron_lattice::user_hadron_dir`]). The identical
    /// seam as [`Engine::with_global_skills_dir`] just above, for the identical
    /// reason: `None` — the default from [`Engine::new`] — means no global
    /// directory is consulted, so a test-constructed engine can never read the
    /// real `$HOME`. Only the daemon bin calls this, with the real path.
    pub fn with_global_preons_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.global_preons_dir = dir;
        self
    }

    /// The roster of seated quarks.
    pub fn roster(&self) -> &[hadron_lattice::QuarkCard] {
        &self.roster
    }

    /// The one way the engine writes to the field: serialized behind `field_lock`,
    /// so concurrent turns can never interleave their event sequences.
    pub async fn append(&self, event: Event) -> anyhow::Result<()> {
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

    /// Opt in to the energy ledger. `limit` is the swarm-wide fallback ceiling;
    /// pass `None` to record spend without ever gating on it (see
    /// [`Engine::default_energy_limit`]).
    pub fn with_ledger(mut self, ledger: crate::ledger::Ledger, limit: Option<u32>) -> Self {
        self.ledger = Some(ledger);
        self.default_energy_limit = limit;
        self
    }

    /// Opt in to nucleus context: the pre-rendered digest is injected into
    /// every projection. Build it with [`build_nucleus_digest`].
    pub fn with_nucleus(mut self, digest: String) -> Self {
        self.nucleus_digest = digest;
        self
    }

    /// Boot-time counterpart to [`Engine::set_nucleus_index_budget_bytes`] — same
    /// chained-builder shape as [`Engine::with_nucleus`], for the daemon bin's
    /// initial construction.
    pub fn with_nucleus_index_budget_bytes(mut self, bytes: usize) -> Self {
        self.nucleus_index_budget_bytes = bytes;
        self
    }

    /// The preons corpus for THIS call — global (injected, see
    /// [`Engine::global_preons_dir`]) merged with `<workspace>/.hadron/preons`
    /// (repo), loaded fresh rather than cached.
    ///
    /// Unlike the skill corpus (loaded once inside `projection_for`, because
    /// that is the one place it's used), preons are consumed at several
    /// distinct router call sites — [`Engine::human_addressees`],
    /// [`Engine::exclusive_task_names_target`], and `finish_turn`'s quark→quark
    /// hand-off resolution — none of which sit inside a projection. So this is
    /// called once per *site*, not once per projection; the cost is the same
    /// handful of small file reads as skills, well short of a hot loop. `Engine::new`
    /// defaults `global_preons_dir` to `None`, so a test-constructed engine can
    /// never read the real `~/.hadron/preons`; the repo half is derived from
    /// `workspace_root`, which every test already controls via its own tempdir
    /// field — a missing `.hadron/preons` directory degrades to `[]`, same as skills.
    fn loaded_preons(&self) -> Vec<Preon> {
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let workspace_root = workspace_root_of(&self.field_path, &base);
        let repo_preons_dir = workspace_root.join(".hadron").join("preons");
        preons::load_preons(self.global_preons_dir.as_deref(), Some(&repo_preons_dir))
    }

    fn loaded_roles(&self) -> Vec<Preon> {
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let workspace_root = workspace_root_of(&self.field_path, &base);
        let repo_roles_dir = workspace_root.join(".hadron").join("roles");
        let global_roles_dir = self.global_preons_dir.as_deref().and_then(|p| p.parent()).map(|p| p.join("roles"));
        preons::load_roles(global_roles_dir.as_deref(), Some(&repo_roles_dir))
    }

}
