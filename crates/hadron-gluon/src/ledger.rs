use std::path::Path;
use std::sync::Mutex;
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

impl Ledger {
    /// Open an in-memory ledger for tests.
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(&conn)?;
        Ok(Ledger { conn: Mutex::new(conn) })
    }

    /// Open a file-backed ledger.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
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
}
