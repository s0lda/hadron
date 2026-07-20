# Roster Exclusivity, File Tree Enhancements, and Single Instance Guards

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Orchestrator exclusivity (exactly one orchestrator per roster), enhance the file tree to display gitignored sub-entries on expansion, color files based on their Git status, and add single-instance locking guards for both gluon and chamber.

**Architecture:**
- **Orchestrator Exclusivity:** Modify `ContextMenuAction::SetFlavor` inside `crates/hadron-chamber/src/app/actions.rs` to demote all other roster entries (both `quarks` and `roster` overrides) to `Worker` if the targeted seat is set to `Orchestrator`. Add tests in `crates/hadron-chamber/src/app/actions.rs` to verify.
- **File Tree Enhancements:**
  - Update `list_workspace_files` in `crates/hadron-chamber/src/sys.rs` to accept `expanded_dirs` and dynamically scan inside gitignored folders if they are in the expanded set.
  - Implement `get_git_statuses` in `crates/hadron-chamber/src/vcs.rs` to run `git status --porcelain` and map modified/added files to `GitStatus` enums.
  - Query and cache `git_statuses` on `Chamber` in `reload.rs` on the 400ms tick.
  - Color the file tree items accordingly in `render_node` inside `crates/hadron-chamber/src/app/render/terminal.rs`.
- **Single-instance guards:**
  - Add `libc` dependency to `hadron-gluon` and `hadron-chamber` to use `flock`.
  - In `hadron-gluon`, acquire an exclusive non-blocking `flock` on `.hadron/gluon.lock` and exit with code 1 if held.
  - In `hadron-chamber`, try to acquire a non-blocking `flock` on `.hadron/chamber.lock` at start, warning if held, and probe `.hadron/gluon.lock` to report whether a gluon is currently running.

**Tech Stack:** Rust, GPUI, Git CLI, libc crate (Unix flock)

## Global Constraints
- Do not import any unvetted dependencies. Only add libc which is already present in Cargo.lock.
- Preserve all existing comments and docstrings.
- Ensure all tests pass.

---

### Task 1: Add libc Dependency
**Files:**
- Modify: `crates/hadron-gluon/Cargo.toml`
- Modify: `crates/hadron-chamber/Cargo.toml`

- [x] **Step 1: Add libc dependency to hadron-gluon**
  Edit [crates/hadron-gluon/Cargo.toml](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-gluon/Cargo.toml) to add `libc = "0.2"` under dependencies.

- [x] **Step 2: Add libc dependency to hadron-chamber**
  Edit [crates/hadron-chamber/Cargo.toml](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/Cargo.toml) to add `libc = "0.2"` under dependencies.

- [x] **Step 3: Run cargo check to verify dependencies compile**
  Run: `cargo check`
  Expected: Successful compilation of dependencies.

---

### Task 2: Orchestrator Exclusivity in SetFlavor
**Files:**
- Modify: `crates/hadron-chamber/src/app/actions.rs`

- [x] **Step 1: Implement demote-others logic in actions.rs SetFlavor**
  Update the `ContextMenuAction::SetFlavor` block to demote all other quarks and overrides to Worker when setting to Orchestrator.

- [x] **Step 2: Add a unit test verifying orchestrator exclusivity**
  Add a test to the `tests` module in [crates/hadron-chamber/src/app/actions.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/app/actions.rs).

- [x] **Step 3: Verify tests pass**
  Run: `cargo test -p hadron-chamber`
  Expected: PASS

---

### Task 3: Git Status Helper
**Files:**
- Modify: `crates/hadron-chamber/src/vcs.rs`

- [x] **Step 1: Declare GitStatus enum and get_git_statuses function**
  Add the enum and status parser to [crates/hadron-chamber/src/vcs.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/vcs.rs).

- [x] **Step 2: Run cargo check**
  Run: `cargo check -p hadron-chamber`
  Expected: PASS

---

### Task 4: Enhance File Tree listing and caching
**Files:**
- Modify: `crates/hadron-chamber/src/sys.rs`
- Modify: `crates/hadron-chamber/src/app/mod.rs`
- Modify: `crates/hadron-chamber/src/app/reload.rs`
- Modify: `crates/hadron-chamber/src/app/render/terminal.rs`

- [x] **Step 1: Update list_workspace_files signature and implementation in sys.rs**
  Modify [crates/hadron-chamber/src/sys.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/sys.rs) to accept `expanded_dirs` and recursively list inside expanded gitignored dirs. Update tests in `sys.rs` to pass empty HashSet.

- [x] **Step 2: Store git_statuses on Chamber struct**
  Update `Chamber` in [crates/hadron-chamber/src/app/mod.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/app/mod.rs) to have `git_statuses: std::collections::HashMap<String, crate::vcs::GitStatus>`. Initialize it in `Chamber::new`.

- [x] **Step 3: Update reload.rs to poll git_statuses and files**
  Update [crates/hadron-chamber/src/app/reload.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/app/reload.rs) to update `self.git_statuses` and call `list_workspace_files` with `self.file_tree_expanded`.

- [x] **Step 4: Update folder click handler to immediately trigger list rescan**
  In [crates/hadron-chamber/src/app/render/terminal.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/app/render/terminal.rs) (the folder `on_click` listener), immediately invoke `list_workspace_files` and update `this.file_tree_paths` and `this.git_statuses` before repainting.

- [x] **Step 5: Color the File Tree files in render_node**
  Pass `git_statuses` into `render_node` inside [crates/hadron-chamber/src/app/render/terminal.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/app/render/terminal.rs) and set the `.text_color` based on `GitStatus`.

- [x] **Step 6: Run cargo test to verify file tree tests still pass**
  Run: `cargo test -p hadron-chamber`
  Expected: PASS

---

### Task 5: Single-Instance Guards for Gluon and Chamber
**Files:**
- Modify: `crates/hadron-gluon/src/bin/hadron-gluon.rs`
- Modify: `crates/hadron-chamber/src/app/mod.rs`

- [x] **Step 1: Exclusive flock guard in hadron-gluon**
  Update `main` in [crates/hadron-gluon/src/bin/hadron-gluon.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-gluon/src/bin/hadron-gluon.rs) to acquire `LOCK_EX` on `.hadron/gluon.lock` and exit if held.

- [x] **Step 2: Non-blocking warning locks/diagnostics in hadron-chamber**
  Update `run` in [crates/hadron-chamber/src/app/mod.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/app/mod.rs) to warning-lock `.hadron/chamber.lock` and report if a gluon is currently running.

- [x] **Step 3: Verify the entire workspace compiles and tests pass**
  Run: `cargo test --workspace --features gui`
  Expected: PASS
