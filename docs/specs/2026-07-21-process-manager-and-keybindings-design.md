# Design: Process Manager Overlay and Global Tab Keyboard Navigation Fix

## Context & Purpose
Users currently lack visibility into running Hadron background processes (such as the `hadron-gluon` daemon and resident ACP Quark subprocesses like `acp-claude` and `acp-agy`). Users need a way to inspect running PIDs, process status, and perform process lifecycle actions (Kill, Restart).

Additionally, keyboard navigation shortcuts for swapping tabs (`ctrl-tab` / `ctrl-shift-tab` for Chat column tabs and `ctrl-pagedown` / `ctrl-pageup` for Inspector tabs) currently fail to dispatch when text input fields or terminal views have active focus. Direct tab selection shortcuts (e.g. `alt-1`..`alt-7`) are also missing.

## Architectural & Design Changes

### 1. Process Manager Overlay & Roster Integration
- **Roster Rail Button**: In `crates/hadron-chamber/src/app/render/roster.rs`, add a pinned **Processes** button above the existing Settings button at the bottom of the Quarks roster rail.
- **Process Manager Overlay**: In `crates/hadron-chamber/src/app/render/overlays.rs` (or `app/process/`), create `process_overlay(cx)`:
  - **Gluon Daemon Row**: Displays `hadron-gluon` PID (if running), active status, and a **Restart Daemon** button.
  - **Resident ACP Seats Row**: For each ACP seat (`acp-claude`, `acp-agy`, etc.), displays child process PID, state (Active / Idle / Stopped), **Restart** action (invokes `reset_session`), and **Kill** action (sends `SIGTERM`/`SIGKILL` signal to child PID or drops child handle).
  - **CLI Quarks Row**: Displays current execution state (Idle / Taking turn).
- **App State**: Add `process_manager_open: bool` to `Chamber` state with action `ToggleProcessManager`.

### 2. Global Keyboard Navigation Fix
- **Action Context Unification**: In `crates/hadron-chamber/src/app/mod.rs` and `crates/hadron-chamber/src/app/render/mod.rs`:
  - Register key bindings for `NextChatTab`, `PrevChatTab`, `NextInspectorTab`, `PrevInspectorTab` with `None` context (global dispatch) instead of restricting to `Some("Chamber")` so focus inside sub-views does not block dispatch.
  - In `crates/hadron-chamber/src/app/render/chat.rs` and `crates/hadron-chamber/src/app/render/terminal.rs`, use `.capture_action` on container handles so focused text inputs and PTY terminals do not swallow tab cycling shortcuts.
- **Direct Tab Shortcuts**:
  - Add actions and keybindings for direct tab selection:
    - `Alt-1`, `Alt-2`, `Alt-3`: Switch directly to Chat tabs (`Chat`, `Log`, `Stats`).
    - `Alt-4`, `Alt-5`, `Alt-6`, `Alt-7`: Switch directly to Inspector tabs (`Terminal`, `Files`, `Changes`, `Plan`).

## Verification & Testing Strategy
1. **Unit Tests**:
   - Add unit tests verifying `ChatTab` and `RightRailTab` direct index navigation.
   - Add unit tests for process tracking data structure resolution.
2. **Workspace & GUI Gate**:
   - Run `cargo test --workspace --features gui` to ensure all tests pass cleanly.
