use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use rusqlite::Connection;
use hadron_lattice::QuarkId;

/// The energy ledger. The `Mutex` is not for contention (every call site is the
/// engine's dispatch loop, one task) but for `Sync`: a rusqlite `Connection` holds a
/// `RefCell` statement cache, so a bare `Ledger` is `Send` but not `Sync` — and an
/// engine that isn't `Sync` can't hold `&self` across an `.await`, which the
/// concurrent turn loop does on every field append.
pub struct Ledger {
    conn: Mutex<Connection>,
}

/// Compute a deterministic host-local SQLite path under `~/.hadron/ledgers/` for
/// workspaces on filesystems that do not support Win32 `LockFileEx` (such as Windows
/// 9P `\\wsl.localhost\...` mounts, where SQLite fails with `Error code 5: database is locked`).
pub fn host_local_ledger_path(original_path: &Path) -> Option<PathBuf> {
    let base = hadron_lattice::user_hadron_dir()?.join("ledgers");
    let norm = hadron_lattice::sys::paths::strip_unc_prefix(&original_path.to_string_lossy())
        .replace('\\', "/")
        .to_ascii_lowercase();
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in norm.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Some(base.join(format!("ledger-{hash:016x}.db")))
}

impl Ledger {
    /// Open an in-memory ledger for tests or lockless fallback.
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(&conn)?;
        Ok(Ledger { conn: Mutex::new(conn) })
    }

    /// Open a file-backed ledger.
    ///
    /// On WSL UNC paths (`\\wsl.localhost\...` or `\\?\UNC\wsl.localhost\...`), the Windows 9P
    /// redirector rejects SQLite's `LockFileEx` byte-range locks with `SQLITE_BUSY` (`Error code 5`).
    /// Those paths are transparently routed to host-local storage under `~/.hadron/ledgers/`,
    /// with an in-memory fallback if file locking still fails.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if hadron_lattice::wsl::is_wsl_unc_path(path) {
            if let Some(local_path) = host_local_ledger_path(path) {
                if let Ok(ledger) = Self::try_open_file(&local_path) {
                    return Ok(ledger);
                }
            }
            return Self::open_in_memory();
        }

        match Self::try_open_file(path) {
            Ok(ledger) => Ok(ledger),
            Err(primary_err) => {
                if let Some(local_path) = host_local_ledger_path(path) {
                    if local_path != path {
                        if let Ok(ledger) = Self::try_open_file(&local_path) {
                            return Ok(ledger);
                        }
                    }
                }
                Self::open_in_memory().or(Err(primary_err))
            }
        }
    }

    fn try_open_file(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let _ = conn.busy_timeout(Duration::from_millis(2500));
        Self::init(&conn)?;
        Ok(Ledger { conn: Mutex::new(conn) })
    }

    fn init(conn: &Connection) -> anyhow::Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage (
                quark_id TEXT PRIMARY KEY,
                used_tokens INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
        Ok(())
    }

    /// Add tokens to a quark's total usage.
    pub fn record_usage(&self, quark: &QuarkId, tokens: u32) -> anyhow::Result<()> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        conn.execute(
            "INSERT INTO usage (quark_id, used_tokens) VALUES (?1, ?2)
             ON CONFLICT(quark_id) DO UPDATE SET used_tokens = used_tokens + ?2",
            rusqlite::params![quark.as_str(), tokens],
        )?;
        Ok(())
    }

    /// Check if a quark has exceeded the given limit.
    pub fn is_depleted(&self, quark: &QuarkId, limit: u32) -> anyhow::Result<bool> {
        let conn = self.conn.lock().expect("ledger mutex poisoned");
        let mut stmt = conn.prepare("SELECT used_tokens FROM usage WHERE quark_id = ?1")?;
        let mut rows = stmt.query([quark.as_str()])?;
        if let Some(row) = rows.next()? {
            let used: u32 = row.get(0)?;
            Ok(used >= limit)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_usage_and_detects_depletion() {
        let ledger = Ledger::open_in_memory().unwrap();
        let q = QuarkId::new("test");
        assert_eq!(ledger.is_depleted(&q, 100).unwrap(), false);
        
        ledger.record_usage(&q, 60).unwrap();
        assert_eq!(ledger.is_depleted(&q, 100).unwrap(), false);
        
        ledger.record_usage(&q, 50).unwrap();
        assert_eq!(ledger.is_depleted(&q, 100).unwrap(), true);
    }

    #[test]
    fn wsl_unc_ledger_path_routes_to_host_local_and_opens_cleanly() {
        let wsl_path = Path::new(r"\\wsl.localhost\Ubuntu\home\user\project\.hadron\ledger.db");
        let verbatim_wsl_path =
            Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\user\project\.hadron\ledger.db");
        assert_eq!(
            host_local_ledger_path(wsl_path),
            host_local_ledger_path(verbatim_wsl_path)
        );
        let ledger = Ledger::open(verbatim_wsl_path).expect("WSL UNC ledger should open cleanly");
        let q = QuarkId::new("wsl-quark");
        ledger.record_usage(&q, 42).unwrap();
        assert!(ledger.is_depleted(&q, 40).unwrap());
    }
}
