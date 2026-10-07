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

    /// Release locks held under a specific lease id and quark.
    pub fn release_by_id(&mut self, quark: &QuarkId, lease_id: Ulid) -> bool {
        let mut found = false;
        self.locks.retain(|_, (holder, id, _, _)| {
            if holder == quark && id == &lease_id {
                found = true;
                false
            } else {
                true
            }
        });
        found
    }

    /// Return all currently active, non-expired leases grouped by lease id.
    pub fn active_leases(&self) -> Vec<LockLease> {
        let mut by_lease: HashMap<Ulid, (QuarkId, Vec<PathBuf>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> = HashMap::new();
        let now_utc = chrono::Utc::now();
        for (path, (quark, lease_id, acquired, ttl)) in &self.locks {
            if acquired.elapsed() < *ttl {
                let remaining = ttl.saturating_sub(acquired.elapsed());
                let expires_at = now_utc + chrono::Duration::from_std(remaining).unwrap_or_default();
                let entry = by_lease.entry(*lease_id).or_insert_with(|| {
                    (quark.clone(), Vec::new(), now_utc, expires_at)
                });
                entry.1.push(path.clone());
            }
        }

        by_lease
            .into_iter()
            .map(|(id, (quark, paths, acquired_at, expires_at))| LockLease {
                id,
                quark,
                paths,
                acquired_at,
                expires_at,
            })
            .collect()
    }

    /// Restore an existing lease (preserving original lease ID and acquired time).
    pub fn restore_lease(&mut self, lease: LockLease, ttl: Duration) {
        let now = Instant::now();
        for p in lease.paths {
            self.locks.insert(p, (lease.quark.clone(), lease.id, now, ttl));
        }
    }

    /// Load persisted leases from disk, restoring active ones with their original IDs.
    pub fn load_from_file(path: &std::path::Path) -> Self {
        let mut table = Self::new();
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(leases) = serde_json::from_str::<Vec<LockLease>>(&data) {
                let now = chrono::Utc::now();
                for lease in leases {
                    if lease.expires_at > now {
                        if let Ok(remaining) = (lease.expires_at - now).to_std() {
                            table.restore_lease(lease, remaining);
                        }
                    }
                }
            }
        }
        table
    }

    /// Persist active leases to disk.
    pub fn save_to_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let active = self.active_leases();
        let data = serde_json::to_string_pretty(&active)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, data)
    }
}
