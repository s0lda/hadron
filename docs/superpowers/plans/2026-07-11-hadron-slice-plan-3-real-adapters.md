# Hadron Slice — Plan 3: Real Quark Adapters (Claude + Antigravity)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `MockQuark` with the two real CLI citizens the user actually runs — **Claude Code** (`claude`) and **Antigravity** (`agy`) — so the engine coordinates *live* models through the field. This is the plan that tests the thesis end-to-end: two agents from two labs self-organizing over a shared bus. It is also the **first plan that spends money and spawns external processes.**

**Architecture:** Add an `adapter` layer to `hadron-gluon`. The Projection is rendered to a Markdown **prompt** (pure function); the prompt is handed to a CLI behind a small **`CliRunner` seam**; the CLI's Markdown output becomes the `TurnOutcome`. `ClaudeQuark` and `AgyQuark` are thin `Quark` impls over that seam, differing only in how they invoke their CLI and whether they carry a resumable session. **All coordination logic stays testable with a `FakeRunner`; real CLIs are exercised only by `#[ignore]`d smoke tests.**

**Tech Stack:** Rust (edition 2021), `tokio::process::Command` for async subprocess, plus Plan 1–2 deps. Dev: `tempfile`.

**This is Plan 3 of 4** for the Hadron vertical slice (spec: `docs/superpowers/specs/2026-07-10-hadron-vertical-slice-design.md`). Plan 1 built the schema+engine; Plan 2 added git safety + nucleus. Plan 4 is the GPUI chamber.

> **Execution status (2026-07-11):** Tasks 1–5 **executed and committed** (zero-spend, all `FakeRunner`/pure — 11 adapter tests green). Task 6 (live smoke tests — real budget + CLIs) **not yet run**; deliberately held for a human-present session. Two design corrections were made during execution: (a) `CliResult` dropped `session_id` — session parsing belongs in the Claude adapter, not the generic runner; (b) added tokio `process`/`io-util` features for `ProcessRunner`.

## ⚠️ Execution safety (read before running)

- **This plan spends real API/subscription budget and spawns `claude` / `agy` subprocesses that can edit files.** Run it with a human present, in a throwaway target repo first.
- **Always execute with git safety on** (`Engine::with_git`, Plan 2): the snapshot-before-excite is the undo net if a live model does something unwanted.
- **Keep `max_exchanges` low** (e.g. 4–6) for the first live runs — the backstop is the cost ceiling per human turn.
- **The unit tests in this plan cost nothing** (they use `FakeRunner`). Only the `#[ignore]`d smoke tests (Task 6) invoke real CLIs; run those deliberately.
- **CLI flags must be verified at execution time.** The exact `claude` / `agy` headless flags below are the design intent; confirm them against the installed CLI versions before the first live run (the `CliRunner` seam localizes any change).

## Global Constraints

- **Transport vs content:** the field carries JSONL envelopes; model I/O is **Markdown**. The adapter is the translator — it renders a Markdown prompt and stores the Markdown reply as an `Event`'s `Kind::Message { body }`. (Antigravity cannot emit structured JSON, which is exactly why the body is Markdown, not a typed payload.)
- **The `CliRunner` trait is the only place a subprocess is spawned.** Prompt-building and reply-handling are pure and unit-tested; the process boundary is faked in tests and real only in production / `#[ignore]`d tests.
- **Delegation stays `@mention`-based** (Plan 1 `router::parse_addressee`). The Invariants preamble MUST instruct real quarks to (a) address teammates with a leading `@quarkid`, and (b) hand back to the human (no `@mention`) when the task is done — this is what makes the engine quiesce.
- **Reserved names enforced at registration:** a quark id must not be `human` or `gluon` (Plan 1 note). Registration returns an error on collision.
- **File edits are the CLI's job; capture is the gluon's.** Real quarks edit files in the target repo with their own tools. The gluon snapshots before excite and derives the diff after (Plan 2). The adapter does not place files in v1.
- **Session continuity is per-quark and opt-in.** `ClaudeQuark` resumes its session across turns (`--resume <id>`); `AgyQuark` runs one-shot per turn in v1. The seam carries session state so this is swappable.
- **Vocabulary (exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, adapter.

---

### Task 1: The prompt builder (pure)

**Files:**
- Create: `crates/hadron-gluon/src/adapter/mod.rs`
- Create: `crates/hadron-gluon/src/adapter/prompt.rs`
- Modify: `crates/hadron-gluon/src/lib.rs` (add `pub mod adapter;`)

**Interfaces:**
- Produces: `adapter::prompt::build(projection: &Projection) -> String` — a deterministic Markdown prompt assembling, in order: Invariants preamble, staleness/nucleus digest, the current task, a rendered recent-field transcript, and the working diff.

- [ ] **Step 1: Write the builder + tests**

Create `crates/hadron-gluon/src/adapter/prompt.rs`:
```rust
use hadron_lattice::{Actor, Kind, Projection};

/// Render one field event as a Markdown transcript line: `**from → to:** body`.
fn render_event_line(from: &Actor, to: &Option<hadron_lattice::QuarkId>, body: &str) -> String {
    let from_s = match from {
        Actor::Human => "human".to_string(),
        Actor::Gluon => "gluon".to_string(),
        Actor::Quark(q) => q.as_str().to_string(),
    };
    match to {
        Some(t) => format!("**{from_s} → {}:** {body}", t.as_str()),
        None => format!("**{from_s}:** {body}"),
    }
}

/// Build the full Markdown prompt handed to a quark's CLI for one turn.
/// Deterministic and side-effect-free so it can be unit-tested exactly.
pub fn build(projection: &Projection) -> String {
    let mut p = String::new();

    // 1. Invariants — the enforced working protocol.
    if !projection.invariants.trim().is_empty() {
        p.push_str("# Working protocol (Invariants)\n");
        p.push_str(projection.invariants.trim());
        p.push_str("\n\n");
    }

    // 2. Nucleus digest — project SSOT context.
    if !projection.nucleus_digest.trim().is_empty() {
        p.push_str("# Project knowledge (nucleus)\n");
        p.push_str(projection.nucleus_digest.trim());
        p.push_str("\n\n");
    }

    // 3. The task.
    p.push_str("# Your task\n");
    p.push_str(projection.task.trim());
    p.push_str("\n\n");

    // 4. Recent field transcript.
    if !projection.field_window.is_empty() {
        p.push_str("# Recent field (most recent last)\n");
        for e in &projection.field_window {
            if let Kind::Message { body } = &e.kind {
                p.push_str(&render_event_line(&e.from, &e.to, body));
                p.push('\n');
            }
        }
        p.push('\n');
    }

    // 5. Working diff.
    if !projection.git_diff.trim().is_empty() {
        p.push_str("# Current working diff\n```diff\n");
        p.push_str(projection.git_diff.trim());
        p.push_str("\n```\n\n");
    }

    // 6. Handoff reminder — how to keep the loop coordinating / quiescing.
    p.push_str("# How to respond\n");
    p.push_str(
        "Reply in Markdown. To delegate, start a line with `@<quark-id>` and the request. \
         When the overall task is complete, reply WITHOUT any `@mention` to hand control back \
         to the human.\n",
    );

    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Event, QuarkCard, QuarkId, EnergyState, Flavor};

    fn projection(task: &str) -> Projection {
        Projection {
            task: task.into(),
            invariants: "Snapshot before editing. Use @mentions.".into(),
            nucleus_digest: "## map.md\nauth lives in src/auth".into(),
            roster: vec![QuarkCard {
                id: QuarkId::new("agy"),
                flavor: Flavor::Worker,
                energy: EnergyState::Available,
            }],
            field_window: vec![Event::new(
                Actor::Human,
                Some(QuarkId::new("claude")),
                Kind::Message { body: "start the auth work".into() },
            )],
            git_diff: String::new(),
        }
    }

    #[test]
    fn prompt_contains_all_sections() {
        let p = build(&projection("Build login"));
        assert!(p.contains("# Working protocol (Invariants)"));
        assert!(p.contains("Snapshot before editing"));
        assert!(p.contains("# Project knowledge (nucleus)"));
        assert!(p.contains("auth lives in src/auth"));
        assert!(p.contains("# Your task"));
        assert!(p.contains("Build login"));
        assert!(p.contains("# Recent field"));
        assert!(p.contains("**human → claude:** start the auth work"));
        assert!(p.contains("@<quark-id>"));
    }

    #[test]
    fn empty_optional_sections_are_omitted() {
        let mut proj = projection("t");
        proj.invariants = String::new();
        proj.nucleus_digest = String::new();
        proj.git_diff = String::new();
        let p = build(&proj);
        assert!(!p.contains("Invariants"));
        assert!(!p.contains("nucleus"));
        assert!(!p.contains("working diff"));
        assert!(p.contains("# Your task"));
    }
}
```

Create `crates/hadron-gluon/src/adapter/mod.rs`:
```rust
pub mod prompt;
```

Update `crates/hadron-gluon/src/lib.rs`:
```rust
pub mod adapter;
pub mod engine;
pub mod field;
pub mod mock;
pub mod nucleus;
pub mod quark;
pub mod router;
pub mod snapshot;
```

- [ ] **Step 2: Run tests** — `cargo test -p hadron-gluon adapter::prompt::` → PASS (2 tests). No spend.

- [ ] **Step 3: Commit** — `feat(gluon): Markdown prompt builder for real adapters`

---

### Task 2: The `CliRunner` seam + reply handling

**Files:**
- Create: `crates/hadron-gluon/src/adapter/runner.rs`
- Modify: `crates/hadron-gluon/src/adapter/mod.rs`

**Interfaces:**
- Produces:
  - `struct CliInvocation { program: String, args: Vec<String>, stdin: String }`.
  - `struct CliResult { stdout: String, session_id: Option<String>, exit: i32 }`.
  - `#[async_trait] trait CliRunner: Send + Sync { async fn run(&self, inv: CliInvocation) -> anyhow::Result<CliResult>; }`.
  - `struct ProcessRunner` — the production impl over `tokio::process::Command`.
  - `struct FakeRunner { replies: Mutex<VecDeque<CliResult>> }` (test-only, `#[cfg(any(test, feature = "test-util"))]`) recording the invocations it received.
  - `adapter::runner::reply_to_outcome(result: &CliResult) -> TurnOutcome` — trims stdout; empty → `TurnOutcome { message: None }`.

- [ ] **Step 1: Write the seam + `ProcessRunner` + `FakeRunner` + reply handling**, with unit tests that: (a) `reply_to_outcome` maps non-empty stdout → `Some`, empty/whitespace → `None`; (b) `FakeRunner` returns queued replies in order and records invocations. `ProcessRunner` spawns `tokio::process::Command`, writes `stdin`, waits, captures stdout/exit. **`ProcessRunner` is NOT unit-tested here** (that would spawn processes) — it is covered by Task 6's `#[ignore]`d tests.

- [ ] **Step 2: Run tests** — `cargo test -p hadron-gluon adapter::runner::` → PASS. No spend (FakeRunner only).

- [ ] **Step 3: Commit** — `feat(gluon): CliRunner seam (Process/Fake) + reply handling`

---

### Task 3: `ClaudeQuark` (resumable session)

**Files:**
- Create: `crates/hadron-gluon/src/adapter/claude.rs`
- Modify: `crates/hadron-gluon/src/adapter/mod.rs`

**Interfaces:**
- Produces: `struct ClaudeQuark<R: CliRunner> { id, flavor, runner, session: Option<String> }` implementing `Quark`. `excite` builds the prompt (Task 1), constructs a `CliInvocation` (headless print mode; on the first turn start a session, on later turns `--resume <session>`), runs it via the seam, stores the returned `session_id`, and maps the reply to a `TurnOutcome`.

**CLI intent (verify at execution):** headless print mode with a machine-readable envelope so a session id can be recovered, e.g. `claude -p --output-format json` (first turn) and `claude -p --resume <id> --output-format json` (later turns), prompt on stdin. If a JSON envelope with a session id is unavailable in the installed version, fall back to `--continue` semantics and set `session: None` (each turn continues the most recent session). **This decision lives entirely in `claude.rs` + `ProcessRunner`; nothing else changes.**

- [ ] **Step 1: Write `ClaudeQuark` + tests using `FakeRunner`.** Tests assert: first `excite` issues an invocation WITHOUT `--resume` and captures the session id from the fake reply; the second `excite` issues an invocation WITH `--resume <captured-id>`; the returned `TurnOutcome.message` equals the fake stdout. No real CLI.

- [ ] **Step 2: Run tests** — `cargo test -p hadron-gluon adapter::claude::` → PASS. No spend.

- [ ] **Step 3: Commit** — `feat(gluon): ClaudeQuark adapter with resumable session (faked in tests)`

---

### Task 4: `AgyQuark` (one-shot per turn)

**Files:**
- Create: `crates/hadron-gluon/src/adapter/agy.rs`
- Modify: `crates/hadron-gluon/src/adapter/mod.rs`

**Interfaces:**
- Produces: `struct AgyQuark<R: CliRunner> { id, flavor, runner }` implementing `Quark`. `excite` builds the prompt and runs `agy` in one-shot print mode, mapping the Markdown reply to a `TurnOutcome`. No session state in v1 (Antigravity's continuity model is a later concern; the prompt already carries the recent field + diff, so each turn is self-contained).

**CLI intent (verify at execution):** `agy --print` (per the agy one-shot mode), prompt on stdin, Markdown on stdout.

- [ ] **Step 1: Write `AgyQuark` + tests using `FakeRunner`** asserting the invocation targets `agy` with the print flag and the reply maps through. No real CLI.

- [ ] **Step 2: Run tests** — `cargo test -p hadron-gluon adapter::agy::` → PASS. No spend.

- [ ] **Step 3: Commit** — `feat(gluon): AgyQuark one-shot adapter (faked in tests)`

---

### Task 5: Registration with reserved-name enforcement

**Files:**
- Create: `crates/hadron-gluon/src/adapter/registry.rs`
- Modify: `crates/hadron-gluon/src/adapter/mod.rs`

**Interfaces:**
- Produces: `fn validate_quark_id(id: &QuarkId) -> anyhow::Result<()>` (errors on `human`/`gluon` or empty/whitespace); a `QuarkSpec` enum/struct describing which adapter + flavor to build; `fn build(spec: QuarkSpec) -> anyhow::Result<Box<dyn Quark>>` wiring a real adapter over a `ProcessRunner`. The engine assembly (a later daemon binary) calls this once per configured quark.

- [ ] **Step 1: Write validation + build + tests.** Tests: `validate_quark_id` rejects `human`, `gluon`, `""`, `"  "`, and accepts `claude`, `agy`; `build` on a spec returns a boxed quark whose `id()`/`flavor()` match the spec. (Build wires a `ProcessRunner` but does not invoke it, so still no spend.)

- [ ] **Step 2: Run tests** — `cargo test -p hadron-gluon adapter::registry::` → PASS. No spend.

- [ ] **Step 3: Commit** — `feat(gluon): quark registration with reserved-name enforcement`

---

### Task 6: Live smoke tests (opt-in, real CLIs, real spend)

**Files:**
- Create: `crates/hadron-gluon/tests/live_adapters.rs` (integration test, all `#[ignore]`d)

**Interfaces:** none new — exercises the real `ProcessRunner` path end-to-end.

- [ ] **Step 1: Write `#[ignore]`d integration tests** that:
  1. `claude_answers_a_trivial_prompt` — build a tiny Projection ("Reply with the single word READY, no @mention"), run a real `ClaudeQuark`, assert the `TurnOutcome.message` is non-empty. Skips (not fails) if `claude` is not on PATH.
  2. `agy_answers_a_trivial_prompt` — same for `AgyQuark` / `agy`.
  3. `two_real_quarks_coordinate_and_quiesce` — a real end-to-end: temp git repo (Plan 2 git safety ON), `max_exchanges = 4`, seed a human message asking the orchestrator to greet the worker and hand back; run `Engine::run_until_quiesce`; assert the field ends with a human-directed (no-`@mention`) message and that the loop quiesced within the backstop. **This is the thesis test.**

- [ ] **Step 2: Document how to run** in the test file's top comment: `cargo test -p hadron-gluon --test live_adapters -- --ignored --nocapture`. State plainly: **this spends real budget and spawns the CLIs.**

- [ ] **Step 3: Verify the non-live suite is still green and free** — `cargo test` (ignored tests skipped) → PASS, zero spend.

- [ ] **Step 4: Commit** — `test(gluon): opt-in live adapter smoke tests (ignored by default)`

---

## Plan 3 Definition of Done

- `cargo test` (default) passes with **zero API spend** — all adapter logic covered via `FakeRunner`.
- The Projection renders to a deterministic Markdown prompt carrying invariants, nucleus, task, recent field, and diff.
- `ClaudeQuark` starts and resumes a session across turns; `AgyQuark` runs one-shot — both behind the `CliRunner` seam.
- Reserved names (`human`/`gluon`) are rejected at registration.
- `#[ignore]`d live tests exist and, when run deliberately with the CLIs installed, demonstrate two real quarks coordinating over the field and quiescing within the backstop.
- No GPUI yet — Plan 4.

## Notes for later plans

- **The daemon binary** (could be Plan 4's sibling or its own step) composes: load config → `adapter::registry::build` per quark → `nucleus::load`/`digest` → `Engine::new().with_git(repo).with_nucleus(digest)` → `run_until_quiesce` on each human turn. Plan 3 provides all the pieces; the binary is glue.
- **Session robustness** (claude session id capture) is the most likely thing to need adjustment against a real CLI version — it is deliberately isolated to `claude.rs` + `ProcessRunner`.
- **Timeouts / cancellation:** `ProcessRunner` should grow a per-turn timeout before heavy live use (a hung CLI otherwise stalls the loop). Wire it when moving past smoke tests.
- **Energy/session-limit detection** (spec §13) can be derived from CLI exit codes / stderr patterns in `ProcessRunner` and surfaced as `EnergyState::Depleted` — bought land, not needed for the thesis test.

## Watch-items for the live run (Task 6)

- **`ProcessRunner` now errors on nonzero exit with stderr embedded** (added during Task 1–5 execution; unit-tested via `cat`/`sh`). This means a failed live CLI call surfaces as a real `Err` (auth expired, rate-limit, unknown flag) instead of a silent empty turn. When wiring the daemon, decide whether an excite error should abort the human turn or append a gluon error message to the field and quiesce.
- **Claude session double-sends context.** `ClaudeQuark` resumes the session (`--resume`) *and* the prompt re-injects the full `field_window` every turn, so from turn 2 the model sees prior turns twice (session memory + re-sent transcript). This is intentional for v1 symmetry with sessionless `agy`, but watch it live: if Claude gets confused or token cost balloons, the fix is a delta-vs-full-window split in the prompt builder for resumable adapters. Do not change on spec — validate first.
