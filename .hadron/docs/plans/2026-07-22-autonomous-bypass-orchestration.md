# Autonomous Bypass Orchestration & Active Plan State Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable autonomous end-to-end task execution in Bypass Mode by injecting mode-driven Orchestrator directives into prompt generation and enforcing active plan state updates (`- [x]`) on disk.

**Architecture:** Prompt generation in `crates/hadron-gluon/src/adapter/prompt/mod.rs` reads `projection.mode` and role (`is_orchestrator`). In `Mode::Bypass`, it appends strict autonomous directives: auto-selecting execution paths, updating plan task checkboxes on disk upon verification, auto-dispatching the next unchecked task, and returning to the human only upon 100% plan completion.

**Tech Stack:** Rust, `hadron-gluon`, `hadron-lattice`, unit tests.

## Global Constraints
- **SSOT**: Plan markdown checkboxes (`- [ ]` / `- [x]`) in `.hadron/docs/plans/*.md` are the single source of truth for plan progress.
- **No breaking changes to routing**: Rely on prompt invariants and role directives; do not introduce complex file-parsing dependencies into `router/mod.rs`.
- **Test gates**: All changes must pass `cargo test -p hadron-gluon` and `cargo test --workspace`.

---

### Task 1: Extend Prompt Adapter with Mode-Driven Orchestrator Directives

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/prompt/mod.rs:309-354`
- Test: `crates/hadron-gluon/src/adapter/prompt/tests.rs`

**Interfaces:**
- Consumes: `projection.mode: hadron_lattice::Mode`, `is_orchestrator(projection, self_id): bool`
- Produces: Mode-specific prompt text appended to Orchestrator system prompt in `build()`.

- [ ] **Step 1: Write failing unit test in `tests.rs`**

Add unit test `bypass_orchestrator_gets_autonomous_loop_directives` in `crates/hadron-gluon/src/adapter/prompt/tests.rs`:

```rust
#[test]
fn bypass_orchestrator_gets_autonomous_loop_directives() {
    let mut proj = test_projection();
    proj.mode = hadron_lattice::Mode::Bypass;
    let orch_id = QuarkId("orchestrator".to_string());
    let prompt = build(&proj, &orch_id);

    assert!(prompt.contains("Autonomous Bypass Execution"));
    assert!(prompt.contains("update the active plan file on disk"));
    assert!(prompt.contains("dispatch the next incomplete task"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-gluon --test prompt_tests bypass_orchestrator_gets_autonomous_loop_directives`
Expected: FAIL with missing assertion pattern "Autonomous Bypass Execution"

- [ ] **Step 3: Implement prompt mode guidance in `mod.rs`**

Update `crates/hadron-gluon/src/adapter/prompt/mod.rs` in `is_orchestrator` block:

```rust
    if is_orchestrator(projection, self_id) {
        if projection.mode == Mode::Bypass {
            p.push_str(
                "**Autonomous Bypass Execution Loop:** You are in Bypass Mode. Drive the overall task to 100% completion autonomously:\n\
                 1. **Plan State Update**: When a task is verified complete, update the active plan file on disk (`.hadron/docs/plans/*.md`) changing `- [ ]` to `- [x] Task N (commit <hash>)` and commit the edit.\n\
                 2. **Continuous Dispatch**: Immediately dispatch the next unchecked task (`- [ ]`) without pausing or asking the human for options.\n\
                 3. **Completion Gate**: Hand control back to the human (reply without `@mention`) ONLY when 100% of tasks in the plan are marked `- [x]` or on unrecoverable blockages.\n\n",
            );
        }
        p.push_str(
            "You are the **orchestrator**: you are the human's conversational partner, worker Quarks in the \
             swarm and your sub-agents report their progress and errors to you, and you carry their work to the human...\n\n",
        );
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-gluon --test prompt_tests bypass_orchestrator_gets_autonomous_loop_directives`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -f crates/hadron-gluon/src/adapter/prompt/mod.rs crates/hadron-gluon/src/adapter/prompt/tests.rs
git commit -m "feat(prompt): add autonomous bypass directives and plan update rules for orchestrator"
```

---

### Task 2: Active Plan File Update Guidance for Workers and Workspace Gate

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/prompt/mod.rs`
- Test: `crates/hadron-gluon/src/adapter/prompt/tests.rs`

- [ ] **Step 1: Write unit test for worker active plan awareness**

```rust
#[test]
fn worker_is_instructed_on_plan_state_reporting() {
    let proj = test_projection();
    let worker_id = QuarkId("worker-1".to_string());
    let prompt = build(&proj, &worker_id);

    assert!(prompt.contains("When your task is complete"));
    assert!(prompt.contains("@orchestrator"));
}
```

- [ ] **Step 2: Run workspace test suite**

Run: `cargo test --workspace`
Expected: PASS (0 failed)

- [ ] **Step 3: Commit plan and workspace verification**

```bash
git add -f .hadron/docs/plans/2026-07-22-autonomous-bypass-orchestration.md
git commit -m "docs(plans): add autonomous bypass orchestration implementation plan"
```
