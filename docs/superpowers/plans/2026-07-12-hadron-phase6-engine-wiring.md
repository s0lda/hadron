# Hadron Phase 6 slice 3a — Gatekeeper Engine Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [x]`) syntax.

**Goal:** Wire the gatekeeper into the coordination loop: a quark self-declares a needed risky op, the engine records a `PermissionReq`, and the god-mode `Policy` decides — auto-grant (god-mode) or pause-for-human. A grant addressed to the quark resumes it on the next tick.

**Architecture:** A quark surfaces intent via a new `TurnOutcome.permission` field (the *structured* self-declaration path, not prose-parsing). The engine holds a `Policy` and consults `hadron_gatekeeper::decide`. **Grants are ordinary events addressed to the quark**, so re-selection reuses `router::next_pending` — the only genuinely new engine behavior is emitting the req/grant and marking the quark `Waiting` when it pauses.

**Tech Stack:** Rust 2021. `hadron-lattice` (TurnOutcome + PermissionAsk), `hadron-gluon` (engine; adds a `hadron-gatekeeper` dep), `hadron-gatekeeper` (decide/Policy).

## Global Constraints

- **Grants re-trigger via addressing.** `PermissionGrant` is appended with `to = Some(quark)`; `next_pending` then re-selects that quark. No new routing code.
- **Task context survives the grant.** The trigger-finder must skip non-task events (the grant) or the resumed quark gets an empty task. See Task 2 Step 4 — this is the load-bearing fix, verified by a task-preservation assertion.
- **God-mode = the gluon grants instead of the human.** On `AutoApprove` the gluon auto-appends the grant; on `AskHuman` it appends nothing and quiesces. Same downstream resume path either way.
- **Safe default.** `Engine` policy defaults to `Policy::locked_down()` — every risky op asks the human unless god-mode is on.
- **Zero API spend.** Tests drive a local recording quark. Adapters set `permission: None` for now (making real quarks *emit* asks is prompt-engineering, deferred).
- **Vocabulary:** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger, block, hash, forge, watch, gatekeeper.

---

### Task 1: `PermissionAsk` + `TurnOutcome.permission` (lattice)

**Files:**
- Modify: `crates/hadron-lattice/src/projection.rs`

**Interfaces:**
- Produces: `struct PermissionAsk { risk: Risk, description: String }`; `TurnOutcome` gains `permission: Option<PermissionAsk>` (serde-default, so old lines/JSON still deserialize).

- [x] **Step 1: Add `PermissionAsk`** (near `TurnOutcome`; import `Risk`). At the top of `projection.rs` ensure `use crate::Risk;` (or `crate::event::Risk`) is in scope:

```rust
/// A quark's self-declared request to perform a risky operation, surfaced on its
/// `TurnOutcome`. The engine turns this into a `Kind::PermissionReq` and consults
/// the god-mode policy. Mirror of gatekeeper's `PendingPermission` (lattice can't
/// depend on gatekeeper, so the shape is duplicated deliberately).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PermissionAsk {
    pub risk: Risk,
    pub description: String,
}
```

- [x] **Step 2: Add the field to `TurnOutcome`:**

```rust
    #[serde(default)]
    pub permission: Option<PermissionAsk>,
```

- [x] **Step 3: Fix the `TurnOutcome::default()` test** at the bottom of `projection.rs`:

```rust
        assert_eq!(
            TurnOutcome::default(),
            TurnOutcome { message: None, used_tokens: 0, permission: None }
        );
```

- [x] **Step 4: Export** — confirm `PermissionAsk` is re-exported (projection.rs is under `pub use projection::*` in lib.rs, so it is automatic).

- [x] **Step 5: Build lattice** — `cargo build -p hadron-lattice`. Expected: clean (the field is additive; `Default` derives fine since `Option` defaults to `None`).

- [x] **Step 6: Run lattice tests** — `cargo test -p hadron-lattice`. Expected: PASS (incl. the fixed default test).

- [x] **Step 7: Commit**

```bash
git add crates/hadron-lattice/src/projection.rs
git commit -m "feat(lattice): TurnOutcome.permission — a quark's structured permission ask"
```

---

### Task 2: Engine consults the gatekeeper

**Files:**
- Modify: `crates/hadron-gluon/Cargo.toml` (add `hadron-gatekeeper` dep)
- Modify: `crates/hadron-gluon/src/engine.rs` (policy field + builder; trigger-finder fix; outcome hook; the other `TurnOutcome{}` sites in this file)
- Modify: `crates/hadron-gluon/src/mock.rs`, `crates/hadron-gluon/src/adapter/claude.rs`, `crates/hadron-gluon/src/adapter/runner.rs`, `crates/hadron-gluon/src/bin/hadron-gluon.rs` (add `permission: None` to their `TurnOutcome {}` literals)

**Interfaces:**
- Consumes: `hadron_gatekeeper::{decide, Decision, Policy}`, `hadron_lattice::{PermissionAsk, Risk, Kind::PermissionReq, Kind::PermissionGrant, QuarkState::Waiting}`.
- Produces: `Engine::with_policy(policy: Policy) -> Self`; permission-aware `run_until_quiesce`.

- [x] **Step 1: Add the dependency** to `crates/hadron-gluon/Cargo.toml` `[dependencies]`:

```toml
hadron-gatekeeper = { path = "../hadron-gatekeeper" }
```

- [x] **Step 2: Fix every non-permission `TurnOutcome {}` literal** so they compile with the new field. In `mock.rs:55`, `adapter/runner.rs` (2 sites), `adapter/claude.rs`, `bin/hadron-gluon.rs`, and the three test literals in `engine.rs` (~360, ~458, ~528): add `permission: None,` to each (or `..Default::default()`). Example (mock.rs):

```rust
        Ok(TurnOutcome { message, used_tokens: 0, permission: None })
```

- [x] **Step 3: Add the `policy` field + builder.** In the `Engine` struct add `policy: hadron_gatekeeper::Policy,`; in `new(...)` initialize `policy: hadron_gatekeeper::Policy::locked_down(),`; add the builder near the others:

```rust
    /// Opt in to god-mode: pre-authorize classes of risky op. Default is
    /// `Policy::locked_down()` (every risky op asks the human).
    pub fn with_policy(mut self, policy: hadron_gatekeeper::Policy) -> Self {
        self.policy = policy;
        self
    }
```

- [x] **Step 4: THE FIX — make the trigger-finder skip non-task events.** In `run_until_quiesce`, change the finder so a `PermissionGrant` addressed to the quark doesn't shadow the real task:

```rust
        if let Some(trigger) = events.iter().rev().find(|e| {
            e.to.as_ref() == Some(&target)
                && matches!(e.kind, Kind::Assign { .. } | Kind::Message { .. })
        }) {
```

(Only the `find` predicate changes; the `match` body is unchanged. Without this, a resumed quark receives an empty task because the grant is the most-recent event addressed to it.)

- [x] **Step 5: Add the outcome permission hook.** Replace the unconditional trailing `append Ground` block. After the existing `used_tokens` and `outcome.message` handling, insert:

```rust
            if let Some(ask) = outcome.permission {
                let risk = ask.risk;
                append_event(
                    &self.field_path,
                    &Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::PermissionReq { risk, description: ask.description },
                    ),
                )?;
                match hadron_gatekeeper::decide(risk, self.policy) {
                    hadron_gatekeeper::Decision::AutoApprove => {
                        // God-mode: the gluon grants on the human's behalf, addressed
                        // to the quark so next_pending re-selects it next tick.
                        append_event(
                            &self.field_path,
                            &Event::new(
                                Actor::Gluon,
                                Some(target.clone()),
                                Kind::PermissionGrant { approved: true },
                            ),
                        )?;
                        exchanges += 1;
                        continue;
                    }
                    hadron_gatekeeper::Decision::AskHuman => {
                        // Pause: mark the quark waiting and quiesce until a human
                        // PermissionGrant (addressed to the quark) resumes it.
                        append_event(
                            &self.field_path,
                            &Event::new(
                                Actor::Quark(target.clone()),
                                None,
                                Kind::Status { state: QuarkState::Waiting },
                            ),
                        )?;
                        return Ok(());
                    }
                }
            }

            append_event(
                &self.field_path,
                &Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::Status { state: QuarkState::Ground },
                ),
            )?;

            exchanges += 1;
```

(The existing `append Ground` + `exchanges += 1` become the fall-through for the no-permission case shown above; delete the old duplicate.)

- [x] **Step 6: Write the tests.** Add to `engine.rs`'s `#[cfg(test)] mod tests` a local recording quark and three tests. Use existing helpers (`seed_human_message`, `tempdir`).

```rust
    use std::sync::{Arc, Mutex};
    use hadron_lattice::PermissionAsk;
    use hadron_gatekeeper::{Policy, Risk};

    /// Asks for permission on excite #1, replies on later excites, and records the
    /// `task` it was given each excite (so a test can prove task context survives
    /// a resume).
    struct PermissionQuark {
        id: QuarkId,
        flavor: Flavor,
        ask: PermissionAsk,
        reply: String,
        calls: usize,
        tasks: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl crate::quark::Quark for PermissionQuark {
        fn id(&self) -> QuarkId { self.id.clone() }
        fn flavor(&self) -> Flavor { self.flavor.clone() }
        fn energy(&self) -> hadron_lattice::EnergyState { hadron_lattice::EnergyState::Available }
        async fn excite(&mut self, turn: hadron_lattice::Projection) -> anyhow::Result<hadron_lattice::TurnOutcome> {
            self.tasks.lock().unwrap().push(turn.task.clone());
            self.calls += 1;
            if self.calls == 1 {
                Ok(hadron_lattice::TurnOutcome { message: None, used_tokens: 0, permission: Some(self.ask.clone()) })
            } else {
                Ok(hadron_lattice::TurnOutcome { message: Some(self.reply.clone()), used_tokens: 0, permission: None })
            }
        }
    }

    fn perm_quark(id: &str, tasks: Arc<Mutex<Vec<String>>>) -> PermissionQuark {
        PermissionQuark {
            id: QuarkId::new(id),
            flavor: Flavor::Orchestrator,
            ask: PermissionAsk { risk: Risk::BashExec, description: "cargo publish".into() },
            reply: "published".into(),
            calls: 0,
            tasks,
        }
    }

    fn has_kind(events: &[Event], pred: impl Fn(&Kind) -> bool) -> bool {
        events.iter().any(|e| pred(&e.kind))
    }

    #[tokio::test]
    async fn locked_down_policy_pauses_for_human() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        seed_human_message(&field, "agy", "hello");
        let tasks = Arc::new(Mutex::new(vec![]));
        let mut engine = Engine::new(field.clone(), vec![Box::new(perm_quark("agy", tasks.clone()))], String::new(), 8);
        engine.run_until_quiesce().await.unwrap();

        let events = crate::field::read_events(&field).unwrap();
        assert!(has_kind(&events, |k| matches!(k, Kind::PermissionReq { .. })), "req recorded");
        assert!(!has_kind(&events, |k| matches!(k, Kind::PermissionGrant { .. })), "no auto-grant when locked down");
        assert!(has_kind(&events, |k| matches!(k, Kind::Status { state: QuarkState::Waiting })), "quark waits");
        assert!(!has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published")), "op not performed yet");
        // Chamber would surface this outstanding request.
        assert!(hadron_gatekeeper::pending_permission(&events).is_some());
    }

    #[tokio::test]
    async fn human_grant_resumes_the_quark_with_its_task() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        seed_human_message(&field, "agy", "hello");
        let tasks = Arc::new(Mutex::new(vec![]));
        let mut engine = Engine::new(field.clone(), vec![Box::new(perm_quark("agy", tasks.clone()))], String::new(), 8);
        engine.run_until_quiesce().await.unwrap();

        // Human approves, addressed to the quark.
        append_event(&field, &Event::new(Actor::Human, Some(QuarkId::new("agy")), Kind::PermissionGrant { approved: true })).unwrap();
        engine.run_until_quiesce().await.unwrap();

        let events = crate::field::read_events(&field).unwrap();
        assert!(has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published")), "op performed after grant");
        // THE FIX: the resumed excite got the original task, not the grant's empty context.
        let recorded = tasks.lock().unwrap().clone();
        assert_eq!(recorded.len(), 2, "asked once, resumed once");
        assert_eq!(recorded[1], "hello", "resumed quark kept its task");
    }

    #[tokio::test]
    async fn god_mode_auto_approves_and_completes() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        seed_human_message(&field, "agy", "hello");
        let tasks = Arc::new(Mutex::new(vec![]));
        let policy = Policy { auto_approve_edits: false, bypass_bash: true };
        let mut engine = Engine::new(field.clone(), vec![Box::new(perm_quark("agy", tasks.clone()))], String::new(), 8)
            .with_policy(policy);
        engine.run_until_quiesce().await.unwrap();

        let events = crate::field::read_events(&field).unwrap();
        assert!(has_kind(&events, |k| matches!(k, Kind::PermissionReq { .. })), "req still recorded (audit trail)");
        assert!(events.iter().any(|e| e.from == Actor::Gluon && matches!(e.kind, Kind::PermissionGrant { approved: true })), "gluon auto-granted");
        assert!(has_kind(&events, |k| matches!(k, Kind::Message { body } if body == "published")), "op completed without a human");
        let recorded = tasks.lock().unwrap().clone();
        assert_eq!(recorded[1], "hello", "auto-resumed quark kept its task");
    }
```

- [x] **Step 7: Run the tests** — `cargo test -p hadron-gluon`. Expected: all prior + 3 new PASS. (If `human_grant_resumes...` fails on `recorded[1] == "hello"`, the trigger-finder fix in Step 4 is missing.)

- [x] **Step 8: Full workspace + clippy** — `cargo test` and `cargo clippy -p hadron-gluon`. Expected: green, no new warnings.

- [x] **Step 9: Commit**

```bash
git add crates/hadron-gluon
git commit -m "feat(gluon): gatekeeper wiring — permission asks pause or god-mode auto-approve"
```

---

## Definition of Done

- A quark's `TurnOutcome.permission` becomes a `Kind::PermissionReq` on the field.
- `locked_down` policy → quark marked `Waiting`, engine quiesces, `pending_permission` surfaces the request; a human `PermissionGrant` (to the quark) resumes it **with its original task intact**.
- God-mode (`bypass_bash`) → gluon auto-grants and the op completes with no human, request still recorded for audit.
- Full workspace green; three tests incl. the task-preservation assertion that proves the trigger-finder fix.

## Notes / limits (say plainly; deferred)

- **Sequential, so pausing is global.** An `AskHuman` quiesce pauses the whole loop, not just the asking quark (same limitation as the Phase-4 sequential swarm loop). Per-agent pause needs concurrent excitation — future.
- **`PermissionAsk` (lattice) and `PendingPermission` (gatekeeper) are duplicate `{risk, description}` shapes** — unavoidable (lattice can't depend on gatekeeper), not wired to each other.
- **Real quarks don't ask yet.** Adapters set `permission: None`; making `claude.rs`/`agy.rs` actually emit asks from model output is prompt/parse work — a follow-up.
- **Chamber is slice 3b (manual verify).** The Approve/Deny toast (appends `PermissionGrant`) and god-mode toggle switches (write `Policy`) need a display; not built headlessly here.
