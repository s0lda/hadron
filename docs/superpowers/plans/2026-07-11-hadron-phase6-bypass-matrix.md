# Hadron Phase 6 — The Bypass Matrix (Gatekeeper core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the pure decision core of the Gatekeeper — the "bypass matrix" that maps a proposed operation's risk against the human's god-mode policy to either auto-approve or ask the human.

**Architecture:** A new pure crate `hadron-gatekeeper` (mirrors `hadron-forge`: offline, no runtime, its own green test binary). It exposes `Risk` (an *input* — the category of a proposed op), `Policy` (two independent god-mode toggles), `Decision` (approve vs ask), and `decide(risk, policy) -> Decision` (the truth table). **This slice deliberately does NOT classify commands, emit events, pause the daemon, or render UI** — those depend on an unresolved architectural fork (see "Deferred / open fork" below) and belong to later slices.

**Tech Stack:** Rust (edition 2021), no dependencies beyond the standard library for the core (serde only, to match the workspace's serializable-types convention, so `Policy`/`Risk`/`Decision` can later flow through events / persist as UI state).

## Global Constraints

- **Bet on nothing uncertain.** In Hadron's CLI-adapter architecture a quark's turn surfaces to the daemon as a `TurnOutcome { message, used_tokens }` — the vendor CLI runs bash/edits inside its own subprocess, so the daemon never sees individual commands. Therefore `classify(command)` has no caller and is NOT built here. `decide` takes `Risk` as a given input.
- **Two independent god-mode toggles, not a ladder.** The roadmap: auto-approve workspace edits (Level 1) *or* bypass all bash prompts (Level 2). Model these as two independent bools, so a human can bypass bash without auto-approving edits or vice-versa.
- **Pure, offline, zero API spend.** No network, no filesystem, no runtime. All logic is a total function over small enums — exhaustively testable.
- **Additive, low-collision with Gemini.** New crate = new files. The only shared-file touch is one additive line in the root `Cargo.toml` `members` list. Do not touch `engine.rs`, `event.rs`, `router.rs`, or any adapter (Gemini is actively editing them for Phase 5; `main`'s gluon test binary is currently red on a transient `router.rs` test-import — verify `git log main` at merge time).
- **Vocabulary (exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger, block, hash, forge, watch, gatekeeper. ("gatekeeper" and "bypass matrix" are the roadmap's own Phase 6 terms.)

---

### Task 1: The `hadron-gatekeeper` crate — the bypass matrix

**Files:**
- Create: `crates/hadron-gatekeeper/Cargo.toml`
- Create: `crates/hadron-gatekeeper/src/lib.rs`
- Create: `crates/hadron-gatekeeper/src/matrix.rs`
- Modify: root `Cargo.toml` (add `"crates/hadron-gatekeeper"` to `members`)

**Interfaces:**
- Produces:
  - `enum Risk { WorkspaceEdit, BashExec }` — the category of a proposed op (an *input*; who supplies it is a later slice's concern).
  - `struct Policy { auto_approve_edits: bool, bypass_bash: bool }` with `Policy::locked_down()` (both false) and `Policy::default()` (== locked_down).
  - `enum Decision { AutoApprove, AskHuman }`.
  - `fn decide(risk: Risk, policy: Policy) -> Decision` — the truth table: `WorkspaceEdit` is auto-approved iff `auto_approve_edits`; `BashExec` (incl. publish-class shell ops) is auto-approved iff `bypass_bash`; otherwise `AskHuman`.

- [ ] **Step 1: Create the crate skeleton.**

`crates/hadron-gatekeeper/Cargo.toml`:
```toml
[package]
name = "hadron-gatekeeper"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
```

`crates/hadron-gatekeeper/src/lib.rs`:
```rust
//! The Gatekeeper's pure decision core: the "bypass matrix" mapping a proposed
//! operation's risk against the human's god-mode policy.
//!
//! This crate is intentionally offline and side-effect-free. It does NOT classify
//! commands, emit events, pause the daemon, or render UI — those are later slices.

mod matrix;

pub use matrix::{decide, Decision, Policy, Risk};
```

- [ ] **Step 2: Add the crate to the workspace** — in root `Cargo.toml`, add `"crates/hadron-gatekeeper"` to the `members` array (keep it sorted/grouped with the other `crates/...` entries).

- [ ] **Step 3: Write the failing tests.** Create `crates/hadron-gatekeeper/src/matrix.rs` with the types + a `todo!()` body for `decide`, and this exhaustive test module:

```rust
use serde::{Deserialize, Serialize};

/// The category of a proposed operation. An *input* to the matrix — Hadron does
/// not derive this from command text in the CLI-adapter architecture (a quark's
/// turn surfaces only as a message, not structured tool calls). A later slice
/// decides who supplies the `Risk`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Writing, editing, or deleting files inside the workspace.
    WorkspaceEdit,
    /// Executing a shell command (includes publish-class ops like `cargo publish`).
    BashExec,
}

/// The human's god-mode configuration: two independent bypass toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Level 1: auto-approve workspace edits without asking.
    pub auto_approve_edits: bool,
    /// Level 2: bypass all bash-execution prompts.
    pub bypass_bash: bool,
}

impl Policy {
    /// The safe default: nothing is bypassed; every risky op asks the human.
    pub fn locked_down() -> Self {
        Policy { auto_approve_edits: false, bypass_bash: false }
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy::locked_down()
    }
}

/// The matrix's verdict for a single proposed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// The policy pre-authorizes this class of op; proceed without a prompt.
    AutoApprove,
    /// Pause and surface a permission request to the human.
    AskHuman,
}

/// The bypass matrix: does `policy` pre-authorize an op of this `risk`?
pub fn decide(risk: Risk, policy: Policy) -> Decision {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_down_asks_for_everything() {
        let p = Policy::locked_down();
        assert_eq!(decide(Risk::WorkspaceEdit, p), Decision::AskHuman);
        assert_eq!(decide(Risk::BashExec, p), Decision::AskHuman);
    }

    #[test]
    fn edit_toggle_only_bypasses_edits() {
        let p = Policy { auto_approve_edits: true, bypass_bash: false };
        assert_eq!(decide(Risk::WorkspaceEdit, p), Decision::AutoApprove);
        // Independent: bypassing edits must NOT bypass bash.
        assert_eq!(decide(Risk::BashExec, p), Decision::AskHuman);
    }

    #[test]
    fn bash_toggle_only_bypasses_bash() {
        let p = Policy { auto_approve_edits: false, bypass_bash: true };
        assert_eq!(decide(Risk::BashExec, p), Decision::AutoApprove);
        // Independent: bypassing bash must NOT bypass edits.
        assert_eq!(decide(Risk::WorkspaceEdit, p), Decision::AskHuman);
    }

    #[test]
    fn both_toggles_bypass_both() {
        let p = Policy { auto_approve_edits: true, bypass_bash: true };
        assert_eq!(decide(Risk::WorkspaceEdit, p), Decision::AutoApprove);
        assert_eq!(decide(Risk::BashExec, p), Decision::AutoApprove);
    }

    #[test]
    fn default_policy_is_locked_down() {
        assert_eq!(Policy::default(), Policy::locked_down());
    }
}
```

- [ ] **Step 4: Run tests to verify they fail** — `cargo test -p hadron-gatekeeper`.
Expected: FAIL — `decide` panics with `not yet implemented`.

- [ ] **Step 5: Implement `decide`.** Replace the `todo!()` body:

```rust
pub fn decide(risk: Risk, policy: Policy) -> Decision {
    let bypassed = match risk {
        Risk::WorkspaceEdit => policy.auto_approve_edits,
        Risk::BashExec => policy.bypass_bash,
    };
    if bypassed {
        Decision::AutoApprove
    } else {
        Decision::AskHuman
    }
}
```

- [ ] **Step 6: Run tests to verify they pass** — `cargo test -p hadron-gatekeeper`.
Expected: PASS (5 tests).

- [ ] **Step 7: Confirm the workspace still builds** — `cargo build` (library only; do NOT run the full `cargo test`, gluon's test binary is transiently red on Gemini's `router.rs`).
Expected: `hadron-gatekeeper` compiles and is a workspace member.

- [ ] **Step 8: Commit**

```bash
git add crates/hadron-gatekeeper Cargo.toml
git commit -m "feat(gatekeeper): the bypass matrix — decide(risk, policy) god-mode core"
```

---

## Definition of Done

- `hadron-gatekeeper` is a workspace member with a green test binary (5 tests) that is independent of gluon's currently-red one.
- `decide(risk, policy)` implements the two-independent-toggle bypass matrix exactly; the truth table is exhaustively tested (both risks × relevant policies).
- Nothing in this slice classifies commands, touches the field/event schema, the engine, or the chamber — so it bets on nothing about the unresolved trigger design and collides with none of Gemini's hot files.

## Deferred / open fork (next slices — resolve with the user before building)

- **THE fork — who triggers a permission request?** On the CLI-adapter path the daemon never sees individual commands (`TurnOutcome` carries only a message body). So a `permission_req` cannot be derived by Hadron classifying a command. The options, to settle with the user before slice 2:
  1. **Quark self-declares** — a quark emits an explicit `permission_req` (structured turn output or a parseable `@human PERMISSION: …` convention in its message), carrying its own `Risk` + human-readable description.
  2. **API-path only** — command classification exists only if/when Hadron drives models via the API (structured `tool_use`) instead of the vendor CLI; on the CLI path Phase 6 is quark-cooperative only.
- **Event schema** — `Kind::PermissionReq { risk, description }` + `Kind::PermissionGrant { approved }` in lattice (additive enum arms). Its exact fields depend on the fork above, so it is NOT frozen here.
- **The req↔grant pairing helper** — `pending_permission(events) -> Option<…>` (analogous to `router::next_pending`): the most recent `PermissionReq` with no `PermissionGrant` after it. Plugs into the swarm loop's `serve`/`next_pending` as another thing the daemon waits on.
- **Engine wiring** — on an `AskHuman` decision, emit `permission_req`, pause that quark, resume on grant. Touches the hot `engine.rs`; wait for Gemini's Phase 5 to settle.
- **Chamber toast + god-mode toggles** — render `PermissionReq` as an Approve/Deny toast; UI switches write `Policy`. GPUI, manual verification (needs a display).
