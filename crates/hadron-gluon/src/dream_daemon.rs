//! "Sleep & Dream" Idle Autonomous Daemon.
//!
//! Executes low-priority background maintenance during workspace idle periods:
//! empty-note cleanup, nucleus index integrity checks (orphan notes, dead index
//! links, duplicate index slugs), cold-note detection from `usage.json`, and
//! stale worktree detection without interrupting active quark turns.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Default age threshold (in days) after which an unopened note is reported as cold.
pub const DEFAULT_COLD_NOTE_DAYS: u64 = 30;

/// Minimum interval (in seconds) between automatic idle `DreamDaemon` safe-point cycles.
pub const DREAM_IDLE_INTERVAL_SECS: u64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamSummary {
    pub notes_scanned: usize,
    pub notes_cleaned: usize,
    pub stale_worktrees_found: usize,
    #[serde(default)]
    pub orphan_notes: Vec<String>,
    #[serde(default)]
    pub dead_index_links: Vec<String>,
    #[serde(default)]
    pub duplicate_index_slugs: Vec<String>,
    #[serde(default)]
    pub cold_notes: Vec<String>,
    pub timestamp: u64,
}

pub struct DreamDaemon;

impl DreamDaemon {
    /// Runs a single dream maintenance cycle on the repository using the current
    /// wall clock and [`DEFAULT_COLD_NOTE_DAYS`].
    pub fn run_cycle(repo_root: &Path) -> DreamSummary {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self::run_cycle_at(repo_root, now, DEFAULT_COLD_NOTE_DAYS)
    }

    /// Deterministic entrypoint for running a dream maintenance cycle at a specific
    /// epoch timestamp and cold-note day threshold.
    ///
    /// Safe by default: deletes ONLY 0-byte empty note files; orphan notes, dead
    /// links, duplicate index slugs, and cold notes are reported non-destructively.
    pub fn run_cycle_at(repo_root: &Path, now_epoch_secs: u64, cold_note_days: u64) -> DreamSummary {
        let mut notes_scanned = 0;
        let mut notes_cleaned = 0;
        let mut stale_worktrees_found = 0;

        let nucleus_root = repo_root.join(".hadron/nucleus");
        let notes_dir = nucleus_root.join("notes");
        let index_path = nucleus_root.join("index.md");

        // 1. Inspect nucleus notes on disk (clean 0-byte empty notes; record surviving slugs + mtimes)
        let mut surviving_notes: BTreeMap<String, Option<u64>> = BTreeMap::new();
        if let Ok(entries) = fs::read_dir(&notes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "md") {
                    notes_scanned += 1;
                    if let Ok(meta) = fs::metadata(&path) {
                        if meta.len() == 0 {
                            if fs::remove_file(&path).is_ok() {
                                notes_cleaned += 1;
                            }
                            continue;
                        }
                        if let Some(slug) = path.file_stem().and_then(|s| s.to_str()) {
                            let mtime_secs = meta
                                .modified()
                                .ok()
                                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                                .map(|d| d.as_secs());
                            surviving_notes.insert(slug.to_string(), mtime_secs);
                        }
                    }
                }
            }
        }

        // 2. Parse `.hadron/nucleus/index.md` for `notes/<slug>.md` links
        let mut index_slug_counts: BTreeMap<String, usize> = BTreeMap::new();
        if let Ok(index_md) = fs::read_to_string(&index_path) {
            for slug in parse_index_note_slugs(&index_md) {
                *index_slug_counts.entry(slug).or_insert(0) += 1;
            }
        }

        let indexed_slugs: BTreeSet<&str> = index_slug_counts.keys().map(String::as_str).collect();

        let duplicate_index_slugs: Vec<String> = index_slug_counts
            .iter()
            .filter_map(|(slug, &count)| if count > 1 { Some(slug.clone()) } else { None })
            .collect();

        let dead_index_links: Vec<String> = index_slug_counts
            .keys()
            .filter(|slug| !surviving_notes.contains_key(*slug))
            .cloned()
            .collect();

        let orphan_notes: Vec<String> = surviving_notes
            .keys()
            .filter(|slug| !indexed_slugs.contains(slug.as_str()))
            .cloned()
            .collect();

        // 3. Read `<nucleus_root>/usage.json` via canonical `hadron-lattice` SSOT loader
        //    (returns default v1 when missing; returns Err on forward schema_version > 1 or malformed JSON).
        let cold_notes = match hadron_lattice::nucleus_usage::load_usage_ledger(&nucleus_root) {
            Ok(ledger) => {
                let threshold_secs = cold_note_days.saturating_mul(86_400);
                surviving_notes
                    .iter()
                    .filter_map(|(slug, mtime_opt)| {
                        let mtime = (*mtime_opt)?;
                        let age = now_epoch_secs.saturating_sub(mtime);
                        let opens = ledger.notes.get(slug).map(|e| e.opens).unwrap_or(0);
                        if age >= threshold_secs && opens == 0 {
                            Some(slug.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            Err(_) => Vec::new(),
        };

        // 4. Inspect worktrees directory
        let trees_dir = repo_root.join(".hadron/trees");
        if let Ok(entries) = fs::read_dir(&trees_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && !path.join(".git").exists() {
                    stale_worktrees_found += 1;
                }
            }
        }

        DreamSummary {
            notes_scanned,
            notes_cleaned,
            stale_worktrees_found,
            orphan_notes,
            dead_index_links,
            duplicate_index_slugs,
            cold_notes,
            timestamp: now_epoch_secs,
        }
    }
}

/// Extract all `notes/<slug>.md` link targets from `index.md` in document order.
fn parse_index_note_slugs(index_md: &str) -> Vec<String> {
    let re = Regex::new(r"notes/([A-Za-z0-9][A-Za-z0-9._-]*)\.md")
        .expect("static note link regex");
    re.captures_iter(index_md)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_dream_daemon_cycle_cleans_empty_and_detects_stale_trees() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let nucleus = root.join(".hadron/nucleus");
        let notes = nucleus.join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("good-note.md"), b"valid note content").unwrap();
        fs::write(notes.join("empty-note.md"), b"").unwrap(); // should be cleaned
        fs::write(
            nucleus.join("index.md"),
            "- [good-note](notes/good-note.md) — valid lesson\n",
        )
        .unwrap();

        let trees = root.join(".hadron/trees");
        fs::create_dir_all(trees.join("corrupted-tree")).unwrap(); // no .git, marked stale

        let summary = DreamDaemon::run_cycle(root);
        assert_eq!(summary.notes_scanned, 2);
        assert_eq!(summary.notes_cleaned, 1);
        assert_eq!(summary.stale_worktrees_found, 1);
        assert!(summary.orphan_notes.is_empty());
        assert!(summary.dead_index_links.is_empty());
        assert!(summary.duplicate_index_slugs.is_empty());
        assert!(!notes.join("empty-note.md").exists());
        assert!(notes.join("good-note.md").exists());
    }

    #[test]
    fn test_dream_daemon_detects_orphans_dead_links_and_duplicates_non_destructively() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let nucleus = root.join(".hadron/nucleus");
        let notes = nucleus.join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("linked.md"), b"linked note").unwrap();
        fs::write(notes.join("orphan-alpha.md"), b"unindexed note").unwrap();
        fs::write(
            nucleus.join("index.md"),
            concat!(
                "- [linked](notes/linked.md) — first entry\n",
                "- [linked](notes/linked.md) — duplicate entry\n",
                "- [missing-note](notes/missing-note.md) — dead link\n",
            ),
        )
        .unwrap();

        let summary = DreamDaemon::run_cycle(root);
        assert_eq!(summary.notes_scanned, 2);
        assert_eq!(summary.notes_cleaned, 0);
        assert_eq!(summary.orphan_notes, vec!["orphan-alpha".to_string()]);
        assert_eq!(summary.dead_index_links, vec!["missing-note".to_string()]);
        assert_eq!(summary.duplicate_index_slugs, vec!["linked".to_string()]);
        // Non-empty orphan notes must NEVER be deleted automatically.
        assert!(notes.join("orphan-alpha.md").exists());
    }

    #[test]
    fn test_dream_daemon_detects_cold_notes_and_respects_usage_schema_guard() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let nucleus = root.join(".hadron/nucleus");
        let notes = nucleus.join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("hot-note.md"), b"opened recently").unwrap();
        fs::write(notes.join("cold-note.md"), b"never opened").unwrap();
        fs::write(
            nucleus.join("index.md"),
            "- [hot-note](notes/hot-note.md) — hot\n- [cold-note](notes/cold-note.md) — cold\n",
        )
        .unwrap();
        fs::write(
            nucleus.join("usage.json"),
            r#"{"schema_version":1,"notes":{"hot-note":{"opens":3,"last_opened_epoch_secs":1700000000},"cold-note":{"opens":0,"last_opened_epoch_secs":0}}}"#,
        )
        .unwrap();

        let future_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + (DEFAULT_COLD_NOTE_DAYS + 1) * 86_400;

        let summary = DreamDaemon::run_cycle_at(root, future_now, DEFAULT_COLD_NOTE_DAYS);
        assert_eq!(summary.cold_notes, vec!["cold-note".to_string()]);
        assert!(notes.join("cold-note.md").exists(), "cold notes must not be deleted");

        // Forward schema_version > 1 must fail safe and skip cold-note flagging.
        fs::write(
            nucleus.join("usage.json"),
            r#"{"schema_version":2,"notes":{}}"#,
        )
        .unwrap();
        let guarded = DreamDaemon::run_cycle_at(root, future_now, DEFAULT_COLD_NOTE_DAYS);
        assert!(guarded.cold_notes.is_empty());
    }
}
