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

/// Persist the latest breakdown for this quark. Atomic (temp + rename), so a
/// reader can never observe a half-written file.
pub fn save(dir: &Path, quark: &QuarkId, breakdown: &PromptBreakdown) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let final_path = file_for(dir, quark);
    let tmp = final_path.with_extension("json.tmp");
    let body = serde_json::to_string(breakdown)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &final_path)
}

/// The breakdown last persisted for this quark. `None` if it has never run a
/// turn through this path, or the file is missing/unreadable/malformed.
pub fn load(dir: &Path, quark: &QuarkId) -> Option<PromptBreakdown> {
    let body = std::fs::read_to_string(file_for(dir, quark)).ok()?;
    serde_json::from_str(&body).ok()
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
        let field = Path::new("/home/jake/dev/hadron/.hadron/field.jsonl");
        assert_eq!(
            prompt_cost_dir(field),
            PathBuf::from("/home/jake/dev/hadron/.hadron/prompt-cost")
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
            task: 20,
            field_window: 300,
        };
        save(&dir, &id, &breakdown).unwrap();
        assert_eq!(load(&dir, &id), Some(breakdown));
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
}
