# Hadron Slice — Plan 4: The GPUI Chamber (viewer + steering)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Hadron a face. The **chamber** is the GPUI desktop app the human watches and steers from: quarks on the left (roster with live state), the field transcript in the centre (chat), and an input box to inject human messages. It is a **pure viewer + writer of `field.jsonl`** — it never links the gluon's runtime. This closes the vertical slice: a human can watch two labs' agents coordinate and jump in.

**Architecture:** A new binary crate `hadron-chamber` that pins `gpui = "0.2"`. The two processes stay decoupled through the file exactly as the spec demands: the chamber **reads** `field.jsonl` to render and **appends** a human `Event` to steer — it shares nothing with `hadron-gluon` except `hadron-lattice` (the schema) and the file path. The render-critical logic — turning a `Vec<Event>` into a view-model (message list + derived roster state) — is a **pure, unit-tested function in `hadron-lattice`**; only the GPUI rendering itself is untested (it needs a display).

**Tech Stack:** Rust (edition 2021), `gpui = "0.2"` (crates.io, Apache-2.0, self-contained), plus `hadron-lattice`. Reuses `hadron-gluon::field` append/read via a thin dependency, or re-reads directly through lattice.

**This is Plan 4 of 4** for the Hadron vertical slice (spec: `docs/superpowers/specs/2026-07-10-hadron-vertical-slice-design.md`). Plans 1–3 built the schema, engine, git safety, nucleus, and real adapters. This plan is the viewer.

> **Execution status (2026-07-11):** Tasks 1–2 **executed and committed** (runtime-free field IO + pure view-model — 8 IO + 4 model tests green, zero GPUI in the test path). Task 3 (GPUI window) **code written and it compiles against the real gpui 0.2.2 API**, but the crate **cannot link** in this environment: `rust-lld: unable to find library -lxkbcommon / -lxkbcommon-x11`. The runtime libs (`libxkbcommon.so.0`, `libxkbcommon-x11.so.0`) are present; only the `-dev` symlink packages are missing. **Fix (needs sudo):** `sudo apt install libxkbcommon-dev libxkbcommon-x11-dev`, then `cargo run -p hadron-chamber --features gui -- <field.jsonl>`. gpui itself, fontconfig, freetype, and the Vulkan/Blade renderer all compiled and linked — only these two X keyboard dev libs block. Tasks 4–5 (input, live tail) build on the Task 3 window and are **not started** (they need the window to run for verification). The `gui` feature is off by default, so the default workspace build/tests remain fully green.

## ⚠️ Platform reality (read before running)

- **GPUI is pre-1.0 and roughest on Linux; most mature on macOS.** On WSL2/Linux a display server (X/Wayland) is required; a headless CI box cannot render the window.
- **The GPUI window cannot be unit-tested.** This plan deliberately pushes all testable logic into a pure view-model (Tasks 1–2, fully tested) and keeps Tasks 3–5 (rendering, input, tailing) as **manual verification** steps.
- **Build cost:** first `gpui` build pulls a large dependency tree and is slow. Budget for it.
- **This is the one plan best executed on the user's primary desktop platform**, not a headless agent — verification is visual.

## Global Constraints

- **Two-process decoupling is absolute.** The chamber depends on `hadron-lattice` (schema) and reads/writes the field file. It **must not** depend on `hadron-gluon`'s engine, adapters, or tokio runtime — GPUI has its own executor and mixing them in-process is the exact conflict the two-process split exists to avoid. (Reusing the tiny pure `field::{append_event, read_events}` IO helpers is acceptable *only* if they are moved to or duplicated in a runtime-free location; simplest is to read/write via `hadron-lattice` serde directly in the chamber.)
- **The chamber is a citizen, not a controller.** It steers by appending a `Actor::Human` `Event` — the same mechanism a quark uses. It has no privileged path into the engine.
- **Append-only, unknown-tolerant.** The chamber renders unknown `Kind::Unknown` events gracefully (e.g. a muted "unrecognized event" row) rather than hiding or crashing — same forward-compat contract as the readers.
- **Vocabulary (exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite.

---

### Task 1: Field IO available to the chamber without the gluon runtime

**Files:**
- Create: `crates/hadron-lattice/src/io.rs`
- Modify: `crates/hadron-lattice/src/lib.rs`
- (Later) `crates/hadron-gluon/src/field.rs` may delegate to this to avoid duplication.

**Interfaces:**
- Produces (lattice, runtime-free): `io::read_events(path: &Path) -> std::io::Result<Vec<Event>>` and `io::append_event(path: &Path, event: &Event) -> std::io::Result<()>` — the same semantics as Plan 1's gluon `field` module (missing → empty, blank/torn lines skipped, append-only), but with **no tokio/anyhow dependency** so the chamber can use them.

- [ ] **Step 1: Move the pure field IO into lattice** (identical logic to `hadron-gluon::field`), with the same four tests (append/read order, missing→empty, unknown-kind survives, torn line skipped). Then have `hadron-gluon::field` re-export or delegate to `hadron_lattice::io` so there is one implementation. Confirm Plan 1's gluon field tests still pass unchanged.

- [ ] **Step 2: Run tests** — `cargo test -p hadron-lattice io::` and `cargo test -p hadron-gluon field::` → both PASS.

- [ ] **Step 3: Commit** — `refactor(lattice): runtime-free field IO shared with the chamber`

---

### Task 2: The chamber view-model (pure, tested)

**Files:**
- Create: `crates/hadron-chamber/Cargo.toml`
- Create: `crates/hadron-chamber/src/model.rs`
- Create: `crates/hadron-chamber/src/main.rs` (stub for now)
- Modify: root `Cargo.toml` (add `crates/hadron-chamber` to members)

**Interfaces:**
- Produces: `struct ChamberView { messages: Vec<MessageRow>, roster: Vec<RosterRow> }`; `struct MessageRow { from: String, to: Option<String>, body: String, kind_label: &'static str }`; `struct RosterRow { id: String, state: QuarkState }`; `fn project(events: &[Event]) -> ChamberView` — derives the chat rows (Message + a compact label for Status/Edit/Command/Snapshot/Unknown) and the current per-quark state (latest `Kind::Status` per author, defaulting to `Ground`).

- [ ] **Step 1: Write `project` + tests.** Tests assert: a Message event becomes a `MessageRow` with correct from/to/body; a `Kind::Status { Excited }` from `agy` sets that roster row's state to `Excited`, and a later `Ground` overrides it (latest wins); an `Kind::Unknown` event becomes a muted row (`kind_label == "unrecognized"`) rather than being dropped; roster reflects every distinct quark that has authored or been addressed.

Cargo.toml pins:
```toml
[package]
name = "hadron-chamber"
version = "0.1.0"
edition = "2021"

[dependencies]
hadron-lattice = { path = "../hadron-lattice" }
gpui = "0.2"

[[bin]]
name = "hadron-chamber"
path = "src/main.rs"
```
`main.rs` for now: a stub that reads the field path from `argv[1]`, calls `project`, and prints the row counts (proves the model links without GPUI). GPUI wiring is Task 3.

- [ ] **Step 2: Run tests** — `cargo test -p hadron-chamber model::` → PASS. (Model tests don't touch GPUI.)

- [ ] **Step 3: Commit** — `feat(chamber): pure view-model projecting the field into chat + roster`

---

### Task 3: The GPUI window — roster | chat (manual verification)

**Files:**
- Modify: `crates/hadron-chamber/src/main.rs`
- Create: `crates/hadron-chamber/src/app.rs`

**Interfaces:** a GPUI `App`/root view rendering `ChamberView`: a left column listing `roster` rows (id + state chip), a centre column listing `messages` rows (from → to, body). Static snapshot first (load once, render).

- [ ] **Step 1: Build the GPUI app** rendering a `ChamberView` loaded once from the field path. Follow the `gpui = "0.2"` render/element API (verify against the installed crate docs — GPUI's API moves pre-1.0). Keep styling minimal: a two-column flex, roster chips coloured by state.

- [ ] **Step 2: Manual verification** (no automated test — needs a display): run `cargo run -p hadron-chamber -- <path-to-a-field.jsonl>` against a field file produced by a Plan 1/2 engine test or a hand-written sample. Confirm the roster and chat render. Capture a screenshot for the record.

- [ ] **Step 3: Commit** — `feat(chamber): GPUI window rendering roster + chat (static)`

---

### Task 4: Human steering — the input box appends to the field

**Files:**
- Modify: `crates/hadron-chamber/src/app.rs`

**Interfaces:** a text input at the bottom of the chat column; on submit, append an `Event::new(Actor::Human, to, Kind::Message { body })` to the field via `hadron_lattice::io::append_event`, where `to` is parsed from a leading `@quarkid` (reuse the same convention; a small local parse or a shared helper).

- [ ] **Step 1: Wire the input.** On Enter, append the human message, then reload + re-render the view so the new row appears. Parse a leading `@mention` into `to` so the human can address a specific quark (and the gluon will excite it on its next tick).

- [ ] **Step 2: Manual verification:** type `@claude hello` and submit; confirm a new human row appears in the chat AND a new line is present in `field.jsonl` (tail the file). This proves the chamber steers through the same bus as the quarks.

- [ ] **Step 3: Commit** — `feat(chamber): human input appends steering messages to the field`

---

### Task 5: Live tail — refresh as the field grows

**Files:**
- Modify: `crates/hadron-chamber/src/app.rs`

**Interfaces:** a periodic (or notify-based) re-read of the field so the chamber updates as the gluon appends quark turns.

- [ ] **Step 1: Add live refresh.** Simplest v1: a GPUI timer that re-reads the field on an interval (e.g. 300–500ms) and re-projects; only re-render when the event count changed. (A filesystem-notify upgrade is bought land.) Keep it a dumb full re-read — the field is small and this matches the engine's own v1 "re-read the whole field" posture.

- [ ] **Step 2: Manual verification (the slice, end to end):** in one terminal run a real (or mock) engine turn that appends to a field file; with the chamber open on that file, confirm new quark messages and roster-state changes appear within the refresh interval without interaction.

- [ ] **Step 3: Commit** — `feat(chamber): live field tail (interval re-read)`

---

## Plan 4 Definition of Done

- `cargo test` passes for the pure parts (`hadron-lattice::io`, `hadron-chamber::model`) with **zero GPUI in the test path**.
- The chamber builds and renders a `field.jsonl` as roster + chat on the user's desktop platform (manual verification + screenshot).
- The human can append a steering message (optionally `@`-addressed) from the input box, and it lands in the field as an `Actor::Human` event.
- The chamber live-refreshes as the field grows.
- The two processes remain decoupled: `hadron-chamber` depends only on `hadron-lattice` + the file, never on `hadron-gluon`'s runtime.

## Notes / bought land (deferred past the slice)

- **Right sidebar (preview / diff / changes)** from the spec's chamber sketch — render `Kind::Edit`/`Snapshot`/diff into a side panel. Deferred; the events already carry the data.
- **Tabbed chat/log view** — the spec's centre-panel tabs. v1 is a single chat column.
- **filesystem-notify** instead of interval polling — lower latency, more code. Bought land.
- **Snapshot restore from the UI** — a "revert to here" affordance on `Kind::Snapshot` rows calling `snapshot::restore` (Plan 2). This crosses the process boundary (the chamber would need to trigger a gluon action) — belongs with the **remote-control seam** (spec §15), so it waits for that transport rather than reaching into the engine directly.
- **Matrix / multi-field view** — watching several projects at once. Bought land.
