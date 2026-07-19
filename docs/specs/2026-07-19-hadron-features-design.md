# Design Specification: Hadron Features & Bugfixes (2026-07-19)

This document specifies the design for the unified implementation of six key Hadron features and bugfixes.

## 1. Chat Focus Hover-Selection Bugfix

### Problem
Clicking an inactive Hadron Chamber window to focus it fires a `MouseDownEvent` with `first_mouse: true`. This click triggers the selection drag state (`is_selecting = true`) but does not fire a corresponding `MouseUpEvent` because the window is focusing/activating. As a result, the cursor enters a "sticky" selection mode where hovering over selectable text highlight-marks it without any click-and-drag.

### Design
Modify the bubble-phase mouse listener inside `crates/gpui-component/crates/ui/src/text/window_selection.rs` to ignore event activation clicks:
```rust
window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
    if event.button != MouseButton::Left {
        return;
    }
    if event.first_mouse {
        return; // Ignore clicks whose sole purpose is window activation
    }
    if phase.capture() {
        ...
```

---

## 2. No-Human-Mode Adjudication Loop

### Problem & Invariants
Under No-Human-Mode (`no_human = true`), if a worker quark requests a permission (e.g., executing a command or merging a branch), the engine's `decide()` logic yields `Decision::AskOrchestrator` instead of `Decision::AskHuman`. 
The engine parks the worker quark in the `Waiting` status and appends a `[no-human-mode]`-marked message addressed to the orchestrator quark.
However, because the orchestrator can only reply via the standard chat text stream, there is currently no mechanism to translate its text reply into a formal `PermissionGrant` event to resume the worker.

### Design
1. **Natural Language Text Parsing (Gluon Engine)**:
   - When a worker quark is `Waiting` for a permission, the Gluon daemon watches for messages from the orchestrator quark addressed to that worker.
   - The daemon parses the text body for approval patterns (case-insensitive):
     - **Approval**: `@worker approved` or `@worker allowed` or `@worker allowed (always)` -> Translates to `Kind::PermissionGrant { approved: true, remember: false }` (or `remember: true` if always is matched).
     - **Denial**: `@worker denied` or `@worker rejected` or `@worker blocked` -> Translates to `Kind::PermissionGrant { approved: false, remember: false }`.
   - Upon matching, the Gluon engine automatically appends the `PermissionGrant` event to `field.jsonl` on the orchestrator's behalf, which excites the worker and resumes execution.

2. **Chamber Slash Commands**:
   - Register `/approve @worker` and `/deny @worker` in `crates/hadron-chamber/src/app/actions.rs` and `app.rs`.
   - Autocomplete candidates will be populated via `text::completion_candidates`.
   - When a human types the command, it immediately appends a `Kind::PermissionGrant` event to `field.jsonl` to resume the target worker.

---

## 3. Worktree Isolation & Merge Gate Activation

### Problem
Quark turns currently run in parallel in one shared checkout workspace (`repo_root` is `None`), which prevents clean attribution of code changes and creates conflicts. Worktree isolation and the merge gate are fully built but disabled.
Turning on `.with_git(repo_root)` without also wiring the merge gate will strand every completed assignment on a `quark/` branch that is never merged.

### Design
1. **Wiring**:
   - In the production daemon entry point (`crates/hadron-gluon/src/bin/hadron-gluon.rs`), resolve `repo_root` using the SSOT helper `hadron_lattice::workspace::repo_root_of(&args.field_path)`.
   - Initialize the `Engine` with BOTH git support and the `CargoMergeRunner`:
     ```rust
     let repo_root = hadron_lattice::workspace::repo_root_of(&args.field_path).to_path_buf();
     let mut engine = Engine::new(args.field_path.clone(), quarks, max_exchanges)
         .with_git(repo_root)
         .with_merge_gate(std::sync::Arc::new(hadron_gluon::merge::CargoMergeRunner))
         ...
     ```
2. **Definition of Done (DoD)**:
   - When a quark completes its task, the merge gate is triggered.
   - It runs `cargo test --workspace` in the isolated worktree directory using the shared target directory cache (`CARGO_TARGET_DIR`) to keep compile times under 15 seconds.
   - On success, it executes a local `--ff-only` merge into the parent check-out branch.

---

## 4. Live Mid-Turn Stream UI

### Problem
The daemon streams volatile agent thought chunks and tool calls to `<field-dir>/live/<quark-id>.json` using `hadron_lattice::live::publish`, but the Chamber UI does not yet read or display these files.

### Design
1. **Roster Row Subtitles**:
   - In `crates/hadron-chamber/src/app/render/roster.rs`, read active quark state files from the live directory:
     ```rust
     let live_dir = hadron_lattice::live::live_dir(&self.path);
     let activity = hadron_lattice::live::read(&live_dir, &r.id, chrono::Utc::now());
     ```
   - If an active/fresh `Activity` is retrieved:
     - Check `activity.doing` (e.g. `Doing::Thinking`, `Doing::Working`).
     - Display a live status string (e.g. `Thinking: Analyzing engine.rs` or `Working: cargo test`) in place of the static details label under the quark's name.
     - Color the text using theme tokens to denote activity state (e.g., blue for working, purple for thinking).

---

## 5. Budget Ceilings

### Problem
The energy ledger tracks total tokens spent, but it is currently disabled. Different AI seat models use different token units, and raw tokens do not scale cleanly to monetary cost.

### Design
1. **Roster Settings**:
   - Add an optional `max_cost_usd` or `max_tokens` field per seat in `team.json`.
2. **Engine Guard**:
   - Wire the Sqlite ledger at `.hadron/ledger.db` into the engine during Gluon startup:
     ```rust
     let ledger_path = args.field_path.parent().unwrap_or(std::path::Path::new(".")).join("ledger.db");
     let ledger = hadron_gluon::ledger::Ledger::open(&ledger_path)?;
     let engine = engine.with_ledger(ledger, global_limit);
     ```
   - Enforce depletion checks using roster-defined custom limit bounds, blocking worker excitation and posting warnings when limits are exceeded.

---

## 6. Foldable Plan Tab

### Problem
The `Plan` tab currently flattens all plan checklists into a single scrolling pane of text, which is hard to scan for long, multi-step plans.

### Design
1. **UI Accordions**:
   - Group checklist steps under their respective `## Task` or `### Task` headers parsed via `parse_plan_progress`.
   - Track folded task names in the UI state (e.g. `folded_tasks: HashSet<String>`).
   - Render each task header with a collapsible chevron toggle:
     - Clicking the header toggles the folder state.
   - **Auto-Expansion**:
     - Automatically expand the first task that contains incomplete checklist steps (the active task) and collapse completed or future tasks.
