# Process Manager Overlay and Global Keybindings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Process Manager overlay to the Hadron Chamber GUI (accessed via a pinned Roster button) for managing daemon and ACP subprocesses, and fix global tab navigation keybindings so shortcuts dispatch regardless of input focus.

**Architecture:** Extend `Chamber` state with `process_manager_open: bool`, build a smoked-glass `process_overlay(cx)` component matching Settings, add a pinned "Processes" button to the roster rail, and unbind `KEY_CONTEXT` restrictions on tab shortcuts while adding direct `Alt-1`..`Alt-7` keybindings.

**Tech Stack:** Rust, GPUI (`crates/hadron-chamber`).

## Global Constraints

- Standard Model Invariant 3: Single Source of Truth (`.hadron/` directory only).
- Standard Model Invariant 5: Workspace & GUI test gate must pass (`cargo test --workspace --features gui`).
- All new struct fields and actions must compile under `#[cfg(feature = "gui")]`.

---

### Task 1: Global Keybindings & Direct Tab Navigation Fix

**Files:**
- Modify: `crates/hadron-chamber/src/app/mod.rs:85-101,738-760`
- Modify: `crates/hadron-chamber/src/app/actions.rs:180-200`
- Modify: `crates/hadron-chamber/src/app/render/mod.rs:74-97`
- Test: `crates/hadron-chamber/src/app/actions.rs`

**Interfaces:**
- Consumes: `ChatTab`, `RightRailTab`
- Produces: `SelectChatTab(usize)`, `SelectInspectorTab(usize)` actions and global key bindings

- [ ] **Step 1: Write failing tests for direct tab selection**

```rust
#[test]
fn test_select_chat_tab_and_inspector_tab() {
    let mut app = Chamber::dummy();
    app.set_chat_tab_index(2);
    assert_eq!(app.chat_tab, ChatTab::Stats);

    app.set_inspector_tab_index(1);
    assert_eq!(app.right_rail_tab, RightRailTab::FileTree);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber test_select_chat_tab_and_inspector_tab --features gui`
Expected: FAIL (methods not implemented yet)

- [ ] **Step 3: Define actions, helper methods, and bind global keybindings**

In `crates/hadron-chamber/src/app/mod.rs`:
Add actions:
- `ChatTab1`, `ChatTab2`, `ChatTab3`
- `InspectorTab1`, `InspectorTab2`, `InspectorTab3`, `InspectorTab4`

Register keybindings in `app/mod.rs`:
```rust
KeyBinding::new("ctrl-tab", NextChatTab, None),
KeyBinding::new("ctrl-shift-tab", PrevChatTab, None),
KeyBinding::new("ctrl-pagedown", NextInspectorTab, None),
KeyBinding::new("ctrl-pageup", PrevInspectorTab, None),
KeyBinding::new("alt-1", ChatTab1, None),
KeyBinding::new("alt-2", ChatTab2, None),
KeyBinding::new("alt-3", ChatTab3, None),
KeyBinding::new("alt-4", InspectorTab1, None),
KeyBinding::new("alt-5", InspectorTab2, None),
KeyBinding::new("alt-6", InspectorTab3, None),
KeyBinding::new("alt-7", InspectorTab4, None),
```

In `crates/hadron-chamber/src/app/actions.rs`:
Implement direct index tab handlers `set_chat_tab_index` and `set_inspector_tab_index`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber test_select_chat_tab_and_inspector_tab --features gui`
Expected: PASS

- [ ] **Step 5: Run full workspace test gate**

Run: `cargo test --workspace --features gui`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/hadron-chamber/src/
git commit -m "feat(chamber): fix global tab keybindings and add Alt-1..7 direct tab shortcuts"
```

---

### Task 2: Process Manager State & Roster Rail Button

**Files:**
- Modify: `crates/hadron-chamber/src/app/mod.rs:220-250`
- Modify: `crates/hadron-chamber/src/app/actions.rs`
- Modify: `crates/hadron-chamber/src/app/render/roster.rs:200-210`
- Test: `crates/hadron-chamber/src/app/actions.rs`

**Interfaces:**
- Consumes: `Chamber` state
- Produces: `ToggleProcessManager` action, `process_manager_open: bool` state, `processes_button(cx)` element

- [ ] **Step 1: Write failing test for process manager toggle**

```rust
#[test]
fn test_toggle_process_manager() {
    let mut app = Chamber::dummy();
    assert!(!app.process_manager_open);
    app.toggle_process_manager();
    assert!(app.process_manager_open);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber test_toggle_process_manager --features gui`
Expected: FAIL

- [ ] **Step 3: Implement toggle action and roster rail button**

Add `process_manager_open: bool` to `Chamber` in `app/mod.rs`.
In `app/actions.rs`, implement `toggle_process_manager`.
In `app/render/roster.rs`, add `processes_button` pinned directly above `settings_button`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber test_toggle_process_manager --features gui`
Expected: PASS

- [ ] **Step 5: Run full workspace test gate**

Run: `cargo test --workspace --features gui`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/hadron-chamber/src/
git commit -m "feat(chamber): add Process Manager state and pinned Roster button"
```

---

### Task 3: Process Manager Overlay Component

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/overlays.rs`
- Modify: `crates/hadron-chamber/src/app/render/mod.rs:65-73`
- Test: `crates/hadron-chamber/src/app/render/overlays.rs`

**Interfaces:**
- Consumes: `Chamber.process_manager_open`, `Chamber.view.roster`, live PIDs / status
- Produces: `process_overlay(cx)` modal card UI

- [ ] **Step 1: Write failing test for process overlay state rendering helper**

```rust
#[test]
fn test_process_list_resolution() {
    let app = Chamber::dummy();
    let processes = app.resolve_running_processes();
    assert!(!processes.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber test_process_list_resolution --features gui`
Expected: FAIL

- [ ] **Step 3: Implement `resolve_running_processes` and `process_overlay`**

Implement process resolution:
- Check `hadron-gluon` daemon status / PID.
- Inspect roster seats for ACP child process state and PIDs.

Build `process_overlay(cx)` in `overlays.rs` using smoked glass surface (`modal_surface`), displaying process cards with PID badges and action buttons (**Restart**, **Kill**).

Wire `process_overlay` into `app/render/mod.rs` when `process_manager_open` is true.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber test_process_list_resolution --features gui`
Expected: PASS

- [ ] **Step 5: Run full workspace test gate**

Run: `cargo test --workspace --features gui`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/hadron-chamber/src/
git commit -m "feat(chamber): add Process Manager overlay modal UI"
```
