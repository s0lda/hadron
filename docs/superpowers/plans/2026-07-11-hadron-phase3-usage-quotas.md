# Phase 3 Implementation Plan: Usage Quotas & Intelligent Routing

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Track model budgets locally via an SQLite ledger, report usage to the field, and pause/reroute execution when a quark hits its limit.

**Architecture:**
1. **Lattice:** Formalize `Kind::EnergyReport` to broadcast usage per turn to the field, and add `used_tokens` to `TurnOutcome` so adapters can report their usage.
2. **Ledger:** A local SQLite database (`rusqlite`) in `hadron-gluon` tracking usage per `QuarkId`.
3. **Engine Pre-Flight:** Before the engine excites a quark, it checks the ledger. If the quark is depleted (usage > limit), the engine appends a `Status::Blocked` event and a `Message` explaining the depletion, then skips exciting the quark so control routes back to the orchestrator or human.
4. **Adapter:** The Claude adapter parses token usage from its JSON envelope (if available) and passes it up in `TurnOutcome`.

**Tech Stack:** Rust (edition 2021), `rusqlite = { version = "0.31", features = ["bundled"] }`.

## Global Constraints

- **Rust edition:** `2021`. Use latest stable Rust.
- **Field is append-only, never rewritten.** Every writer only appends whole lines. History is immutable.
- **Readers must tolerate unknown `kind` values.**
- **Vocabulary (use these exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger.

---

### Task 1: EnergyReport Event & TurnOutcome

**Files:**
- Modify: `crates/hadron-lattice/src/event.rs`
- Modify: `crates/hadron-lattice/src/projection.rs`

**Interfaces:**
- Produces: `Kind::EnergyReport { used_tokens: u32 }` in `event.rs`.
- Produces: `pub used_tokens: u32` field in `TurnOutcome` (with default 0).

- [ ] **Step 1: Add `EnergyReport` to `Kind`**
In `crates/hadron-lattice/src/event.rs`, add the `EnergyReport` variant to the `Kind` enum:
```rust
    Command { cmd: String, exit: i32, out_summary: String },
    Snapshot { git: String, label: String },
    EnergyReport { used_tokens: u32 },
    /// Any kind this version does not understand...
```
Update the `Serialize` and `Deserialize` implementations for `Event` to handle `Kind::EnergyReport { used_tokens }`. Map it to `"kind": "energy_report"`.

- [ ] **Step 2: Add `used_tokens` to `TurnOutcome`**
In `crates/hadron-lattice/src/projection.rs`, add `pub used_tokens: u32` to `TurnOutcome`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TurnOutcome {
    pub message: Option<String>,
    #[serde(default)]
    pub used_tokens: u32,
}
```
Update the `turn_outcome_default_is_empty` test to check `used_tokens: 0`.

- [ ] **Step 3: Run tests**
Run: `cargo test -p hadron-lattice`
Expected: PASS.

- [ ] **Step 4: Commit**
```bash
git add crates/hadron-lattice/src
git commit -m "feat(lattice): EnergyReport event and TurnOutcome token tracking"
```

---

### Task 2: SQLite Usage Ledger

**Files:**
- Modify: `crates/hadron-gluon/Cargo.toml`
- Create: `crates/hadron-gluon/src/ledger.rs`
- Modify: `crates/hadron-gluon/src/lib.rs`

**Interfaces:**
- Produces: `struct Ledger { conn: rusqlite::Connection }` with methods to open, record usage, and check if a quark is depleted.

- [ ] **Step 1: Add `rusqlite` dependency**
Add to `crates/hadron-gluon/Cargo.toml`:
```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

- [ ] **Step 2: Write `ledger.rs`**
Create `crates/hadron-gluon/src/ledger.rs`:
```rust
use std::path::Path;
use rusqlite::Connection;
use hadron_lattice::QuarkId;

pub struct Ledger {
    conn: Connection,
}

impl Ledger {
    /// Open an in-memory ledger for tests.
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(&conn)?;
        Ok(Ledger { conn })
    }

    /// Open a file-backed ledger.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(&conn)?;
        Ok(Ledger { conn })
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
        self.conn.execute(
            "INSERT INTO usage (quark_id, used_tokens) VALUES (?1, ?2)
             ON CONFLICT(quark_id) DO UPDATE SET used_tokens = used_tokens + ?2",
            rusqlite::params![quark.as_str(), tokens],
        )?;
        Ok(())
    }

    /// Check if a quark has exceeded the given limit.
    pub fn is_depleted(&self, quark: &QuarkId, limit: u32) -> anyhow::Result<bool> {
        let mut stmt = self.conn.prepare("SELECT used_tokens FROM usage WHERE quark_id = ?1")?;
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
```

- [ ] **Step 3: Expose `ledger`**
Add `pub mod ledger;` to `crates/hadron-gluon/src/lib.rs`.

- [ ] **Step 4: Run tests**
Run: `cargo test -p hadron-gluon ledger::`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**
```bash
git add crates/hadron-gluon/Cargo.toml crates/hadron-gluon/src
git commit -m "feat(gluon): SQLite usage ledger for energy tracking"
```

---

### Task 3: Engine Pre-Flight Check & Energy Reporting

**Files:**
- Modify: `crates/hadron-gluon/src/engine.rs`

**Interfaces:**
- Produces: `Engine::with_ledger(self, ledger: Ledger, limit: u32) -> Self`. The engine blocks depleted quarks, and reports used tokens from `TurnOutcome`.

- [ ] **Step 1: Wire Ledger into Engine**
In `crates/hadron-gluon/src/engine.rs`:
Add fields to `Engine`:
```rust
    ledger: Option<crate::ledger::Ledger>,
    energy_limit: u32,
```
Update `Engine::new` to initialize `ledger: None` and `energy_limit: 0`.
Add builder method:
```rust
    pub fn with_ledger(mut self, ledger: crate::ledger::Ledger, limit: u32) -> Self {
        self.ledger = Some(ledger);
        self.energy_limit = limit;
        self
    }
```

- [ ] **Step 2: Implement Pre-flight and Post-flight**
Inside `run_until_quiesce`, just after `let target = match next_pending(&events) { ... };` and the backstop check, add the pre-flight check:
```rust
            if let Some(ledger) = &self.ledger {
                if ledger.is_depleted(&target, self.energy_limit)? {
                    let msg = format!("⚠️ Quark {} is depleted (exceeded {} tokens).", target.as_str(), self.energy_limit);
                    append_event(
                        &self.field_path,
                        &Event::new(Actor::Gluon, None, Kind::Message { body: msg }),
                    )?;
                    append_event(
                        &self.field_path,
                        &Event::new(Actor::Quark(target.clone()), None, Kind::Status { state: QuarkState::Blocked }),
                    )?;
                    continue; // Reroute: skip this quark and process the next pending event
                }
            }
```

After `let outcome = quark.excite(projection).await?;`, handle the energy report before appending the standard message:
```rust
            if outcome.used_tokens > 0 {
                if let Some(ledger) = &self.ledger {
                    ledger.record_usage(&target, outcome.used_tokens)?;
                }
                append_event(
                    &self.field_path,
                    &Event::new(Actor::Quark(target.clone()), None, Kind::EnergyReport { used_tokens: outcome.used_tokens }),
                )?;
            }
```

- [ ] **Step 3: Update MockQuark for Tests**
In `crates/hadron-gluon/src/mock.rs`, update `TurnOutcome` instantiations to include `used_tokens: 0`. You can just add `used_tokens: 0` to all occurrences of `TurnOutcome { message: ... }`.

- [ ] **Step 4: Write Engine Test**
In `crates/hadron-gluon/src/engine.rs` tests:
```rust
    #[tokio::test]
    async fn engine_blocks_depleted_quarks_and_records_usage() {
        use crate::ledger::Ledger;
        let fdir = tempdir().unwrap();
        let path = fdir.path().join("field.jsonl");
        seed_human_message(&path, "worker", "do heavy work");

        struct HeavyQuark;
        #[async_trait::async_trait]
        impl Quark for HeavyQuark {
            fn id(&self) -> QuarkId { QuarkId::new("worker") }
            fn flavor(&self) -> Flavor { Flavor::Worker }
            fn energy(&self) -> hadron_lattice::EnergyState { hadron_lattice::EnergyState::Available }
            async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
                // Consume 100 tokens per turn, and hand back to itself to trigger another turn
                Ok(TurnOutcome { message: Some("@worker do more".into()), used_tokens: 100 })
            }
        }

        let ledger = Ledger::open_in_memory().unwrap();
        let mut engine = Engine::new(path.clone(), vec![Box::new(HeavyQuark)], "".into(), 5)
            .with_ledger(ledger, 150);
            
        engine.run_until_quiesce().await.unwrap();

        let events = read_events(&path).unwrap();
        
        let reports = events.iter().filter(|e| matches!(e.kind, Kind::EnergyReport { .. })).count();
        assert_eq!(reports, 2, "Quark should execute 2 times before depleting");
        
        let blocks = events.iter().filter(|e| matches!(e.kind, Kind::Status { state: QuarkState::Blocked })).count();
        assert_eq!(blocks, 1, "Quark should be blocked on the 3rd attempt");
    }
```

- [ ] **Step 5: Run tests**
Run: `cargo test -p hadron-gluon engine::`
Expected: PASS (all tests including the new one).

- [ ] **Step 6: Commit**
```bash
git add crates/hadron-gluon/src
git commit -m "feat(gluon): engine pre-flight ledger check and EnergyReport handling"
```

---

### Task 4: Adapter Token Parsing (Claude)

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/claude.rs`

**Interfaces:**
- Extract `usage.total_tokens` (or similar) from the Claude JSON envelope and populate `used_tokens`.

- [ ] **Step 1: Extract usage**
In `claude.rs` `excite()`, update the JSON parsing:
```rust
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result.stdout) {
            if let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) {
                self.session = Some(sid.to_string());
            }
            
            // Extract usage if available
            let used_tokens = v.get("usage")
                .and_then(|u| u.get("total_tokens"))
                .and_then(|t| t.as_u64())
                .map(|t| t as u32)
                .unwrap_or(0);

            if let Some(text) = v.get("result").and_then(|s| s.as_str()) {
                let t = text.trim();
                return Ok(TurnOutcome {
                    message: if t.is_empty() { None } else { Some(t.to_string()) },
                    used_tokens,
                });
            }
        }
```

- [ ] **Step 2: Update Tests**
In `claude.rs` tests, update `first_turn_starts_session_then_resumes` mock JSON to include `usage`:
```json
r#"{"session_id":"sess-1","result":"hello @worker","usage":{"total_tokens":42}}"#
```
Assert that `o1.used_tokens == 42`.
Update the second mock string to include usage:
```json
r#"{"session_id":"sess-1","result":"all done","usage":{"total_tokens":12}}"#
```
Assert that `o2.used_tokens == 12`.

- [ ] **Step 3: Run tests**
Run: `cargo test -p hadron-gluon adapter::claude::`
Expected: PASS.

- [ ] **Step 4: Commit**
```bash
git add crates/hadron-gluon/src/adapter/claude.rs
git commit -m "feat(gluon): Claude adapter parses token usage"
```

---

## Phase 3 Definition of Done
- Workspace compiles and all tests pass.
- `EnergyReport` is formally added to `Kind` and successfully serialized/deserialized.
- SQLite usage ledger tracks tokens per quark.
- The Engine blocks depleted quarks and prevents excitation.
- Adapters can bubble up usage metrics via `TurnOutcome`.
