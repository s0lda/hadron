# Next-Gen Seven Capabilities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use Swarm Quark Dispatch or subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the suite of seven requested core capabilities across Hadron: Speculative Merge Pre-Testing, Sliding Context Pruner, AST-Aware Rebase Healer, Tiled PTY Terminal Panes, Nucleus Integrity Linter, Headless Swarm Runner, and Repo Monitor.

**Architecture:** Distributed across `hadron-gluon` (speculative testing, sliding context pruner, AST rebase healer wiring), `hadron-forge` (AST merge resolution), `hadron-gatekeeper` (nucleus linter, repo monitor diagnostics), and `hadron-chamber` (tiled PTY splits, headless CLI runner commands, repo monitor UI settings & status indicators).

**Tech Stack:** Rust (2021 edition), Tokio, GPUI, Git CLI / tree-sitter.

---

### Task 1: Speculative Merge Pre-Testing (`hadron-gluon/src/engine/shadow_gate.rs`)

**Files:**
- Modify: `crates/hadron-gluon/src/engine/shadow_gate.rs`
- Modify: `crates/hadron-gluon/src/engine/merge.rs`
- Test: `crates/hadron-gluon/src/engine/shadow_gate.rs`

**Interfaces:**
- Consumes: Worktree, head commit SHA, base branch.
- Produces: `SpeculativeTestResult`, asynchronous pre-test execution, test cache lookup during merge gate.

- [x] **Step 1: Write failing test for speculative pre-testing cache**
- [x] **Step 2: Implement speculative test execution & result caching in `shadow_gate.rs`**
- [x] **Step 3: Wire speculative pre-test cache check into `merge_gate_body` in `merge.rs`**
- [x] **Step 4: Verify tests pass**
- [x] **Step 5: Commit Task 1**

---

### Task 2: Sliding Context Pruner (`hadron-gluon/src/sliding_pruner.rs`)

**Files:**
- Create: `crates/hadron-gluon/src/sliding_pruner.rs`
- Modify: `crates/hadron-gluon/src/lib.rs`
- Modify: `crates/hadron-gluon/src/prompt_distiller.rs`
- Modify: `crates/hadron-gluon/src/adapter/cli.rs`
- Test: `crates/hadron-gluon/src/sliding_pruner.rs`

**Interfaces:**
- Consumes: Field events window, token budget, compaction threshold.
- Produces: Summarized sliding window preserving prompt cache prefix and recent turns verbatim.

- [x] **Step 1: Write failing test for sliding context pruner**
- [x] **Step 2: Implement `SlidingContextPruner` with rolling turn summarization**
- [x] **Step 3: Wire into prompt generation / CLI truncation**
- [x] **Step 4: Verify tests pass**
- [x] **Step 5: Commit Task 2 (`f687a55a`)**

---

### Task 3: AST-Aware Rebase Healer (`hadron-gluon/src/merge/ast_healer.rs`)

**Files:**
- Create: `crates/hadron-gluon/src/merge/ast_healer.rs`
- Modify: `crates/hadron-gluon/src/merge.rs`
- Test: `crates/hadron-gluon/src/merge/ast_healer.rs`

**Interfaces:**
- Consumes: Conflicted worktree path, base branch, conflicting files.
- Produces: `heal_rebase_conflicts` calling `hadron_forge::ast_merge::merge_rust_ast` to resolve non-overlapping AST changes.

- [x] **Step 1: Write failing test for AST rebase healing**
- [x] **Step 2: Implement `heal_rebase_conflicts` using `hadron_forge::ast_merge::merge_rust_ast`**
- [x] **Step 3: Wire healer into `sync` on `Synced::Conflicted`**
- [x] **Step 4: Verify tests pass**
- [x] **Step 5: Commit Task 3 (`1122ceb5`)**

---

### Task 4: Tiled PTY Terminal Panes (`hadron-chamber/src/app/render/pty_grid.rs`)

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/pty_grid.rs`
- Modify: `crates/hadron-chamber/src/app/render/terminal.rs`
- Modify: `crates/hadron-chamber/src/app/mod.rs`
- Test: `crates/hadron-chamber/src/app/render/pty_grid.rs`

**Interfaces:**
- Consumes: `PtySplitMode` (Single, Horizontal, Vertical, Grid).
- Produces: Dynamic tiled PTY split layout in GPUI with cycling toggle and aligned split panes.

- [x] **Step 1: Define `PtySplitMode` enum and add mode cycle tests**
- [x] **Step 2: Update `multi_pty_grid` rendering for horizontal split, vertical split, and grid**
- [x] **Step 3: Wire split mode toggle into terminal toolbar in `terminal.rs`**
- [x] **Step 4: Verify tests pass**
- [x] **Step 5: Commit Task 4**

---

### Task 5: Nucleus Integrity Linter (`hadron-gatekeeper/src/nucleus_linter.rs`)

**Files:**
- Create: `crates/hadron-gatekeeper/src/nucleus_linter.rs`
- Modify: `crates/hadron-gatekeeper/src/lib.rs`
- Test: `crates/hadron-gatekeeper/src/nucleus_linter.rs`

**Interfaces:**
- Consumes: Nucleus directory path, index byte budget.
- Produces: `NucleusHealthReport` checking orphaned notes, broken markdown links, dead symbol refs, and byte budget overruns.

- [ ] **Step 1: Write failing test for nucleus integrity linter**
- [ ] **Step 2: Implement `NucleusIntegrityLinter` and report generation**
- [ ] **Step 3: Verify tests pass**
- [ ] **Step 4: Commit Task 5**

---

### Task 6: Headless Swarm Runner (`hadron run` / `hadron ci`)

**Files:**
- Modify: `crates/hadron-chamber/src/main.rs`
- Create: `crates/hadron-chamber/src/headless_runner.rs`
- Test: `crates/hadron-chamber/src/headless_runner.rs`

**Interfaces:**
- Consumes: CLI subcommand (`run <prompt>` or `ci --plan <path>`).
- Produces: Headless execution appending turn event, driving engine, and returning exit code 0 or non-zero.

- [ ] **Step 1: Write failing test for headless runner CLI argument parsing**
- [ ] **Step 2: Implement `run_headless_batch` in `headless_runner.rs`**
- [ ] **Step 3: Wire into `main.rs` entrypoint**
- [ ] **Step 4: Verify tests pass**
- [ ] **Step 5: Commit Task 6**

---

### Task 7: Repo Monitor (`hadron-gatekeeper/src/repo_monitor.rs` & Chamber UI)

**Files:**
- Create: `crates/hadron-gatekeeper/src/repo_monitor.rs`
- Modify: `crates/hadron-gatekeeper/src/lib.rs`
- Modify: `crates/hadron-chamber/src/config.rs`
- Modify: `crates/hadron-chamber/src/app/settings/providers.rs`
- Modify: `crates/hadron-chamber/src/app/render/status_bar.rs`
- Test: `crates/hadron-gatekeeper/src/repo_monitor.rs`

**Interfaces:**
- Consumes: Repo root, `ChamberPrefs::repo_monitor`, interval.
- Produces: `RepoHealthReport` (tests, stale trees, lock drift, nucleus issues), UI settings toggle, status indicator.

- [ ] **Step 1: Write failing test for `RepoMonitor` diagnostics**
- [ ] **Step 2: Implement `RepoMonitor::check_repo` combining baseline, worktree, and lock checks**
- [ ] **Step 3: Add `repo_monitor` & `repo_monitor_interval_secs` to `ChamberPrefs` and Settings → Execution**
- [ ] **Step 4: Render non-intrusive status indicator in Chamber**
- [ ] **Step 5: Verify tests pass**
- [ ] **Step 6: Commit Task 7**
