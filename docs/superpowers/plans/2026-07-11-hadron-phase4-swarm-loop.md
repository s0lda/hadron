# Hadron Phase 4 — The Swarm Loop (persistent 0-CPU daemon) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the engine from a run-once-then-quiesce batch into a persistent daemon that sleeps at ~0% CPU and wakes to process new field appends until told to shut down.

**Architecture:** A new `hadron_gluon::daemon` module adds `Engine::serve(shutdown)`. `serve` loops `run_until_quiesce()` then blocks in a `tokio::select!` on three arms: a shutdown `watch` channel, a change signal fed by the slice-2 `FieldWatcher` (via a `spawn_blocking` bridge → `mpsc`), and a periodic safety-poll `sleep`. **Notify is a latency optimization; the safety-poll is the liveness guarantee** — so the daemon still makes progress on filesystems where inotify never fires (e.g. WSL2 `/mnt/c`) and never busy-spins if the watcher bridge dies.

**Tech Stack:** Rust (edition 2021), `tokio` (add the `sync` feature for `watch` + `mpsc`), existing `hadron_gluon::watch::FieldWatcher`, `hadron_gluon::engine::Engine`, `hadron_gluon::mock::MockQuark`.

## Global Constraints

- **0-CPU class, never busy-spin.** The daemon sleeps in `select!`; when idle the process consumes no CPU except one safety-poll wake every `SAFETY_POLL` (~500ms) that does an O(events) re-read of a small file. A dropped change channel must NOT turn into a hot loop.
- **Liveness cannot depend on notify.** notify silently never fires on WSL2 `/mnt/c` mounts. The safety-poll arm guarantees progress regardless; notify only lowers latency below `SAFETY_POLL`.
- **Additive, low-collision with Gemini.** Gemini is live on `main` (Phase 5). Put the loop in a NEW file `daemon.rs`; the only edit to the contended `engine.rs` is a one-line `pub(crate) fn field_path()` getter. Cargo.toml gets one feature added to an existing array.
- **Zero API spend in tests.** Tests drive `MockQuark` only. No network.
- **Append-only, unknown-tolerant.** Unchanged — `serve` reuses `run_until_quiesce`, which already honors this.
- **Vocabulary (exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger, block, hash, forge, watch. (The daemon "serves"/"wakes"; no new vocabulary introduced.)

---

### Task 1: Add the `sync` tokio feature + the `field_path` getter

**Files:**
- Modify: `crates/hadron-gluon/Cargo.toml` (tokio features array)
- Modify: `crates/hadron-gluon/src/engine.rs` (add getter to `impl Engine`)

**Interfaces:**
- Produces: `Engine::field_path(&self) -> &std::path::Path` (pub(crate)) — lets `daemon.rs` read the private `field_path` field to construct the watcher.
- Consumes: tokio `sync` feature for `watch`/`mpsc` in Task 2.

- [ ] **Step 1: Add `"sync"` to the tokio features array** in `crates/hadron-gluon/Cargo.toml`. The array becomes:

```toml
tokio = { version = "1", features = [
    "rt",
    "rt-multi-thread",
    "macros",
    "process",
    "io-util",
    "time",
    "sync",
] }
```

- [ ] **Step 2: Add the getter** to `impl Engine` in `crates/hadron-gluon/src/engine.rs` (place it right after `pub fn new(...)`'s closing brace, before `with_git`):

```rust
    /// The field file this engine reads and appends to.
    pub(crate) fn field_path(&self) -> &std::path::Path {
        &self.field_path
    }
```

- [ ] **Step 3: Verify it compiles** — `cargo build -p hadron-gluon`.
Expected: builds clean (the getter is currently unused; that's fine — Task 2 consumes it in the same crate, so no dead-code warning once wired. If a warning appears before Task 2, ignore it.)

- [ ] **Step 4: Commit**

```bash
git add crates/hadron-gluon/Cargo.toml crates/hadron-gluon/src/engine.rs
git commit -m "feat(gluon): tokio sync feature + Engine::field_path getter for the daemon"
```

---

### Task 2: The `serve` loop (the swarm daemon)

**Files:**
- Create: `crates/hadron-gluon/src/daemon.rs`
- Modify: `crates/hadron-gluon/src/lib.rs` (add `pub mod daemon;`)

**Interfaces:**
- Consumes: `Engine::run_until_quiesce(&mut self) -> anyhow::Result<()>`, `Engine::field_path(&self) -> &Path` (Task 1), `crate::watch::FieldWatcher::{new, wait}`.
- Produces: `impl Engine { pub async fn serve(&mut self, shutdown: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<()> }` — runs until `shutdown` observes `true`, then returns `Ok(())` after joining the watcher bridge.

- [ ] **Step 1: Write the failing test.** Create `crates/hadron-gluon/src/daemon.rs` with the loop stubbed to `todo!()` inside, plus these two tests. (The tests import from the existing test-helper patterns in `engine.rs`; `MockQuark::scripted` and manual field seeding.)

```rust
use std::path::Path;
use std::time::Duration;

use tokio::sync::watch;

use crate::engine::Engine;
use crate::watch::FieldWatcher;

/// How often the daemon re-checks the field even if notify never fires.
/// notify (via FieldWatcher) wakes it sooner; this is the liveness floor.
const SAFETY_POLL: Duration = Duration::from_millis(500);

impl Engine {
    /// Run as a persistent daemon: process the field to quiescence, then sleep
    /// until the field changes (notify) or `SAFETY_POLL` elapses, then process
    /// again. Returns when `shutdown` observes `true`.
    ///
    /// 0-CPU class: the process is asleep in `select!` between wakes. notify is
    /// a latency optimization; the safety-poll arm is the liveness guarantee, so
    /// the daemon still progresses where inotify never fires (WSL2 `/mnt/c`) and
    /// never busy-spins if the watcher bridge dies.
    pub async fn serve(&mut self, mut shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{append_event, read_events};
    use crate::mock::MockQuark;
    use hadron_lattice::{Actor, Event, Flavor, Kind, QuarkId};
    use tempfile::tempdir;

    fn human_msg(to: &str, body: &str) -> Event {
        Event::new(
            Actor::Human,
            Some(QuarkId::new(to)),
            Kind::Message { body: body.into() },
        )
    }

    fn engine_with_quark(field: &Path, id: &str, reply: &str) -> Engine {
        let quark = MockQuark::repeating(QuarkId::new(id), Flavor::Orchestrator, reply);
        Engine::new(field.to_path_buf(), vec![Box::new(quark)], String::new(), 8)
    }

    #[tokio::test]
    async fn daemon_processes_an_appended_message_then_shuts_down() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        // Seed the file so the watcher's parent dir exists and is watchable.
        append_event(&field, &human_msg("agy", "hello")).unwrap();

        let mut engine = engine_with_quark(&field, "agy", "pong");
        let (tx, rx) = watch::channel(false);

        let handle = tokio::spawn(async move { engine.serve(rx).await });

        // Poll for the quark's reply to land (safety-poll floor is 500ms; give margin).
        let mut saw_reply = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let events = read_events(&field).unwrap();
            if events.iter().any(|e| matches!(&e.kind, Kind::Message { body } if body == "pong")) {
                saw_reply = true;
                break;
            }
        }
        assert!(saw_reply, "daemon should have processed the seeded message and appended a reply");

        // Shut down and confirm serve() returns Ok promptly.
        tx.send(true).unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(joined.is_ok(), "serve() should return promptly after shutdown");
        joined.unwrap().unwrap().unwrap();
    }

    #[tokio::test]
    async fn daemon_shuts_down_promptly_when_idle() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        append_event(&field, &human_msg("agy", "hello")).unwrap();

        let mut engine = engine_with_quark(&field, "agy", "pong");
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(async move { engine.serve(rx).await });

        // Let it reach the idle select, then shut down.
        tokio::time::sleep(Duration::from_millis(200)).await;
        tx.send(true).unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(joined.is_ok(), "idle serve() should return promptly after shutdown");
        joined.unwrap().unwrap().unwrap();
    }
}
```

Add to `crates/hadron-gluon/src/lib.rs` after `pub mod watch;`:

```rust
pub mod daemon;
```

- [ ] **Step 2: Run tests to verify they fail** — `cargo test -p hadron-gluon daemon::`.
Expected: FAIL — `todo!()` panics (`not yet implemented`).

- [ ] **Step 3: Implement `serve`.** Replace the `todo!()` body with the real loop:

```rust
    pub async fn serve(&mut self, mut shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
        // Bridge the blocking notify watcher into an async change signal. A
        // bounded(1) channel coalesces bursts: one pending "something changed"
        // is all the loop needs.
        let (change_tx, mut change_rx) = tokio::sync::mpsc::channel::<()>(1);
        let field_path = self.field_path().to_path_buf();
        let bridge_shutdown = shutdown.clone();
        let bridge = tokio::task::spawn_blocking(move || {
            // If the watcher can't be created (e.g. dir missing), drop the
            // sender and let the daemon fall back to pure safety-polling.
            let watcher = match FieldWatcher::new(&field_path) {
                Ok(w) => w,
                Err(_) => return,
            };
            while !*bridge_shutdown.borrow() {
                if watcher.wait(Duration::from_millis(200)) {
                    let _ = change_tx.try_send(());
                }
            }
        });

        // Once the change channel closes (bridge exited / watcher failed) we
        // stop selecting on it so a ready-`None` can't starve the sleep arm and
        // spin the loop. The safety-poll keeps the daemon live regardless.
        let mut change_open = true;
        loop {
            self.run_until_quiesce().await?;
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        break;
                    }
                }
                recv = change_rx.recv(), if change_open => {
                    if recv.is_none() {
                        change_open = false;
                    }
                }
                _ = tokio::time::sleep(SAFETY_POLL) => {}
            }
        }

        let _ = bridge.await;
        Ok(())
    }
```

- [ ] **Step 4: Run tests to verify they pass** — `cargo test -p hadron-gluon daemon::`.
Expected: PASS (both tests). If `daemon_processes...` is flaky, the safety-poll should still catch the seeded event within 500ms; the 4s poll window has ample margin.

- [ ] **Step 5: Run the full gluon suite** — `cargo test -p hadron-gluon`.
Expected: all prior tests still green + 2 new.

- [ ] **Step 6: Commit**

```bash
git add crates/hadron-gluon/src/daemon.rs crates/hadron-gluon/src/lib.rs
git commit -m "feat(gluon): the swarm loop — persistent 0-CPU serve() daemon (notify + safety-poll)"
```

---

### Task 3: Full-workspace verification

**Files:** none (verification only)

- [ ] **Step 1: Full workspace test** — `cargo test` (default features; GPUI `gui` feature stays off).
Expected: all crates green (lattice, gluon incl. 2 new daemon tests, forge, chamber model).

- [ ] **Step 2: Clippy sanity** — `cargo clippy -p hadron-gluon 2>&1 | grep -E "warning: unused|error" | head`.
Expected: no new warnings from `daemon.rs`/the getter. (Pre-existing `ledger.rs` assert-bool warnings from Phase 3 are out of scope.)

---

## Definition of Done

- `Engine::serve(shutdown)` runs as a persistent daemon: processes the field to quiescence, sleeps in `select!`, wakes on notify OR the `SAFETY_POLL` floor, and returns `Ok(())` promptly when the shutdown `watch` flips to `true`.
- The daemon is 0-CPU class when idle and cannot busy-spin even if the watcher bridge dies (the `change_open` guard).
- The daemon makes progress on filesystems where inotify never fires, because the safety-poll — not notify — is the liveness guarantee.
- Two tests drive it with `MockQuark` (zero API spend), both green; full workspace green.
- Collision surface with Gemini's `engine.rs` is one getter; the loop lives in new `daemon.rs`.

## Notes / bought land (deferred)

- **Still sequential.** `run_until_quiesce` processes one quark per tick; `serve` just makes that persistent. True concurrent multi-quark excitation (where slice-1 `forge` edit-by-hash arbitration finally earns its keep) is a larger future item, NOT part of "finish Phase 4."
- **CLI-adapter path, not reqwest.** The roadmap sketched quarks executing via their HTTP API; Hadron instead drives the vendor CLIs (the same pragmatic deviation Plan 3 already booked). `serve` inherits that path unchanged.
- **`SAFETY_POLL` is a fixed 500ms.** An adaptive/backoff poll (longer when idle) is bought land; 500ms is already 0-CPU class for a small field.
- **No binary entrypoint yet.** `serve` is a library method; wiring it into a `main`/subcommand that a chamber launches is a later integration step.
