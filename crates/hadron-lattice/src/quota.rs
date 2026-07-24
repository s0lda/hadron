//! A durable home for the quota buckets a provider has reported, so a daemon
//! restart or a `/clear` does not lose a still-valid rate-limit window.
//!
//! Before this existed the only copy lived in an ACP session's in-memory
//! `Arc<Mutex<Vec<QuotaBucket>>>` — real for the life of that session and gone
//! the moment it re-boots. `render/stats.rs`'s `quota_is_live` already decides
//! what to *show* (an expired bucket is hidden); this module only decides what
//! to *keep*, so it does not filter anything — that is the renderer's job, not
//! the store's.
//!
//! Same shape as [`crate::live`]: one small JSON file per quark, atomic
//! temp+rename writes, and a directory derived from the field path so every
//! process agrees on the location without a second setting.

use crate::{QuarkId, QuotaBucket};
use std::path::{Path, PathBuf};

/// The directory that holds one quota file per quark, derived from the field
/// path the same way [`crate::live::live_dir`] is: `<field-dir>/quota`.
pub fn quota_dir(field: &Path) -> PathBuf {
    crate::hadron_dir_of(field).join("quota")
}

fn file_for(dir: &Path, quark: &QuarkId) -> PathBuf {
    dir.join(format!("{}.json", quark.as_str()))
}

/// Persist the full set of buckets last known for this quark. Atomic
/// (temp + rename), so a reader can never observe a half-written file.
pub fn save(dir: &Path, quark: &QuarkId, buckets: &[QuotaBucket]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let final_path = file_for(dir, quark);
    let tmp = final_path.with_extension("json.tmp");
    let body = serde_json::to_string(buckets)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &final_path)
}

/// The buckets last persisted for this quark. Absent, unreadable, or malformed
/// all read as "nothing known" — an empty `Vec`, same honesty rule as
/// `Usage::quota`: empty means unknown, never "full".
///
/// Deliberately does **not** drop expired buckets — that filter belongs at the
/// render site ([`quota_is_live`] in the chamber), not here, so a caller that
/// wants to know "was there ever a window" can still see one that has lapsed.
pub fn load(dir: &Path, quark: &QuarkId) -> Vec<QuotaBucket> {
    let Ok(body) = std::fs::read_to_string(file_for(dir, quark)) else {
        return Vec::new();
    };
    serde_json::from_str(&body).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("hadron-quota-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn bucket(key: &str, remaining: f64) -> QuotaBucket {
        QuotaBucket { key: key.to_string(), remaining_fraction: remaining, reset_time: None }
    }

    #[test]
    fn the_quota_dir_sits_beside_the_field() {
        let field = Path::new("/home/jake/dev/hadron/.hadron/field.jsonl");
        assert_eq!(quota_dir(field), PathBuf::from("/home/jake/dev/hadron/.hadron/quota"));
    }

    #[test]
    fn a_saved_bucket_round_trips() {
        let dir = tmp();
        let id = QuarkId::new("acp-claude");
        let buckets = vec![bucket("claude-five_hour", 0.42)];
        save(&dir, &id, &buckets).unwrap();
        assert_eq!(load(&dir, &id), buckets);
    }

    /// Absence is "nothing known", not an error — a quark that has never
    /// reported quota must not block a fresh boot.
    #[test]
    fn an_absent_file_loads_as_empty() {
        let dir = tmp();
        assert_eq!(load(&dir, &QuarkId::new("nobody")), Vec::<QuotaBucket>::new());
    }

    /// **The property that matters**: the store does not decide what is still
    /// valid. A bucket whose window has already lapsed round-trips exactly as
    /// saved — `quota_is_live` at the render site is what hides it.
    #[test]
    fn an_expired_bucket_still_round_trips() {
        let dir = tmp();
        let id = QuarkId::new("acp-claude");
        let expired = QuotaBucket {
            key: "claude-five_hour".to_string(),
            remaining_fraction: 0.06,
            reset_time: Some(Utc::now() - Duration::hours(6)),
        };
        save(&dir, &id, &[expired.clone()]).unwrap();
        assert_eq!(load(&dir, &id), vec![expired], "the store must not filter — the renderer decides what to show");
    }

    /// Saving again for the same quark replaces the file wholesale — the
    /// caller always hands over the full accumulated set, same contract as
    /// `live::publish` overwriting in place.
    #[test]
    fn saving_again_replaces_the_prior_snapshot() {
        let dir = tmp();
        let id = QuarkId::new("acp-claude");
        save(&dir, &id, &[bucket("claude-five_hour", 0.42)]).unwrap();
        save(&dir, &id, &[bucket("claude-five_hour", 0.10), bucket("claude-seven_day", 0.55)]).unwrap();
        let loaded = load(&dir, &id);
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|b| b.key == "claude-five_hour" && b.remaining_fraction == 0.10));
    }
}
