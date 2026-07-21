---
author: agy
status: draft
---

# Route Gluon Execution & Turn Errors to Orchestrator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically append an error message event addressed to `@orchestrator` whenever a Quark turn fails, panics, or encounters a Gluon excite error.

**Architecture:** Update `Engine::run_until_quiesce` in `crates/hadron-gluon/src/engine/run.rs` to append a `Kind::Message` event addressing `@orchestrator` when a turn fails or panics, and update `crates/hadron-gluon/src/bin/hadron-gluon.rs` to log excite errors to the field log.

**Tech Stack:** Rust (hadron-gluon, hadron-lattice).

## Global Constraints
- Failed turn errors and excite errors must append a `Kind::Message` event from `Actor::Gluon`.
- If an orchestrator seat exists on the roster and the failed quark is NOT the orchestrator, the message body MUST prefix `@orchestrator`.
- If the failing quark is the orchestrator itself or no orchestrator exists, do NOT prefix `@orchestrator` (prevents self-loop).

---

### Task 1: Add Error Message Event Generation to Engine Turn Execution

**Files:**

- Modify: `crates/hadron-gluon/src/engine/run.rs:360-415`
- Test: `crates/hadron-gluon/src/engine/tests.rs`

**Interfaces:**

- Consumes: `Engine::run_until_quiesce`, `Event`, `Actor::Gluon`, `Kind::Message`.
- Produces: Error notification events appended to the field log addressing `@orchestrator`.

- [ ] **Step 1: Check baseline hadron-gluon tests pass**

Run: `cargo test -p hadron-gluon`
Expected: PASS

- [ ] **Step 2: Implement helper and error event emission in `src/engine/run.rs`**

Add helper method `format_error_message` on `Engine`:

```rust
fn format_error_message(&self, quark_id: &QuarkId, err: &anyhow::Error) -> String {
    let orchestrator = self.roster.iter().find(|c| c.flavor == Flavor::Orchestrator);
    if let Some(orch) = orchestrator {
        if &orch.id != quark_id {
            return format!("@{} ⚠️ Quark `{}` turn errored: {err:#}", crate::router::ORCHESTRATOR_ALIAS, quark_id.as_str());
        }
    }
    format!("⚠️ Quark `{}` turn errored: {err:#}", quark_id.as_str())
}
```

In `run_until_quiesce` turn error branch (`Ok((target, _, assignment, Err(err)))`):

```rust
let err_msg = self.format_error_message(&target, &err);
let _ = self
    .append(Event::new(Actor::Gluon, None, Kind::Message { body: err_msg }))
    .await;
```

In `run_until_quiesce` turn panic branch (`Err(join_err)`):

```rust
let panic_err = anyhow::anyhow!("a quark turn panicked: {join_err}");
let orchestrator = self.roster.iter().find(|c| c.flavor == Flavor::Orchestrator);
let panic_msg = match orchestrator {
    Some(orch) => format!("@{} ⚠️ A quark turn panicked: {join_err}", crate::router::ORCHESTRATOR_ALIAS),
    None => format!("⚠️ A quark turn panicked: {join_err}"),
};
let _ = self
    .append(Event::new(Actor::Gluon, None, Kind::Message { body: panic_msg }))
    .await;
```

- [ ] **Step 3: Add unit tests in `src/engine/tests.rs`**

Add test `failing_quark_turn_sends_error_message_to_orchestrator`:

```rust
#[tokio::test]
async fn failing_quark_turn_sends_error_message_to_orchestrator() {
    // Verify that when a worker quark turn errors out, an error message mentioning @orchestrator is appended
}
```

- [ ] **Step 4: Run tests to verify**

Run: `cargo test -p hadron-gluon`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src/engine/run.rs crates/hadron-gluon/src/engine/tests.rs
git commit -m "feat(engine): send turn error messages to orchestrator"
```

---

### Task 2: Field Event Emission for Daemon Excite Errors

**Files:**

- Modify: `crates/hadron-gluon/src/bin/hadron-gluon.rs:400-406`

**Interfaces:**

- Consumes: `engine.run_until_quiesce()`.
- Produces: Field log event when `run_until_quiesce` returns `Err(e)`.

- [ ] **Step 1: Update error handling in `src/bin/hadron-gluon.rs`**

In `run_loop`:

```rust
if let Err(e) = engine.run_until_quiesce().await {
    eprintln!("gluon: excite error (continuing): {e:#}");
    let orch_exists = engine.projection().roster.iter().any(|c| c.flavor == Flavor::Orchestrator);
    let body = if orch_exists {
        format!("@{} ⚠️ Gluon excite error: {e:#}", hadron_gluon::router::ORCHESTRATOR_ALIAS)
    } else {
        format!("⚠️ Gluon excite error: {e:#}")
    };
    let _ = engine.append(Event::new(Actor::Gluon, None, Kind::Message { body })).await;
}
```

- [ ] **Step 2: Run tests to verify**

Run: `cargo test -p hadron-gluon`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-gluon/src/bin/hadron-gluon.rs
git commit -m "feat(daemon): log excite errors to field log and notify orchestrator"
```

---

### Task 3: Full Workspace Verification

**Files:**

- Test: Full workspace gate

- [ ] **Step 1: Run full workspace test gate**

Run: `cargo test --workspace --features gui`
Expected: PASS

- [ ] **Step 2: Verify git status**

Run: `git status`
Expected: Clean working tree.
