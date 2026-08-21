//! File-level intent locks and collision detection.
//!
//! Tracks planned file write targets across active worker quarks to prevent
//! concurrent write overlap while keeping orthogonal tasks parallel.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use crate::QuarkId;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// An acquired lock lease representing exclusive write intent over a set of paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockLease {
    pub id: Ulid,
    pub quark: QuarkId,
    pub paths: Vec<PathBuf>,
    pub acquired_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Advisory intent lock table preventing concurrent overlapping writes across worker quarks.
#[derive(Debug, Clone, Default)]
pub struct IntentLockTable {
    pub locks: HashMap<PathBuf, (QuarkId, Ulid, Instant, Duration)>,
}

impl IntentLockTable {
    pub fn new() -> Self {
        Self {
            locks: HashMap::new(),
        }
    }

    /// Prune expired locks based on elapsed TTL.
    pub fn prune_expired(&mut self) {
        self.locks.retain(|_, (_, _, acquired, ttl)| acquired.elapsed() < *ttl);
    }

    /// Attempt to acquire exclusive locks for `paths`. If any path is locked by another quark,
    /// returns `Err(conflicts)`.
    pub fn try_acquire(
        &mut self,
        quark: QuarkId,
        paths: &[PathBuf],
        ttl: Duration,
    ) -> Result<LockLease, Vec<PathBuf>> {
        self.prune_expired();

        let mut conflicts = Vec::new();
        for p in paths {
            if let Some((holder, _, _, _)) = self.locks.get(p) {
                if holder != &quark {
                    conflicts.push(p.clone());
                }
            }
        }

        if !conflicts.is_empty() {
            return Err(conflicts);
        }

        let lease_id = Ulid::new();
        let now_utc = chrono::Utc::now();
        let ttl_chrono = chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::seconds(60));
        let expires_at = now_utc + ttl_chrono;

        for p in paths {
            self.locks.insert(p.clone(), (quark.clone(), lease_id, Instant::now(), ttl));
        }

        Ok(LockLease {
            id: lease_id,
            quark,
            paths: paths.to_vec(),
            acquired_at: now_utc,
            expires_at,
        })
    }

    /// Release locks held by a lease.
    pub fn release(&mut self, lease: &LockLease) {
        for p in &lease.paths {
            if let Some((holder, id, _, _)) = self.locks.get(p) {
                if holder == &lease.quark && id == &lease.id {
                    self.locks.remove(p);
                }
            }
        }
    }

    /// Check if a path is currently locked by a different quark.
    pub fn is_locked_by_other(&self, path: &PathBuf, quark: &QuarkId) -> bool {
        if let Some((holder, _, acquired, ttl)) = self.locks.get(path) {
            if acquired.elapsed() < *ttl && holder != quark {
                return true;
            }
        }
        false
    }
}
