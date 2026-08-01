//! What a quark is doing **right now**.
//!
//! The field (`field.jsonl`) is the permanent record: what was said, what was
//! spent, what was edited. This is the opposite — a **volatile** view of a turn
//! in flight, so the chamber can show "opus is reading engine.rs" while it
//! happens.
//!
//! WHY NOT AN `Event`: an agent emits a thought chunk every few tokens. Writing
//! those to the field would append tens of thousands of lines per turn to a file
//! that is replayed on every daemon start and rendered by the chamber — the
//! stream is worth *seeing* and not worth *keeping*. So each quark gets one small
//! file that is overwritten in place: `<field-dir>/live/<quark>.json`.
//!
//! **Absence is idle.** There is deliberately no `Doing::Idle` variant — a quark
//! that is not working has no file. That makes "working on nothing" unrepresentable
//! rather than something every reader has to remember to filter out.
//!
//! **A stale file is not a working quark.** If the daemon is killed mid-turn the
//! file survives, and a naive reader would show that quark as thinking forever.
//! [`Activity::is_fresh`] is the guard, and readers must use it.

use crate::QuarkId;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How long an activity is believed after it was written. A turn can sit on a
/// single long tool call, so this is generous — it exists to bound a *dead*
/// daemon's last words, not to expire a slow one.
pub const STALE_AFTER_SECS: i64 = 120;

/// The maximum length of the human-readable detail line, in **characters**.
///
/// Characters, not bytes: a byte truncation lands mid-codepoint and panics the
/// renderer. That crash has been paid for once already in this codebase.
pub const DETAIL_CHARS: usize = 200;

/// What kind of work a turn is doing. Deliberately coarse — this drives an icon
/// and a colour, not a debugger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Doing {
    /// Reasoning. The agent's internal thought stream.
    Thinking,
    /// Running a tool — reading, editing, executing.
    Working,
    /// Laying out a plan for a multi-step task.
    Planning,
    /// Composing the reply that will land in the field.
    Speaking,
    /// The merge gate is rebasing/testing/landing a branch. Not a seat thinking —
    /// see [`gates_dir`] for why this never rides a quark's own live file.
    Gating,
}

impl Doing {
    /// A stable label for a UI that has no icon set yet.
    pub fn label(self) -> &'static str {
        match self {
            Doing::Thinking => "thinking",
            Doing::Working => "working",
            Doing::Planning => "planning",
            Doing::Speaking => "speaking",
            Doing::Gating => "gating",
        }
    }
}

/// One quark's current activity. Overwritten in place many times per turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub quark: QuarkId,
    pub at: DateTime<Utc>,
    pub doing: Doing,
    /// A short human-readable line: the tool's title, or the tail of the thought.
    pub detail: String,
    /// The **message so far**, uncapped and with its newlines intact — set only by
    /// [`Activity::speaking`], `None` for every other kind of activity.
    ///
    /// This is the streaming reply, and it is deliberately NOT `detail`: `detail` is
    /// one truncated line for the Live card, while a chat bubble needs the whole
    /// markdown. It rides this file rather than `field.jsonl` because the field is
    /// what `router::next_pending` dispatches from — a partial message whose text
    /// happens to begin a line with `@peer` would excite that peer mid-turn and again
    /// when the real message lands. A live file is volatile, unrouted, and already
    /// cleared when the turn ends, which is exactly the lifetime a draft wants.
    ///
    /// `serde(default)` so a live file written by an older build still parses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,
    /// When a [`Doing::Gating`] run actually STARTED, set once by [`Activity::gating`]
    /// and republished unchanged on every heartbeat. `at` still advances each tick — it
    /// is what keeps the file fresh — so without a separate start time the Tasks tab's
    /// elapsed column would reset to ~0 every 30s instead of counting from when the
    /// gate began. `None` for every other kind of activity; `serde(default)` so an
    /// older live file still parses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<DateTime<Utc>>,
}

impl Activity {
    /// Build an activity, truncating the detail to [`DETAIL_CHARS`] **characters**
    /// and collapsing its newlines — it is rendered as one line.
    pub fn new(quark: QuarkId, doing: Doing, detail: &str) -> Self {
        Activity {
            quark,
            at: Utc::now(),
            doing,
            detail: one_line(detail),
            full: None,
            started: None,
        }
    }

    /// The reply as it streams in: [`Doing::Speaking`] carrying `message` whole in
    /// [`Activity::full`].
    ///
    /// `detail` is left EMPTY on purpose. A reader that shows `detail` is the Live
    /// card, whose job is what the quark is *doing* — the message itself belongs in
    /// the chat, and duplicating it into both would say the same thing twice. Empty
    /// already means "show the `doing` label" to every existing reader, so the Live
    /// card reads `speaking` and needs no special case.
    pub fn speaking(quark: QuarkId, message: &str) -> Self {
        Activity {
            quark,
            at: Utc::now(),
            doing: Doing::Speaking,
            detail: String::new(),
            full: Some(message.to_string()),
            started: None,
        }
    }

    /// A merge gate's heartbeat: `quark` is who the branch belongs to (a field on the
    /// payload, never part of the file's name — see [`gates_dir`]), `branch` becomes
    /// `detail`, and `started` is the gate's real start time, carried unchanged through
    /// every republish so the elapsed column doesn't reset each tick.
    pub fn gating(quark: QuarkId, branch: &str, started: DateTime<Utc>) -> Self {
        Activity {
            quark,
            at: Utc::now(),
            doing: Doing::Gating,
            detail: one_line(branch),
            full: None,
            started: Some(started),
        }
    }

    /// Is this activity recent enough to believe? A file left behind by a killed
    /// daemon must not render as a quark that is still working.
    pub fn is_fresh(&self, now: DateTime<Utc>) -> bool {
        now.signed_duration_since(self.at) < Duration::seconds(STALE_AFTER_SECS)
    }
}

/// Squash to a single line and cap the length, on a character boundary.
fn one_line(s: &str) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.chars().count() <= DETAIL_CHARS {
        return trimmed;
    }
    // `take` counts characters, so this can never split a codepoint.
    let mut out: String = trimmed.chars().take(DETAIL_CHARS.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The directory that holds the live files, derived from the field path so that
/// both processes agree without a second setting: `<field-dir>/live`.
pub fn live_dir(field: &Path) -> PathBuf {
    crate::hadron_dir_of(field).join("live")
}

fn file_for(dir: &Path, quark: &QuarkId) -> PathBuf {
    dir.join(format!("{}.json", quark.as_str()))
}

/// Publish what this quark is doing. Atomic (temp + rename), so a reader can
/// never observe a half-written file — the same rule `save_team` learned.
pub fn publish(dir: &Path, activity: &Activity) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let final_path = file_for(dir, &activity.quark);
    let tmp = final_path.with_extension("json.tmp");
    let body = serde_json::to_string(activity)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &final_path)
}

/// The quark has stopped working. Absence is idle, so this removes the file.
/// Missing is success — the caller wants the quark to be idle, and it is.
pub fn clear(dir: &Path, quark: &QuarkId) -> std::io::Result<()> {
    match std::fs::remove_file(file_for(dir, quark)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Read one quark's activity. `None` means idle — which includes a file that is
/// unreadable, malformed, or **stale**: none of those are a working quark.
pub fn read(dir: &Path, quark: &QuarkId, now: DateTime<Utc>) -> Option<Activity> {
    let body = std::fs::read_to_string(file_for(dir, quark)).ok()?;
    let activity: Activity = serde_json::from_str(&body).ok()?;
    activity.is_fresh(now).then_some(activity)
}

/// Where a merge gate's in-flight heartbeats live: a **subdirectory** of `live/`, not
/// a sibling file. A gate is not a seat — it is keyed by branch, not by quark — and an
/// orchestrator's Work and Chat lanes already share one `live/<id>.json`
/// (`engine.rs`'s `Lanes` doc). Publishing a gate there would let its heartbeat read as
/// a concurrently-watched lane's sign of life (masking a real hang) and would
/// wholesale-overwrite `Activity.full` (killing a streaming chat draft), since
/// `merge_gate` has no `LiveFeed` to re-attach it. The subdirectory means `read`/
/// `publish`/`clear` above — which only ever look up `<quark>.json` by exact name —
/// need no change at all, and a `read_dir` of `live/` can never mistake a gate file
/// for a quark's.
pub fn gates_dir(field: &Path) -> PathBuf {
    live_dir(field).join("gates")
}

/// Sanitised so a branch name (which contains `/`) is a single path component.
fn gate_file_for(dir: &Path, branch: &str) -> PathBuf {
    dir.join(format!("{}.json", branch.replace('/', "-")))
}

/// Publish a merge gate's heartbeat, keyed by `branch` rather than `activity.quark` —
/// see [`gates_dir`]. Atomic, same as [`publish`].
pub fn publish_gate(dir: &Path, branch: &str, activity: &Activity) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let final_path = gate_file_for(dir, branch);
    let tmp = final_path.with_extension("json.tmp");
    let body = serde_json::to_string(activity)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &final_path)
}

/// The gate has stopped running. Missing is success, same as [`clear`].
pub fn clear_gate(dir: &Path, branch: &str) -> std::io::Result<()> {
    match std::fs::remove_file(gate_file_for(dir, branch)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Every gate currently believed to be running — fresh ones only, same guard as
/// [`read`]. An unreadable or malformed file is skipped rather than failing the whole
/// read: one corrupt heartbeat must not hide every other gate.
pub fn gates(dir: &Path, now: DateTime<Utc>) -> Vec<Activity> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|body| serde_json::from_str::<Activity>(&body).ok())
        .filter(|a| a.is_fresh(now))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("hadron-live-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn the_live_dir_sits_beside_the_field() {
        let field = Path::new("/home/jake/dev/hadron/.hadron/field.jsonl");
        assert_eq!(
            live_dir(field),
            PathBuf::from("/home/jake/dev/hadron/.hadron/live")
        );
    }

    #[test]
    fn an_activity_round_trips() {
        let dir = tmp();
        let id = QuarkId::new("opus");
        let a = Activity::new(id.clone(), Doing::Working, "Read engine.rs");
        publish(&dir, &a).unwrap();

        let back = read(&dir, &id, Utc::now()).expect("just published");
        assert_eq!(back.doing, Doing::Working);
        assert_eq!(back.detail, "Read engine.rs");
    }

    /// Absence is idle: no file, no activity. The reader must not invent one.
    #[test]
    fn an_absent_file_is_idle() {
        let dir = tmp();
        assert_eq!(read(&dir, &QuarkId::new("nobody"), Utc::now()), None);
    }

    /// Clearing is what ends a turn, and clearing something already absent is
    /// not an error — the caller wanted it idle, and it is.
    #[test]
    fn clearing_makes_a_quark_idle_and_is_idempotent() {
        let dir = tmp();
        let id = QuarkId::new("agy");
        publish(&dir, &Activity::new(id.clone(), Doing::Thinking, "…")).unwrap();
        assert!(read(&dir, &id, Utc::now()).is_some());

        clear(&dir, &id).unwrap();
        assert_eq!(read(&dir, &id, Utc::now()), None);
        clear(&dir, &id).expect("clearing an idle quark is not an error");
    }

    /// **The guard that matters.** A daemon killed mid-turn leaves the file
    /// behind; without this the chamber shows that quark thinking forever.
    #[test]
    fn a_stale_activity_reads_as_idle() {
        let dir = tmp();
        let id = QuarkId::new("ghost");
        publish(&dir, &Activity::new(id.clone(), Doing::Thinking, "left behind")).unwrap();

        let long_after = Utc::now() + Duration::seconds(STALE_AFTER_SECS + 1);
        assert_eq!(
            read(&dir, &id, long_after),
            None,
            "a killed daemon's last words are not a working quark"
        );
    }

    /// Byte-slicing a multi-byte character panics the renderer. This is the one
    /// crash Jake has actually hit, so the cap counts characters.
    #[test]
    fn a_long_multibyte_detail_is_cut_on_a_character_boundary() {
        let detail = "🚀".repeat(DETAIL_CHARS * 2);
        let a = Activity::new(QuarkId::new("q"), Doing::Speaking, &detail);
        assert_eq!(a.detail.chars().count(), DETAIL_CHARS);
        assert!(a.detail.ends_with('…'));
    }

    /// The detail is rendered as a single line; a thought stream is full of
    /// newlines, and they must not blow the row height open.
    #[test]
    fn a_multi_line_detail_is_flattened() {
        let a = Activity::new(QuarkId::new("q"), Doing::Thinking, "one\n\ntwo\tthree  ");
        assert_eq!(a.detail, "one two three");
    }

    /// A streaming reply keeps its newlines and its length — it is rendered as a
    /// markdown chat bubble, not as the Live card's one-line status. `detail` stays
    /// empty so the Live card falls back to the `speaking` label instead of printing
    /// the same text a second time.
    #[test]
    fn a_speaking_draft_carries_the_whole_message_and_no_detail() {
        let body = "First paragraph.\n\n@peer second paragraph, ".to_string()
            + &"long ".repeat(DETAIL_CHARS);
        let a = Activity::speaking(QuarkId::new("q"), &body);
        assert_eq!(a.doing, Doing::Speaking);
        assert_eq!(a.detail, "", "the message belongs in the chat, not in the Live card");
        assert_eq!(a.full.as_deref(), Some(body.as_str()), "uncapped and unflattened");
    }

    /// The gate's payload round-trips through serde as `"gating"`, and carries no
    /// draft — a gate has nothing streaming to a chat bubble.
    #[test]
    fn a_gating_activity_serialises_as_gating_and_carries_no_draft() {
        let started = Utc::now() - Duration::seconds(5);
        let a = Activity::gating(QuarkId::new("w"), "quark/w/some-branch", started);
        assert_eq!(a.doing, Doing::Gating);
        assert_eq!(Doing::Gating.label(), "gating");
        assert_eq!(a.detail, "quark/w/some-branch");
        assert_eq!(a.full, None);
        assert_eq!(a.started, Some(started));

        let body = serde_json::to_string(&a).unwrap();
        assert!(body.contains("\"gating\""));
        let back: Activity = serde_json::from_str(&body).unwrap();
        assert_eq!(back, a);
    }

    /// A gate is keyed by BRANCH, one file per branch in its own subdirectory —
    /// never a quark's own `<id>.json`, which is the whole point of `gates_dir`.
    #[test]
    fn a_published_gate_is_found_by_gates_and_lives_in_its_own_subdirectory() {
        let field = tmp().join(".hadron").join("field.jsonl");
        let dir = gates_dir(&field);
        assert_eq!(dir, live_dir(&field).join("gates"));

        let started = Utc::now();
        let a = Activity::gating(QuarkId::new("w"), "quark/w/fix-the-thing", started);
        publish_gate(&dir, "quark/w/fix-the-thing", &a).unwrap();

        let found = gates(&dir, Utc::now());
        assert_eq!(found, vec![a]);
    }

    /// **The guard that matters.** A gate must never be readable as a quark's own
    /// sign of life — `until_silent` (the turn watchdog) and `LiveFeed` (the
    /// streaming chat draft) both look up `<quark>.json` by exact name, and the
    /// separate subdirectory is what keeps a gate invisible to both.
    #[test]
    fn a_running_gate_does_not_feed_the_turn_watchdog() {
        let field = tmp().join(".hadron").join("field.jsonl");
        let quark = QuarkId::new("orch");
        let a = Activity::gating(quark.clone(), "quark/orch/some-branch", Utc::now());
        publish_gate(&gates_dir(&field), "quark/orch/some-branch", &a).unwrap();

        assert_eq!(
            read(&live_dir(&field), &quark, Utc::now()),
            None,
            "a gate heartbeat must never be visible to the quark's own live file"
        );
    }

    /// Same staleness guard as a quark's own activity: a gate whose heartbeat has
    /// stopped (a killed daemon) must not read as one still running.
    #[test]
    fn a_stale_gate_is_not_returned_by_gates() {
        let dir = tmp();
        let a = Activity::gating(QuarkId::new("w"), "quark/w/old", Utc::now());
        publish_gate(&dir, "quark/w/old", &a).unwrap();

        let long_after = Utc::now() + Duration::seconds(STALE_AFTER_SECS + 1);
        assert!(gates(&dir, long_after).is_empty());
    }

    /// Clearing an already-absent gate is not an error — the caller wanted it gone,
    /// and it is. Same idempotence as [`clear`].
    #[test]
    fn clearing_a_gate_is_idempotent() {
        let dir = tmp();
        clear_gate(&dir, "quark/w/never-published").expect("clearing an absent gate is not an error");

        let a = Activity::gating(QuarkId::new("w"), "quark/w/x", Utc::now());
        publish_gate(&dir, "quark/w/x", &a).unwrap();
        clear_gate(&dir, "quark/w/x").unwrap();
        assert!(gates(&dir, Utc::now()).is_empty());
    }

    /// The draft rides the SAME file, so it inherits the staleness guard and the
    /// atomic write — and a live file written by a build that predates `full` must
    /// still parse rather than reading as an idle quark.
    #[test]
    fn a_draft_round_trips_and_an_older_file_without_full_still_parses() {
        let dir = tmp();
        let id = QuarkId::new("opus");
        publish(&dir, &Activity::speaking(id.clone(), "half a sen")).unwrap();
        let back = read(&dir, &id, Utc::now()).expect("just published");
        assert_eq!(back.full.as_deref(), Some("half a sen"));

        let legacy = r#"{"quark":"opus","at":"2026-08-01T00:00:00Z","doing":"working","detail":"Read x.rs"}"#;
        let parsed: Activity = serde_json::from_str(legacy).expect("an older live file still parses");
        assert_eq!(parsed.full, None);
    }
}
