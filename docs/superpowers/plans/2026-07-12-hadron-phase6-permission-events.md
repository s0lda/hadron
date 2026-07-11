# Hadron Phase 6 slice 2 — Permission Events & Pairing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make a permission request a first-class citizen of the append-only field: add `Kind::PermissionReq`/`Kind::PermissionGrant` events, and a pure pairing helper that finds an outstanding (ungranted) request — so the swarm loop can wait on it exactly like it waits on a pending quark turn.

**Architecture:** `Risk` moves into `hadron-lattice` (it is event payload data, and lattice is the foundational schema crate that must not depend on gatekeeper). `hadron-gatekeeper` gains a `hadron-lattice` dependency, re-exports `Risk`, and hosts the `pending_permission` helper (kept out of the Gemini-hot `router.rs`). Nothing here pauses the daemon, populates the request, or renders UI — those are slice 3+.

**Tech Stack:** Rust 2021, `hadron-lattice` (custom serde for `Kind`), `hadron-gatekeeper` (adds lattice dep).

## Global Constraints

- **Permission lives on the bus.** A request/grant is an `Event`, not out-of-band state — so it composes with `serve`/`next_pending`. An ungranted `PermissionReq` is just another "pending" the daemon waits on.
- **`Risk` has one definition, in lattice.** No duplicate risk enums. gatekeeper re-exports `hadron_lattice::Risk` so `hadron_gatekeeper::Risk` and `decide(risk, …)` keep working.
- **Additive + unknown-tolerant.** New `Kind` arms extend the hand-written `Serialize`/`Deserialize` in `event.rs`; the `Unknown` fallback and forward-compat contract stay intact.
- **`main` is stable** (Gemini's Phase 5 merged; 93 tests green). Still additive-only to `event.rs`.
- **Vocabulary:** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger, block, hash, forge, watch, gatekeeper.

---

### Task 1: Permission events in lattice

**Files:**
- Modify: `crates/hadron-lattice/src/event.rs` (add `Risk`; add two `Kind` arms + their serialize/deserialize arms; tests)

**Interfaces:**
- Produces: `enum Risk { WorkspaceEdit, BashExec }` (snake_case serde). `Kind::PermissionReq { risk: Risk, description: String }` and `Kind::PermissionGrant { approved: bool }`.

- [ ] **Step 1: Add the `Risk` enum** right after `QuarkState` (same derive/attrs):

```rust
/// The category of a proposed operation, carried on a `PermissionReq`. Matched
/// against the human's god-mode policy by `hadron_gatekeeper::decide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Writing, editing, or deleting files inside the workspace.
    WorkspaceEdit,
    /// Executing a shell command (includes publish-class ops like `cargo publish`).
    BashExec,
}
```

- [ ] **Step 2: Add the two `Kind` arms** (after `Assign`, before `Unknown`):

```rust
    PermissionReq { risk: Risk, description: String },
    PermissionGrant { approved: bool },
```

- [ ] **Step 3: Add serialize arms** (in the `match &self.kind` block, after `Assign`):

```rust
            Kind::PermissionReq { risk, description } => {
                m.serialize_entry("kind", "permission_req")?;
                m.serialize_entry("risk", risk)?;
                m.serialize_entry("description", description)?;
            }
            Kind::PermissionGrant { approved } => {
                m.serialize_entry("kind", "permission_grant")?;
                m.serialize_entry("approved", approved)?;
            }
```

- [ ] **Step 4: Add deserialize arms** (in the `match kind_tag.as_str()`, after `"assign"`):

```rust
            "permission_req" => Kind::PermissionReq {
                risk: take_field(&mut map, "risk")?,
                description: take_field(&mut map, "description")?,
            },
            "permission_grant" => Kind::PermissionGrant {
                approved: take_field(&mut map, "approved")?,
            },
```

- [ ] **Step 5: Write round-trip tests** (append to `mod event_tests`):

```rust
    #[test]
    fn permission_req_round_trips() {
        let ev = Event::new(
            Actor::Quark(QuarkId::new("agy")),
            Some(QuarkId::new("human")),
            Kind::PermissionReq {
                risk: Risk::BashExec,
                description: "cargo publish".into(),
            },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"permission_req""#));
        assert!(line.contains(r#""risk":"bash_exec""#));
        assert!(line.contains(r#""description":"cargo publish""#));
    }

    #[test]
    fn permission_grant_round_trips() {
        let ev = Event::new(
            Actor::Human,
            None,
            Kind::PermissionGrant { approved: true },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"permission_grant""#));
        assert!(line.contains(r#""approved":true"#));
    }
```

- [ ] **Step 6: Run tests** — `cargo test -p hadron-lattice event`.
Expected: PASS incl. the 2 new; the existing `unknown_kind_is_preserved_not_crashed` still passes (forward-compat intact).

- [ ] **Step 7: Commit**

```bash
git add crates/hadron-lattice/src/event.rs
git commit -m "feat(lattice): permission_req/permission_grant events + Risk on the field"
```

---

### Task 2: The `pending_permission` pairing helper (gatekeeper)

**Files:**
- Modify: `crates/hadron-gatekeeper/Cargo.toml` (add lattice dep)
- Modify: `crates/hadron-gatekeeper/src/matrix.rs` (import `Risk` from lattice instead of defining it)
- Modify: `crates/hadron-gatekeeper/src/lib.rs` (re-export `Risk` from lattice; add `gate` module)
- Create: `crates/hadron-gatekeeper/src/gate.rs` (`PendingPermission`, `pending_permission`)

**Interfaces:**
- Consumes: `hadron_lattice::{Event, Kind, Risk}`.
- Produces: `struct PendingPermission { risk: Risk, description: String }`; `fn pending_permission(events: &[Event]) -> Option<PendingPermission>` — the most recent `PermissionReq` with no `PermissionGrant` after it (mirrors `router::next_pending`).

- [ ] **Step 1: Add the lattice dependency** to `crates/hadron-gatekeeper/Cargo.toml`:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
hadron-lattice = { path = "../hadron-lattice" }
```

- [ ] **Step 2: Move `Risk` out of gatekeeper.** In `matrix.rs`, delete the local `pub enum Risk { … }` and add at the top: `use hadron_lattice::Risk;`. (`decide` and the existing tests reference `Risk` unchanged via `use super::*`.)

- [ ] **Step 3: Update `lib.rs`** so the public API is unchanged and `gate` is wired:

```rust
mod gate;
mod matrix;

pub use gate::{pending_permission, PendingPermission};
pub use hadron_lattice::Risk;
pub use matrix::{decide, Decision, Policy};
```

- [ ] **Step 4: Write `gate.rs` with a failing-first stub + tests:**

```rust
use hadron_lattice::{Event, Kind, Risk};

/// An outstanding permission request awaiting a human grant/deny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPermission {
    pub risk: Risk,
    pub description: String,
}

/// The latest `PermissionReq` with no `PermissionGrant` after it, or `None` if
/// there is no request or the last one was already answered. Mirrors the
/// stateless reconstruct-from-the-field rule of `router::next_pending`.
pub fn pending_permission(events: &[Event]) -> Option<PendingPermission> {
    let idx = events
        .iter()
        .rposition(|e| matches!(e.kind, Kind::PermissionReq { .. }))?;
    let granted = events[idx + 1..]
        .iter()
        .any(|e| matches!(e.kind, Kind::PermissionGrant { .. }));
    if granted {
        return None;
    }
    match &events[idx].kind {
        Kind::PermissionReq { risk, description } => Some(PendingPermission {
            risk: *risk,
            description: description.clone(),
        }),
        _ => unreachable!("rposition matched PermissionReq"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Actor, QuarkId};

    fn req(desc: &str) -> Event {
        Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::PermissionReq { risk: Risk::BashExec, description: desc.into() },
        )
    }
    fn grant(approved: bool) -> Event {
        Event::new(Actor::Human, None, Kind::PermissionGrant { approved })
    }
    fn msg() -> Event {
        Event::new(Actor::Human, None, Kind::Message { body: "hi".into() })
    }

    #[test]
    fn none_when_no_request() {
        assert_eq!(pending_permission(&[msg()]), None);
    }

    #[test]
    fn returns_unanswered_request() {
        let got = pending_permission(&[req("cargo publish")]).unwrap();
        assert_eq!(got.risk, Risk::BashExec);
        assert_eq!(got.description, "cargo publish");
    }

    #[test]
    fn granted_request_is_not_pending() {
        assert_eq!(pending_permission(&[req("x"), grant(true)]), None);
    }

    #[test]
    fn denied_request_is_also_resolved() {
        // A deny (approved=false) still answers the request; not pending.
        assert_eq!(pending_permission(&[req("x"), grant(false)]), None);
    }

    #[test]
    fn newest_unanswered_request_wins() {
        let got = pending_permission(&[req("first"), grant(true), req("second")]).unwrap();
        assert_eq!(got.description, "second");
    }
}
```

- [ ] **Step 5: Run tests** — `cargo test -p hadron-gatekeeper`.
Expected: PASS — the 5 existing matrix tests (now using lattice's `Risk`) + 5 new gate tests.

- [ ] **Step 6: Confirm the workspace builds** — `cargo build`.
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/hadron-gatekeeper
git commit -m "feat(gatekeeper): pending_permission pairing helper over field events"
```

---

## Definition of Done

- `Kind::PermissionReq { risk, description }` and `Kind::PermissionGrant { approved }` round-trip through the field; forward-compat (`Unknown`) intact.
- `Risk` has a single home in lattice; `hadron_gatekeeper::{Risk, decide}` unchanged for callers.
- `pending_permission(events)` returns the outstanding request (or `None`), tested for: no request, unanswered, granted, denied, newest-wins.
- Full workspace green. No engine/adapter/chamber changes (slice 3+).

## Deferred (slice 3+, resolve trigger with the user)

- **Who populates `PermissionReq`** — recommended: a structured optional field on `TurnOutcome` a quark returns, mapped by the adapter into a `PermissionReq` event (NOT prose-parsing). Then the engine, on a `decide(...) == AskHuman`, emits it, pauses that quark, and resumes when `pending_permission` clears via a `PermissionGrant`.
- **Engine wiring** in `run_until_quiesce`/`serve` (hot `engine.rs`).
- **Chamber**: render `PermissionReq` as an Approve/Deny toast that appends `PermissionGrant`; god-mode toggle switches writing `Policy`. GPUI, manual verify.
