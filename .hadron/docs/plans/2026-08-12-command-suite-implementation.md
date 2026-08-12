# Command Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement 8 operational chat command families (`retry`, `doctor`, `prune`, `compact-nucleus`, `stop`/`kill`/`cancel`, `gate-cancel`, `revert`, `unabandon`) in Hadron's GUI and daemon command surface.

**Architecture:** Register commands in `COMMANDS` in [`crates/hadron-chamber/src/text.rs`](file:///home/Jake/dev/hadron/crates/hadron-chamber/src/text.rs) as SSOT, add command handling arms in `handle_chat_command` in [`crates/hadron-chamber/src/app/actions.rs`](file:///home/Jake/dev/hadron/crates/hadron-chamber/src/app/actions.rs), wire state/process control handlers, and update `HANDLED` in [`crates/hadron-chamber/src/app/input.rs`](file:///home/Jake/dev/hadron/crates/hadron-chamber/src/app/input.rs).

**Tech Stack:** Rust, GPUI, Tokio, Git VCS.

## Global Constraints

- Standard Model Rules 0-11 strictly enforced.
- Single Source of Truth (SSOT): All commands registered in `COMMANDS` slice in `crates/hadron-chamber/src/text.rs:90`.
- All listed commands MUST be handled in `handle_chat_command` and listed in `HANDLED` array in `crates/hadron-chamber/src/app/input.rs:620`.
- Branch deletion in `/prune` MUST write `archive/<slug>` tags first and use `git branch -d` (Rule 5 & VCS Invariants).
- Daemon/Gate isolation: `/gate-cancel` and `/kill` target process groups via `kill(-pgid, SIGKILL)`.
- No Preamble or Essays in turn reports.

---

### Task 1: Command Table Registrations & SSOT Guard Update (commit 35cf2dfd)

**Files:**
- Modify: `crates/hadron-chamber/src/text.rs:145-146`
- Modify: `crates/hadron-chamber/src/app/actions.rs:500-510`
- Modify: `crates/hadron-chamber/src/app/input.rs:620-627`
- Test: `crates/hadron-chamber/src/app/input.rs:627-642`

**Interfaces:**
- Consumes: Existing `Command`, `Arity`, `ArgSource` definitions in `crates/hadron-chamber/src/text.rs`.
- Produces: 10 new registered commands in `COMMANDS` table (`retry`, `doctor`, `prune`, `compact-nucleus`, `stop`, `kill`, `cancel`, `gate-cancel`, `revert`, `unabandon`).

- [x] **Step 1: Write the failing test** (commit 35cf2dfd)
- [x] **Step 2: Run test to verify it fails** (commit 35cf2dfd)
- [x] **Step 3: Write minimal implementation** (commit 35cf2dfd)
- [x] **Step 4: Run test to verify it passes** (commit 35cf2dfd)
- [x] **Step 5: Commit** (commit 35cf2dfd)

In `crates/hadron-chamber/src/app/input.rs`, add the 10 new command names to `HANDLED` array inside test `every_listed_command_is_handled`:
```rust
"retry",
"doctor",
"prune",
"compact-nucleus",
"stop",
"kill",
"cancel",
"gate-cancel",
"revert",
"unabandon",
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --lib app::input::tests::every_listed_command_is_handled`
Expected: FAIL with "handle_chat_command has an arm for /retry, which is not in COMMANDS"

- [ ] **Step 3: Write minimal implementation**

1. Register all 10 entries in `COMMANDS` in `crates/hadron-chamber/src/text.rs`:
```rust
Command { name: "retry", detail: "Re-dispatch the last failed message or turn for a seat or global", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
Command { name: "doctor", detail: "Run automated system diagnostics on daemon, locks, nucleus, fonts, and git worktrees", arity: Arity::None, arg: ArgSource::None, listed: true },
Command { name: "prune", detail: "Preview or clean up merged/stale quark worktrees and branches safely", arity: Arity::Line, arg: ArgSource::None, listed: true },
Command { name: "compact-nucleus", detail: "Audit and compact nucleus index against target budget limit", arity: Arity::Line, arg: ArgSource::None, listed: true },
Command { name: "stop", detail: "Gracefully stop a quark's in-flight turn", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
Command { name: "kill", detail: "Immediately force-kill a quark's subprocess group", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
Command { name: "cancel", detail: "Cancel pending unhandled dispatch for a seat", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
Command { name: "gate-cancel", detail: "Force cancel a hung merge-gate run by killing its process group", arity: Arity::None, arg: ArgSource::None, listed: true },
Command { name: "revert", detail: "Revert the last landed commit on main via git revert", arity: Arity::Line, arg: ArgSource::None, listed: true },
Command { name: "unabandon", detail: "Restore an archived branch from its archive tag", arity: Arity::Line, arg: ArgSource::None, listed: true },
```
2. Add placeholder arms in `handle_chat_command` in `crates/hadron-chamber/src/app/actions.rs`:
```rust
"retry" | "doctor" | "prune" | "compact-nucleus" | "stop" | "kill" | "cancel" | "gate-cancel" | "revert" | "unabandon" => {
    true
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib app::input::tests::every_listed_command_is_handled`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/text.rs crates/hadron-chamber/src/app/actions.rs crates/hadron-chamber/src/app/input.rs
git commit -m "feat(chamber): register 10 new operational commands in COMMANDS table"
```

---

### Task 2: Implement `/retry` Command (commit 745495f1)

**Files:**
- Modify: `crates/hadron-chamber/src/app/actions.rs:500`
- Test: `crates/hadron-chamber/src/app/actions.rs`

**Interfaces:**
- Consumes: `self.view.messages` in `Chamber`.
- Produces: `post_chat_message(Actor::Human, body, cx)` re-dispatching previous failed prompt.

- [x] **Step 1: Write the failing unit test** (commit 745495f1)
- [x] **Step 2: Run test to verify it fails** (commit 745495f1)
- [x] **Step 3: Write minimal implementation** (commit 745495f1)
- [x] **Step 4: Run test to verify it passes** (commit 745495f1)
- [x] **Step 5: Commit** (commit 745495f1)

```bash
git add crates/hadron-chamber/src/app/actions.rs
git commit -m "feat(chamber): implement /retry chat command"
```

---

### Task 3: Implement `/doctor` Diagnostic Suite

**Files:**
- Modify: `crates/hadron-chamber/src/text.rs:160` (add `doctor_body` formatter)
- Modify: `crates/hadron-chamber/src/app/actions.rs` (`"doctor"` arm)
- Test: `crates/hadron-chamber/src/text.rs`

**Interfaces:**
- Consumes: System stats (`gluon.lock`, nucleus size, font resolution, ACP paths).
- Produces: `doctor_body(...) -> String` formatted report.

- [ ] **Step 1: Write the failing unit test**

In `crates/hadron-chamber/src/text.rs` tests module, add `test_doctor_body_formatting`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --lib text::tests::test_doctor_body_formatting`
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

1. Implement `doctor_body` in `crates/hadron-chamber/src/text.rs`.
2. Update `"doctor"` arm in `crates/hadron-chamber/src/app/actions.rs`:
```rust
"doctor" => {
    let body = crate::text::doctor_body(&self.path, &self.view.roster);
    self.post_chat_message(Actor::Gluon, body, cx);
    true
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib text::tests::test_doctor_body_formatting`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/text.rs crates/hadron-chamber/src/app/actions.rs
git commit -m "feat(chamber): implement /doctor diagnostic command"
```

---

### Task 4: Implement `/prune` Worktree and Branch Cleanup

**Files:**
- Modify: `crates/hadron-chamber/src/vcs.rs` (add `prune_merged_worktrees_and_branches`)
- Modify: `crates/hadron-chamber/src/app/actions.rs` (`"prune"` arm)
- Test: `crates/hadron-chamber/src/vcs.rs`

**Interfaces:**
- Consumes: `git branch` and `git worktree` info.
- Produces: Safe deletion with `archive/<slug>` tag and `git branch -d`.

- [ ] **Step 1: Write the failing unit test**

In `crates/hadron-chamber/src/vcs.rs` tests module, add `test_prune_merged_branches_creates_archive_tags`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --lib vcs::tests::test_prune_merged_branches_creates_archive_tags`
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

1. Implement `prune_merged_worktrees_and_branches` in `crates/hadron-chamber/src/vcs.rs`.
2. Connect `"prune"` arm in `crates/hadron-chamber/src/app/actions.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib vcs::tests::test_prune_merged_branches_creates_archive_tags`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/vcs.rs crates/hadron-chamber/src/app/actions.rs
git commit -m "feat(chamber): implement /prune worktree and branch cleanup command"
```

---

### Task 5: Implement `/compact-nucleus`

**Files:**
- Modify: `crates/hadron-chamber/src/text.rs` (add `compact_nucleus_index`)
- Modify: `crates/hadron-chamber/src/app/actions.rs` (`"compact-nucleus"` arm)
- Test: `crates/hadron-chamber/src/text.rs`

**Interfaces:**
- Consumes: Path to `.hadron/nucleus/index.md` and target size string (e.g. `24kb`).
- Produces: Compacted `index.md` file and audit report.

- [ ] **Step 1: Write the failing unit test**

In `crates/hadron-chamber/src/text.rs` tests module, add `test_compact_nucleus_index_removes_invalid_pointers`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --lib text::tests::test_compact_nucleus_index_removes_invalid_pointers`
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

1. Implement `compact_nucleus_index` helper in `crates/hadron-chamber/src/text.rs`.
2. Wire `"compact-nucleus"` arm in `crates/hadron-chamber/src/app/actions.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib text::tests::test_compact_nucleus_index_removes_invalid_pointers`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/text.rs crates/hadron-chamber/src/app/actions.rs
git commit -m "feat(chamber): implement /compact-nucleus command"
```

---

### Task 6: Implement `/stop`, `/kill`, `/cancel` Process Controls

**Files:**
- Modify: `crates/hadron-chamber/src/app/actions.rs` (`"stop"`, `"kill"`, `"cancel"` arms)
- Test: `crates/hadron-chamber/src/app/actions.rs`

**Interfaces:**
- Consumes: Target `@quark` handle.
- Produces: Graceful stop signal, `SIGKILL` process group signal, or event retraction.

- [ ] **Step 1: Write failing test**

In `crates/hadron-chamber/src/app/actions.rs` tests, add test for process control commands.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --lib app::actions::tests::test_stop_kill_cancel_commands`
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

Wire `"stop"`, `"kill"`, and `"cancel"` arms in `handle_chat_command`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib app::actions::tests::test_stop_kill_cancel_commands`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/app/actions.rs
git commit -m "feat(chamber): implement /stop, /kill, and /cancel process control commands"
```

---

### Task 7: Implement `/gate-cancel`

**Files:**
- Modify: `crates/hadron-chamber/src/app/actions.rs` (`"gate-cancel"` arm)
- Test: `crates/hadron-chamber/src/app/actions.rs`

**Interfaces:**
- Consumes: Merge gate process group state.
- Produces: `kill(-pgid, SIGKILL)` on hung gate runner process group.

- [ ] **Step 1: Write failing test**

Add unit test `test_gate_cancel_kills_gate_process_group` in `crates/hadron-chamber/src/app/actions.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --lib app::actions::tests::test_gate_cancel_kills_gate_process_group`
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

Implement `"gate-cancel"` arm in `crates/hadron-chamber/src/app/actions.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib app::actions::tests::test_gate_cancel_kills_gate_process_group`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/app/actions.rs
git commit -m "feat(chamber): implement /gate-cancel command"
```

---

### Task 8: Implement `/revert` & `/unabandon`

**Files:**
- Modify: `crates/hadron-chamber/src/vcs.rs` (add `revert_last_landed_commit` and `restore_archived_branch`)
- Modify: `crates/hadron-chamber/src/app/actions.rs` (`"revert"`, `"unabandon"` arms)
- Test: `crates/hadron-chamber/src/vcs.rs`

**Interfaces:**
- Consumes: Gate history and `git tag -l "archive/*"`.
- Produces: Revert commit passed to gate, or restored branch SHA from archive tag.

- [ ] **Step 1: Write failing test**

Add tests `test_revert_last_landed_commit` and `test_unabandon_restores_archive_tag` in `crates/hadron-chamber/src/vcs.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --lib vcs::tests::test_revert_and_unabandon`
Expected: FAIL

- [ ] **Step 3: Write minimal implementation**

Implement `revert_last_landed_commit` and `restore_archived_branch` in `crates/hadron-chamber/src/vcs.rs`, and wire `"revert"` and `"unabandon"` arms in `crates/hadron-chamber/src/app/actions.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib vcs::tests::test_revert_and_unabandon`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/vcs.rs crates/hadron-chamber/src/app/actions.rs
git commit -m "feat(chamber): implement /revert and /unabandon VCS recovery commands"
```

---

## Plan Verification Check

Run full workspace tests after all tasks complete:
`cargo test -p hadron-chamber --all-targets`
`cargo test -p hadron-gluon --lib`
