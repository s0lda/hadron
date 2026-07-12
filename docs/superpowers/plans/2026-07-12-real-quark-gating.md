# Real-Quark Gating Implementation Plan

> **For agentic workers:** implement task-by-task, TDD, commit per task. Steps use `- [ ]`.

**Goal:** Make the resolved permission mode select each real CLI quark's permission posture at invocation time.

**Architecture:** `Projection` carries the resolved `Mode`; the engine resolves it before `excite`; each adapter maps mode → CLI argv via a pure, unit-tested function. Zero API spend in tests (mock `FakeRunner`).

**Tech Stack:** Rust workspace; `hadron-lattice` (Projection/Mode), `hadron-gluon` (engine + adapters), `hadron-gatekeeper` (`resolve_mode`).

## Global Constraints

- Zero API spend in the automated test suite — all adapter assertions use `FakeRunner`.
- Do not change the mode vocabulary, field-as-SSOT model, or chamber UI.
- Auto degrades to Write's posture (safety), never `acceptEdits`-all-bash.
- Mapping verbatim (claude): Ask→`--permission-mode plan`; Write & Auto→`--permission-mode acceptEdits --disallowedTools Bash`; Bypass→`--permission-mode bypassPermissions`.
- Mapping verbatim (agy): Ask→`--mode plan`; Write & Auto→`--mode accept-edits`; Bypass→`--dangerously-skip-permissions`.

---

### Task 1: `Projection.mode`

**Files:** Modify `crates/hadron-lattice/src/projection.rs`; fix construction sites in `crates/hadron-gluon/src/engine.rs` and adapter/test helpers.

**Interfaces:** Produces `Projection.mode: Mode` (public field, `#[serde(default)]`).

- [ ] Add `pub mode: Mode` to `Projection` with `#[serde(default)]`; `use crate::Mode`.
- [ ] Fix every `Projection { .. }` literal (engine, adapter tests) to set `mode: Mode::default()` (or a test value).
- [ ] Test in projection.rs: a `Projection` round-trips with a non-default mode; a JSON blob without `mode` deserializes to `Mode::Ask`.
- [ ] `cargo test -p hadron-lattice` green. Commit.

### Task 2: engine resolves mode before `excite`

**Files:** Modify `crates/hadron-gluon/src/engine.rs` (the `run_until_quiesce` projection-build site, ~line 233).

**Interfaces:** Consumes `gatekeeper::resolve_mode(&events, &quark_id)`. Produces: the `Projection` handed to `excite` has `mode` = the quark's resolved mode.

- [ ] Before building the projection, compute `let mode = hadron_gatekeeper::resolve_mode(&events, &quark_id);` for the quark being excited.
- [ ] Set `mode` on the projection literal.
- [ ] Test: a spy quark records the `Projection.mode` it received; seed a per-quark `ModeSet{Bypass}` and assert the excited quark's projection carried `Mode::Bypass`; with no ModeSet, `Mode::Ask`.
- [ ] `cargo test -p hadron-gluon` green. Commit.

### Task 3: claude adapter posture mapping + token fix

**Files:** Modify `crates/hadron-gluon/src/adapter/claude.rs`.

- [ ] Add free fn `fn posture_args(mode: Mode) -> Vec<String>` implementing the claude mapping (Global Constraints).
- [ ] In `invocation`, take the turn's mode and append `posture_args(mode)`. Thread the mode from `excite` (from `turn.mode`) into `invocation`.
- [ ] Fix token extraction: `input_tokens + output_tokens` from `usage`.
- [ ] Capture `permission_denials` defensively (comment: always `[]` headless today; SDK-path hook).
- [ ] Tests (FakeRunner): each mode yields the right flags — plan for Ask; `acceptEdits`+`--disallowedTools Bash` for Write and Auto; `bypassPermissions` for Bypass. Token test: envelope with `input_tokens:10,output_tokens:5` → `used_tokens==15`.
- [ ] `cargo test -p hadron-gluon` green. Commit.

### Task 4: agy adapter posture mapping

**Files:** Modify `crates/hadron-gluon/src/adapter/agy.rs`.

- [ ] Add `fn posture_args(mode: Mode) -> Vec<String>` implementing the agy mapping (Global Constraints).
- [ ] Thread `turn.mode` → invocation, append `posture_args`.
- [ ] Tests (FakeRunner): each mode yields the right agy flags.
- [ ] Add a `// NEEDS LIVE VALIDATION` comment (finicky parse; display-name models).
- [ ] `cargo test -p hadron-gluon` green. Commit.

### Task 5: config template + records

**Files:** Modify `docs/superpowers/plans/team.example.json`; update `STATUS.md` note.

- [ ] Fix `team.example.json`: claude model → `opus` (alias; note full ids accepted); agy model → a real display-name id (e.g. `Gemini 3.1 Pro (High)`), with a comment that `agy models` lists them.
- [ ] Add a short "Real-quark gating" section to `STATUS.md`: what modes now do to real quarks, the turn-granular escalation flow, and the deferred SDK-`canUseTool` upgrade for true Auto.
- [ ] Commit.

## Self-Review

- Spec coverage: mapping (T3/T4), plumbing (T1/T2), token fix (T3), template/records (T5), Auto-degrades-to-Write (T3/T4 constraints). Deferred SDK path documented, not built. ✓
- Type consistency: `posture_args(Mode) -> Vec<String>` in both adapters; `Projection.mode: Mode`; `resolve_mode(&[Event], &QuarkId) -> Mode`. ✓
