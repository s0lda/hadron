# Hadron Phase 4 (slice 2) — The 0-CPU Field Watcher

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Build the "0-CPU file bus" wake mechanism from Phase 4 — a filesystem-watched, incrementally-read field so a reader (the future swarm daemon, or a lower-latency chamber tail) sleeps at 0% CPU and wakes only when `field.jsonl` grows, then reads *only the newly-appended* events instead of re-parsing the whole file.

**Architecture:** The project's pure-core + thin-seam pattern (as with `forge` and the `CliRunner`). The correctness-bearing piece is a **pure incremental reader** `hadron_lattice::io::read_new(path, offset) -> (Vec<Event>, new_offset)` — deterministic, torn-line-safe, exhaustively unit-tested with tempfiles, and immediately useful (the chamber currently full-re-reads every 400ms tick). The OS-timing-dependent piece is a **thin `notify` seam** `hadron_gluon::watch::FieldWatcher` that coalesces raw fs events into a single "the field changed" signal; its test is a deliberately thin "the wire is live" check with a generous timeout — *not* where correctness lives. A small `watch_new_events` helper composes the two into a 0-CPU "block until new events, then yield them" loop.

**Tech Stack:** Rust (edition 2021), `notify = "8"` (cached, validated to fire end-to-end in this environment), plus existing lattice/gluon deps. Dev: `tempfile`.

> **Execution status (2026-07-11):** All 4 tasks **executed and committed** on branch `worktree-phase4-field-watcher` (branched from merged `main`). 7 new tests green (4 `read_new` deterministic + 3 `watch` real-inotify); full `cargo test --workspace` = 82 passed / 0 failed; the new `read_new`/`watch` code is clippy-clean. Zero API spend, no GPUI in the path. Note: `cargo clippy -p hadron-gluon` surfaces 3 pre-existing `assert_eq!`-with-bool warnings in Phase 3's `ledger.rs` — deliberately NOT fixed here (out of slice scope; belongs in a Phase 3 cleanup). Deferred: wiring the watcher into a persistent swarm daemon (next slice), and the `/mnt/c` inotify-silent-no-fire fallback (bought land below).

**This is slice 2 of Phase 4** (roadmap: `docs/plans/001_Initial_Plan.md` §"Phase 4"), building on slice 1 (`hadron-forge` edit-by-hash, already merged). It is **roadmap item 1 ("The Watcher") and the hard prerequisite for the swarm loop.** Honest framing: `FieldWatcher`/`watch_new_events` is infrastructure nothing consumes yet (the sequential engine still runs per-human-turn); it has a complete internal story (watch → read_new → yield) and is roadmap-ordered, but this slice does **not** wire it into the engine — that is the swarm-loop slice.

## Global Constraints

- **Rust edition:** `2021`. Latest stable Rust.
- **Append-only field, unknown-tolerant readers.** `read_new` must honor the same contract as `read_events`: blank lines skipped, un-parseable lines skipped, a torn *final* line (no trailing newline) NOT consumed until complete.
- **Correctness lives in the pure core.** `read_new` gets exhaustive deterministic tests. The `notify` seam gets one thin liveness test with a generous timeout — never make CI depend on tight fs-event timing.
- **Two-process decoupling stays intact.** `read_new` goes in `hadron-lattice` (runtime-free, shared with the chamber). The `notify` seam goes in `hadron-gluon` (the daemon's concern); the chamber does **not** gain a gluon dependency from this slice.
- **The watcher signals "maybe changed"; `read_new` is ground truth.** Coalesce *any* raw event touching the field path into one signal; let `read_new` decide whether there is actually new content (it returns an empty vec if not). Never try to interpret notify event *kinds* semantically.
- **Vocabulary (exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger, block, hash, forge, watch.

## Validated environment facts (confirmed by probe before writing this plan)

- `notify = "8"` (`notify::recommended_watcher(handler)` + `.watch(dir, RecursiveMode::NonRecursive)`) **fires end-to-end** here: appending to a watched file delivered events whose `.paths` included the file, within well under the 3s timeout. `/home/Jake` is WSL2-native ext4, where inotify works.
- **A single append delivers multiple raw events** (e.g. `Access(Open)`, `Modify`, `Close`) — hence coalescing is mandatory; a naive 1-signal-per-event design would over-fire.
- The recommended handler signature is `impl FnMut(notify::Result<notify::Event>) + Send + 'static`; wiring it to a `std::sync::mpsc` channel is the simplest testable bridge.

---

### Task 1: `read_new` — incremental, torn-line-safe field read (pure)

**Files:**
- Modify: `crates/hadron-lattice/src/io.rs`

**Interfaces:**
- Consumes: existing `Event` (de)serialization.
- Produces: `pub fn read_new(path: &std::path::Path, offset: u64) -> std::io::Result<(Vec<Event>, u64)>` — reads bytes after `offset`, parses only *complete* lines (up to the last newline), returns the new events and the byte offset to pass next time. A missing file yields `(vec![], offset)`. A torn final line (bytes after the last newline) is left unconsumed — the returned offset points at the last newline boundary.

- [x] **Step 1: Write the failing tests**

Add to the bottom of `crates/hadron-lattice/src/io.rs` (inside the existing `#[cfg(test)] mod tests`, after the current tests — reuse its `use` imports; add `use std::io::Write;` at the top of the test module if not present):

```rust
    #[test]
    fn read_new_reads_only_appended_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");

        let e1 = Event::new(Actor::Human, Some(QuarkId::new("claude")), Kind::Message { body: "one".into() });
        append_event(&path, &e1).unwrap();

        // First read from offset 0 sees e1.
        let (batch1, off1) = read_new(&path, 0).unwrap();
        assert_eq!(batch1, vec![e1.clone()]);
        assert_eq!(off1, std::fs::metadata(&path).unwrap().len());

        // Nothing new yet.
        let (batch_empty, off_same) = read_new(&path, off1).unwrap();
        assert!(batch_empty.is_empty());
        assert_eq!(off_same, off1);

        // Append e2; only e2 comes back.
        let e2 = Event::new(Actor::Quark(QuarkId::new("claude")), None, Kind::Status { state: QuarkState::Ground });
        append_event(&path, &e2).unwrap();
        let (batch2, off2) = read_new(&path, off1).unwrap();
        assert_eq!(batch2, vec![e2]);
        assert_eq!(off2, std::fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn read_new_missing_file_is_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.jsonl");
        let (batch, off) = read_new(&path, 0).unwrap();
        assert!(batch.is_empty());
        assert_eq!(off, 0);
    }

    #[test]
    fn read_new_does_not_consume_a_torn_final_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");

        // A complete line followed by a torn (newline-less) partial line.
        let e1 = Event::new(Actor::Human, None, Kind::Message { body: "done".into() });
        let complete = serde_json::to_string(&e1).unwrap();
        std::fs::write(&path, format!("{complete}\n{{\"partial\": ")).unwrap();

        let (batch, off) = read_new(&path, 0).unwrap();
        assert_eq!(batch, vec![e1.clone()]);
        // Offset stops at the newline after the complete line, NOT at EOF.
        assert_eq!(off, (complete.len() + 1) as u64);

        // Completing the torn line makes it readable on the next call.
        let e2 = Event::new(Actor::Human, None, Kind::Message { body: "more".into() });
        std::fs::write(&path, format!("{complete}\n{}\n", serde_json::to_string(&e2).unwrap())).unwrap();
        let (batch2, _off2) = read_new(&path, off).unwrap();
        assert_eq!(batch2, vec![e2]);
    }

    #[test]
    fn read_new_skips_blank_and_unparseable_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        let e1 = Event::new(Actor::Human, None, Kind::Message { body: "ok".into() });
        let good = serde_json::to_string(&e1).unwrap();
        // blank line, garbage line, then a good line — all newline-terminated.
        std::fs::write(&path, format!("\n{{bad json\n{good}\n")).unwrap();
        let (batch, off) = read_new(&path, 0).unwrap();
        assert_eq!(batch, vec![e1]);
        assert_eq!(off, std::fs::metadata(&path).unwrap().len());
    }
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo test -p hadron-lattice read_new`
Expected: FAIL to compile — `cannot find function read_new`.

- [x] **Step 3: Write the implementation**

Add to `crates/hadron-lattice/src/io.rs` (after `read_events`). Add `use std::io::{Read, Seek, SeekFrom};` to the existing top-of-file `use std::io::...` line (it currently imports `BufRead, BufReader, Write`; extend it to `use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};`).

```rust
/// Incrementally read events appended after byte `offset`. Returns the newly
/// parsed events and the byte offset to pass on the next call.
///
/// Only *complete* lines (terminated by a newline at or before EOF) are
/// consumed; a torn final line — bytes written after the last newline, e.g. a
/// half-flushed append — is left unread, and the returned offset points at the
/// last newline boundary so the completed line is picked up next time. Blank
/// and un-parseable lines are skipped (same contract as [`read_events`]). A
/// missing file yields `(vec![], offset)`.
pub fn read_new(path: &Path, offset: u64) -> std::io::Result<(Vec<Event>, u64)> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), offset)),
        Err(e) => return Err(e),
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    // Consume only up to and including the last newline; anything after it is a
    // torn final line we must not parse yet.
    let last_nl = match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => i,
        None => return Ok((Vec::new(), offset)), // no complete line yet
    };
    let complete = &buf[..=last_nl];
    let consumed = complete.len() as u64;

    let mut out = Vec::new();
    for line in complete.split(|&b| b == b'\n') {
        let text = match std::str::from_utf8(line) {
            Ok(t) => t.trim(),
            Err(_) => continue,
        };
        if text.is_empty() {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<Event>(text) {
            out.push(ev);
        }
    }
    Ok((out, offset + consumed))
}
```

- [x] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hadron-lattice read_new`
Expected: PASS (4 tests).

- [x] **Step 5: Commit**
```bash
git add crates/hadron-lattice/src/io.rs
git commit -m "feat(lattice): read_new — incremental, torn-line-safe field read"
```

---

### Task 2: `FieldWatcher` — the notify seam (coalesced 0-CPU wake)

**Files:**
- Modify: `crates/hadron-gluon/Cargo.toml`
- Create: `crates/hadron-gluon/src/watch.rs`
- Modify: `crates/hadron-gluon/src/lib.rs` (add `pub mod watch;`)

**Interfaces:**
- Produces:
  - `pub struct FieldWatcher { /* holds the RecommendedWatcher + a std mpsc Receiver<()> */ }`
  - `pub fn FieldWatcher::new(field_path: &Path) -> anyhow::Result<FieldWatcher>` — watches the field file's **parent directory** (non-recursive) and coalesces any raw event whose path matches `field_path` into a `()` signal. (Watching the parent dir, not the file, survives create/rename/atomic-replace.)
  - `pub fn FieldWatcher::wait(&self, timeout: Duration) -> bool` — blocks until at least one change signal arrives (draining any coalesced backlog), returning `true` on a change or `false` on timeout.

- [x] **Step 1: Add the `notify` dependency**

Add to `crates/hadron-gluon/Cargo.toml` under `[dependencies]`:
```toml
notify = "8"
```

- [x] **Step 2: Write `watch.rs` with a thin liveness test**

Create `crates/hadron-gluon/src/watch.rs`:
```rust
//! The 0-CPU field watcher: a thin `notify` seam that coalesces raw filesystem
//! events on `field.jsonl` into a single "the field changed" signal. Correctness
//! of *what* changed lives in [`hadron_lattice::io::read_new`]; this only answers
//! *whether* to re-read. Watches the parent directory (not the file) so it
//! survives create/rename/atomic-replace.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct FieldWatcher {
    // Kept alive for the lifetime of the watcher; dropping it stops watching.
    _watcher: RecommendedWatcher,
    rx: Receiver<()>,
}

impl FieldWatcher {
    /// Start watching `field_path`'s parent directory. Any raw event touching
    /// `field_path` becomes one `()` on the channel.
    pub fn new(field_path: &Path) -> anyhow::Result<FieldWatcher> {
        let field: PathBuf = field_path.to_path_buf();
        let dir = field
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if ev.paths.iter().any(|p| p == &field) {
                    // Ignore send errors: a closed receiver just means we're shutting down.
                    let _ = tx.send(());
                }
            }
        })?;
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        Ok(FieldWatcher { _watcher: watcher, rx })
    }

    /// Block until the field changes or `timeout` elapses. Coalesces: a single
    /// call drains all currently-queued signals so one `wait` maps to one
    /// re-read regardless of how many raw fs events a write produced.
    pub fn wait(&self, timeout: Duration) -> bool {
        match self.rx.recv_timeout(timeout) {
            Ok(()) => {
                // Drain any backlog from the same logical change.
                while self.rx.try_recv().is_ok() {}
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    // Liveness only: proves the notify wire is live in this environment. Correctness
    // of incremental reads is covered deterministically by hadron_lattice::io::read_new.
    #[test]
    fn wait_fires_on_append() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        std::fs::write(&field, "seed\n").unwrap();

        let watcher = FieldWatcher::new(&field).unwrap();
        // Give the watch a moment to register before the write.
        std::thread::sleep(Duration::from_millis(150));

        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&field).unwrap();
            writeln!(f, "another line").unwrap();
            f.flush().unwrap();
        }

        // Generous timeout — a liveness check, not a latency benchmark.
        assert!(watcher.wait(Duration::from_secs(5)), "watcher should fire on append");
    }

    // Negative case kept separate and write-free so it can't race trailing events
    // from a preceding append (the flaky failure mode).
    #[test]
    fn wait_times_out_when_no_changes() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        // Seed write happens BEFORE the watcher exists, so it produces no signal.
        std::fs::write(&field, "seed\n").unwrap();

        let watcher = FieldWatcher::new(&field).unwrap();
        std::thread::sleep(Duration::from_millis(150));

        // No writes at all after watching → wait must time out.
        assert!(!watcher.wait(Duration::from_millis(400)), "watcher should be quiet when idle");
    }
}
```

Add `pub mod watch;` to `crates/hadron-gluon/src/lib.rs` (append after `pub mod snapshot;` / the existing module list).

- [x] **Step 3: Run the test**

Run: `cargo test -p hadron-gluon watch::`
Expected: PASS (2 tests). If `wait_fires_on_append` *times out* rather than passes, inotify is unavailable in the run environment (e.g. a `/mnt/c` mount) — see the bought-land note; the pure `read_new` tests remain the correctness guarantee.

- [x] **Step 4: Commit**
```bash
git add crates/hadron-gluon/Cargo.toml crates/hadron-gluon/src/watch.rs crates/hadron-gluon/src/lib.rs
git commit -m "feat(gluon): FieldWatcher — coalesced 0-CPU notify seam over the field"
```

---

### Task 3: `watch_new_events` — compose watcher + cursor into a 0-CPU stream

**Files:**
- Modify: `crates/hadron-gluon/src/watch.rs`

**Interfaces:**
- Consumes: `FieldWatcher` (Task 2), `hadron_lattice::io::read_new` (Task 1).
- Produces: `pub fn FieldWatcher::next_batch(&self, offset: u64, timeout: Duration) -> anyhow::Result<(Vec<Event>, u64)>` — waits for a change (up to `timeout`), then returns whatever `read_new` yields from `offset` (possibly empty if the change added no complete line yet) and the advanced offset. This is the building block a swarm daemon loops on: `loop { let (evs, off) = w.next_batch(off, ...)?; for e in evs { ... } }`, sleeping at 0 CPU between appends.

- [x] **Step 1: Write the failing test + implementation**

Add to `crates/hadron-gluon/src/watch.rs`:

```rust
use hadron_lattice::io::read_new;
use hadron_lattice::Event;

impl FieldWatcher {
    /// Wait for the field to change (up to `timeout`), then read any newly
    /// appended events via [`read_new`]. Returns `(events, new_offset)`; the
    /// events may be empty if the wait timed out or the change added no complete
    /// line yet. `field_path` is the same path the watcher was created for.
    pub fn next_batch(
        &self,
        field_path: &Path,
        offset: u64,
        timeout: Duration,
    ) -> anyhow::Result<(Vec<Event>, u64)> {
        self.wait(timeout);
        let (events, new_offset) = read_new(field_path, offset)?;
        Ok((events, new_offset))
    }
}
```

Add the test (in the same `#[cfg(test)] mod tests`):

```rust
    #[test]
    fn next_batch_yields_appended_events() {
        use hadron_lattice::{Actor, Kind};
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        std::fs::write(&field, "").unwrap();

        let watcher = FieldWatcher::new(&field).unwrap();
        std::thread::sleep(Duration::from_millis(150));

        // Append a real event via the lattice writer.
        let ev = Event::new(Actor::Human, None, Kind::Message { body: "hi swarm".into() });
        hadron_lattice::io::append_event(&field, &ev).unwrap();

        let (batch, off) = watcher
            .next_batch(&field, 0, Duration::from_secs(5))
            .unwrap();
        assert_eq!(batch, vec![ev]);
        assert_eq!(off, std::fs::metadata(&field).unwrap().len());
    }
```

- [x] **Step 2: Run the test**

Run: `cargo test -p hadron-gluon watch::`
Expected: PASS (3 tests).

- [x] **Step 3: Commit**
```bash
git add crates/hadron-gluon/src/watch.rs
git commit -m "feat(gluon): FieldWatcher::next_batch — 0-CPU watch-then-read building block"
```

---

### Task 4: Workspace green + lint

**Files:** none (verification only).

- [x] **Step 1: Full workspace build & test**

Run: `cargo test --workspace`
Expected: PASS — all pre-existing tests plus 7 new (4 `read_new` + 3 `watch`), 0 failures. (The watch tests exercise real inotify; they pass here per the probe.)

- [x] **Step 2: Lint the changed crates**

Run: `cargo clippy -p hadron-lattice -p hadron-gluon --all-targets`
Expected: no new warnings from `read_new`/`watch`. Fix any that appear.

- [x] **Step 3: Commit any lint fixes (skip if none)**
```bash
git add crates/hadron-lattice/src crates/hadron-gluon/src
git commit -m "chore: clippy clean for field watcher slice"
```

---

## Slice 2 Definition of Done

- `read_new` incrementally reads only newly-appended events, is torn-line-safe, skips blank/un-parseable lines, and returns a resumable byte offset — all covered by deterministic tempfile tests.
- `FieldWatcher` wakes on field growth via `notify`, coalescing the multiple raw events per append into one signal; verified live by a thin liveness test.
- `FieldWatcher::next_batch` composes the two into the 0-CPU `wait → read_new` building block a swarm daemon will loop on.
- `cargo test --workspace` is green; no API spend; no GPUI in the path.
- `read_new` lives in `hadron-lattice` (runtime-free, reusable by the chamber); the `notify` seam lives in `hadron-gluon` and adds no gluon dependency to the chamber.

## Deferred / bought land (explicitly NOT in this slice)

- **Wiring the watcher into a persistent swarm daemon** — the `loop { next_batch → excite addressed quarks }` that turns the request-driven engine into a 0-CPU watch-driven daemon. This is the next Phase 4 slice and the point at which edit-by-hash (slice 1) becomes relevant (concurrent agents editing).
- **`/mnt/c` (Windows-mount) silent no-fire.** On WSL2, inotify works on native ext4 (`/home/...`, verified) but **silently never fires on `/mnt/c` Windows mounts.** Hadron watches `.hadron/field.jsonl` inside the user's active project, which a WSL user could place on `/mnt/c`. A production watcher should detect no-events and fall back to interval polling (the chamber's existing 400ms poll is the natural fallback). Left as bought land; the failure mode is on record here.
- **Chamber adoption of `read_new`** — replacing the chamber's full-re-read-every-400ms tick with incremental `read_new` is an immediate latency/CPU win, but it's a chamber change (Phase 5 surface) kept out of this gluon-focused slice.
- **Debounce window / rename-storm handling** — coalescing here is "drain the backlog on each wait." A time-based debounce (e.g. collapse a burst within 50ms) is a refinement, not needed for correctness since `read_new` is idempotent on offset.
