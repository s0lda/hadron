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
}

impl Doing {
    /// A stable label for a UI that has no icon set yet.
    pub fn label(self) -> &'static str {
        match self {
            Doing::Thinking => "thinking",
            Doing::Working => "working",
            Doing::Planning => "planning",
            Doing::Speaking => "speaking",
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
}
