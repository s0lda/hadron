//! Nucleus lesson usage ledger: how often each `notes/<slug>.md` was opened, and when.
//!
//! Stored at `<nucleus_root>/usage.json`. Fed by `hadron-forge-mcp` when a quark queries
//! or reads a note; read by the `DreamDaemon` to report cold lessons. Telemetry, not a
//! source of truth: concurrent writers from different quark processes may drop an
//! increment (last rename wins), but can never leave a torn file.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const NUCLEUS_USAGE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteUsageEntry {
    pub opens: u64,
    pub last_opened_epoch_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_quark: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NucleusUsageLedger {
    pub schema_version: u32,
    #[serde(default)]
    pub notes: BTreeMap<String, NoteUsageEntry>,
}

impl Default for NucleusUsageLedger {
    fn default() -> Self {
        Self { schema_version: NUCLEUS_USAGE_SCHEMA_VERSION, notes: BTreeMap::new() }
    }
}

pub fn usage_ledger_path(nucleus_root: &Path) -> PathBuf {
    nucleus_root.join("usage.json")
}

/// A missing file is an empty v1 ledger. A malformed file or a newer schema is an
/// error — never silently treated as empty, which would overwrite it on next record.
pub fn load_usage_ledger(nucleus_root: &Path) -> Result<NucleusUsageLedger, String> {
    let path = usage_ledger_path(nucleus_root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(NucleusUsageLedger::default()),
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    let ledger: NucleusUsageLedger =
        serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
    if ledger.schema_version > NUCLEUS_USAGE_SCHEMA_VERSION {
        return Err(format!(
            "{} has schema_version {} but this build understands up to {NUCLEUS_USAGE_SCHEMA_VERSION}",
            path.display(),
            ledger.schema_version
        ));
    }
    Ok(ledger)
}

/// Increment `slug`'s open count and persist atomically (temp file + rename).
pub fn record_note_open(nucleus_root: &Path, slug: &str, quark: Option<&str>) -> Result<NoteUsageEntry, String> {
    if !is_valid_slug(slug) {
        return Err(format!("invalid note slug: {slug:?}"));
    }
    let mut ledger = load_usage_ledger(nucleus_root)?;
    ledger.schema_version = NUCLEUS_USAGE_SCHEMA_VERSION;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let entry = ledger.notes.entry(slug.to_string()).or_default();
    entry.opens += 1;
    entry.last_opened_epoch_secs = now;
    if let Some(q) = quark {
        entry.last_quark = Some(q.to_string());
    }
    let snapshot = entry.clone();

    let path = usage_ledger_path(nucleus_root);
    let tmp = nucleus_root.join(format!(".usage.json.{}.tmp", std::process::id()));
    let json = serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(nucleus_root).map_err(|e| format!("create {}: {e}", nucleus_root.display()))?;
    std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let cleanup = match std::fs::remove_file(&tmp) {
            Ok(()) => String::new(),
            Err(c) => format!(" (temp file left behind: {c})"),
        };
        return Err(format!("rename {} -> {}: {e}{cleanup}", tmp.display(), path.display()));
    }
    Ok(snapshot)
}

/// `notes/<slug>.md` (optionally under `.hadron/nucleus/`, any leading dirs, either
/// separator) → `<slug>`. Anything else, including nested note dirs, is `None`.
pub fn extract_note_slug_from_path(path: &str) -> Option<String> {
    let normalized = path.trim().replace('\\', "/");
    let mut parts = normalized.rsplit('/');
    let file = parts.next()?;
    if parts.next()? != "notes" {
        return None;
    }
    let slug = file.strip_suffix(".md")?;
    is_valid_slug(slug).then(|| slug.to_string())
}

fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('.')
        && slug.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_ledger_is_empty_v1() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_usage_ledger(dir.path()).unwrap(), NucleusUsageLedger::default());
    }

    #[test]
    fn record_persists_and_increments() {
        let dir = tempfile::tempdir().unwrap();
        let first = record_note_open(dir.path(), "a-lesson", Some("cli-agy-2")).unwrap();
        assert_eq!(first.opens, 1);
        assert!(first.last_opened_epoch_secs > 0);
        let second = record_note_open(dir.path(), "a-lesson", None).unwrap();
        assert_eq!(second.opens, 2);
        assert_eq!(second.last_quark.as_deref(), Some("cli-agy-2"), "None keeps the last known quark");
        record_note_open(dir.path(), "other", None).unwrap();

        let ledger = load_usage_ledger(dir.path()).unwrap();
        assert_eq!(ledger.schema_version, 1);
        assert_eq!(ledger.notes["a-lesson"].opens, 2);
        assert_eq!(ledger.notes["other"].opens, 1);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "atomic write must not leave temp files");
    }

    #[test]
    fn rejects_newer_schema_and_malformed_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let path = usage_ledger_path(dir.path());
        std::fs::write(&path, r#"{"schema_version": 2, "notes": {}}"#).unwrap();
        assert!(load_usage_ledger(dir.path()).unwrap_err().contains("schema_version 2"));
        assert!(record_note_open(dir.path(), "x", None).is_err());
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"schema_version\": 2"));

        std::fs::write(&path, "not json").unwrap();
        assert!(load_usage_ledger(dir.path()).is_err());
        assert!(record_note_open(dir.path(), "x", None).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
    }

    #[test]
    fn extracts_slugs_only_from_note_paths() {
        let ok = |p: &str, s: &str| assert_eq!(extract_note_slug_from_path(p).as_deref(), Some(s), "{p}");
        ok("notes/compiled-is-not-running.md", "compiled-is-not-running");
        ok(".hadron/nucleus/notes/bug-x.md", "bug-x");
        ok("/home/j/dev/hadron/.hadron/nucleus/notes/a_b.md", "a_b");
        ok(r"C:\repo\.hadron\nucleus\notes\win.md", "win");
        for bad in ["index.md", "notes/x.txt", "notes/.md", "notes/sub/x.md", "x.md", "notes/..md", ""] {
            assert_eq!(extract_note_slug_from_path(bad), None, "{bad}");
        }
        assert!(record_note_open(tempfile::tempdir().unwrap().path(), "../escape", None).is_err());
    }
}
