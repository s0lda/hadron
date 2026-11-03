//! A durable home for the byte size of each section a built prompt carries, so
//! Stats can show where a turn's tokens actually went instead of only the total.
//!
//! Same shape as [`crate::quota`]: one small JSON file per quark, atomic
//! temp+rename writes, and a directory derived from the field path so every
//! process agrees on the location without a second setting. This module only
//! measures *size* — whether a given turn's tokens for a section were fresh or
//! cache-read is answered separately by `TokenSpend` on that turn's
//! `UsageUpdate`; the two are never merged into one number (see the spec).

use crate::QuarkId;
use std::path::{Path, PathBuf};

/// Byte size of each section a built prompt writes, in the order `build`
/// writes them. `standard_model` is the fixed directive header every prompt
/// carries; the rest mirror the matching `Projection` field.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptBreakdown {
    pub standard_model: usize,
    pub invariants: usize,
    pub nucleus_digest: usize,
    pub nucleus_index: usize,
    /// The matched skill's rendered body plus its handoff block. Defaulted, so a
    /// prompt-cost file written before this section existed still deserialises.
    #[serde(default)]
    pub active_skill: usize,
    pub task: usize,
    pub field_window: usize,
}

/// The directory that holds one prompt-cost file per quark, derived from the
/// field path the same way [`crate::quota::quota_dir`] is: `<field-dir>/prompt-cost`.
pub fn prompt_cost_dir(field: &Path) -> PathBuf {
    crate::hadron_dir_of(field).join("prompt-cost")
}

fn file_for(dir: &Path, quark: &QuarkId) -> PathBuf {
    dir.join(format!("{}.json", quark.as_str()))
}

fn latest_marker_file(dir: &Path) -> PathBuf {
    dir.join("latest")
}

/// Persist the latest breakdown for this quark. Atomic (temp + rename), so a
/// reader can never observe a half-written file. Also updates the `latest`
/// quark marker so [`load_latest`] resolves the most recent prompt turn
/// deterministically even when two saves occur within the same filesystem
/// timestamp tick.
pub fn save(dir: &Path, quark: &QuarkId, breakdown: &PromptBreakdown) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let final_path = file_for(dir, quark);
    let tmp = final_path.with_extension("json.tmp");
    let body = serde_json::to_string(breakdown)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &final_path)?;

    let marker = latest_marker_file(dir);
    let marker_tmp = dir.join("latest.tmp");
    std::fs::write(&marker_tmp, quark.as_str())?;
    std::fs::rename(&marker_tmp, &marker)
}

/// The breakdown last persisted for this quark. `None` if it has never run a
/// turn through this path, or the file is missing/unreadable/malformed.
pub fn load(dir: &Path, quark: &QuarkId) -> Option<PromptBreakdown> {
    let body = std::fs::read_to_string(file_for(dir, quark)).ok()?;
    serde_json::from_str(&body).ok()
}

/// Load the most recently persisted `(quark_id, PromptBreakdown)` from `dir`.
///
/// Checks the `latest` marker written by [`save`] first (when it matches a
/// valid breakdown and, if `roster_ids` is non-empty, a known roster quark),
/// then falls back to selecting the candidate `<quark>.json` file with the
/// newest filesystem modification timestamp (`mtime`). This prevents disabled
/// or older quarks earlier in `roster_ids` from shadowing the quark that
/// actually ran the latest prompt.
pub fn load_latest(dir: &Path, roster_ids: &[String]) -> Option<(String, PromptBreakdown)> {
    if let Ok(marker_raw) = std::fs::read_to_string(latest_marker_file(dir)) {
        let marker_id = marker_raw.trim();
        if !marker_id.is_empty()
            && (roster_ids.is_empty() || roster_ids.iter().any(|id| id == marker_id))
        {
            if let Some(breakdown) = load(dir, &QuarkId::new(marker_id)) {
                return Some((marker_id.to_string(), breakdown));
            }
        }
    }

    let pick_newest = |ids: &[String]| -> Option<(String, PromptBreakdown)> {
        let mut best: Option<(Option<std::time::SystemTime>, String, PromptBreakdown)> = None;
        for id in ids {
            let path = file_for(dir, &QuarkId::new(id));
            let Some(breakdown) = load(dir, &QuarkId::new(id)) else {
                continue;
            };
            let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let replace = match &best {
                None => true,
                Some((best_mtime, _, _)) => mtime > *best_mtime,
            };
            if replace {
                best = Some((mtime, id.clone(), breakdown));
            }
        }
        best.map(|(_, id, b)| (id, b))
    };

    if !roster_ids.is_empty() {
        if let Some(found) = pick_newest(roster_ids) {
            return Some(found);
        }
    }

    let entries = std::fs::read_dir(dir).ok()?;
    let mut disk_ids: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            } else {
                None
            }
        })
        .collect();
    disk_ids.sort();
    pick_newest(&disk_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("hadron-prompt-cost-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn the_prompt_cost_dir_sits_beside_the_field() {
        let field = Path::new("/home/user/project/.hadron/field.jsonl");
        assert_eq!(
            prompt_cost_dir(field),
            PathBuf::from("/home/user/project/.hadron/prompt-cost")
        );
    }

    #[test]
    fn a_saved_breakdown_round_trips() {
        let dir = tmp();
        let id = QuarkId::new("acp-claude");
        let breakdown = PromptBreakdown {
            standard_model: 100,
            invariants: 200,
            nucleus_digest: 0,
            nucleus_index: 50,
            active_skill: 3_145,
            task: 20,
            field_window: 300,
        };
        save(&dir, &id, &breakdown).unwrap();
        assert_eq!(load(&dir, &id), Some(breakdown));
    }

    /// A file written before `active_skill` existed must still load — the field is
    /// `#[serde(default)]` for exactly this, and every quark already has one on disk.
    #[test]
    fn a_breakdown_written_before_active_skill_still_loads() {
        let dir = tmp();
        let id = QuarkId::new("acp-claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            file_for(&dir, &id),
            r#"{"standard_model":393,"invariants":26563,"nucleus_digest":1653,"nucleus_index":24286,"task":1171,"field_window":44185}"#,
        )
        .unwrap();
        assert_eq!(load(&dir, &id).map(|b| b.active_skill), Some(0));
    }

    /// Absence is "never run a turn here", not an error.
    #[test]
    fn an_absent_file_loads_as_none() {
        let dir = tmp();
        assert_eq!(load(&dir, &QuarkId::new("nobody")), None);
    }

    /// Saving again for the same quark replaces the file wholesale.
    #[test]
    fn saving_again_replaces_the_prior_breakdown() {
        let dir = tmp();
        let id = QuarkId::new("acp-claude");
        save(&dir, &id, &PromptBreakdown { task: 10, ..Default::default() }).unwrap();
        save(&dir, &id, &PromptBreakdown { task: 999, ..Default::default() }).unwrap();
        assert_eq!(load(&dir, &id).unwrap().task, 999);
    }

    /// `load_latest` must return the quark that saved most recently, even when an older
    /// quark appears earlier in `roster_ids` (e.g. a disabled `acp-claude-2` at index 0
    /// while `cli-agy` at index 1 just ran a new prompt).
    #[test]
    fn load_latest_picks_most_recent_save_over_roster_order() {
        let dir = tmp();
        let old_id = QuarkId::new("acp-claude-2");
        let new_id = QuarkId::new("cli-agy");
        save(&dir, &old_id, &PromptBreakdown { task: 111, ..Default::default() }).unwrap();
        save(&dir, &new_id, &PromptBreakdown { task: 777, ..Default::default() }).unwrap();

        let roster_ids = vec!["acp-claude-2".to_string(), "cli-agy".to_string()];
        let (picked_id, picked_breakdown) = load_latest(&dir, &roster_ids).unwrap();
        assert_eq!(picked_id, "cli-agy");
        assert_eq!(picked_breakdown.task, 777);
    }

    /// When `latest` marker is absent (legacy `prompt-cost/` directories), `load_latest`
    /// falls back to the newest file modification time.
    #[test]
    fn load_latest_falls_back_to_mtime_when_marker_missing() {
        let dir = tmp();
        let old_id = QuarkId::new("acp-claude-2");
        let new_id = QuarkId::new("cli-agy");
        save(&dir, &old_id, &PromptBreakdown { task: 111, ..Default::default() }).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        save(&dir, &new_id, &PromptBreakdown { task: 888, ..Default::default() }).unwrap();
        std::fs::remove_file(latest_marker_file(&dir)).unwrap();

        let roster_ids = vec!["acp-claude-2".to_string(), "cli-agy".to_string()];
        let (picked_id, picked_breakdown) = load_latest(&dir, &roster_ids).unwrap();
        assert_eq!(picked_id, "cli-agy");
        assert_eq!(picked_breakdown.task, 888);
    }
}
