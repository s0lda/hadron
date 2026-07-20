# Memory Feature Map & Invariants Registry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a dedicated Feature Map and Invariants Registry in `.hadron/memory/`, and update Rule 9 of the Standard Model rules to enforce their maintenance.

**Architecture:** Create `.hadron/memory/features.md` and `.hadron/memory/invariants.md` to store current architecture mappings and operational constraints, offloading them from `index.md`. Modify `crates/hadron-gluon/invariants/standard_model.md` to update Rule 9 definitions and verify compile/tests.

**Tech Stack:** Markdown, Rust (Rust compiler and test suite).

## Global Constraints
- Document files must be written under `/home/Jake/dev/hadron/.hadron/memory/`.
- Rust source edits must only touch `crates/hadron-gluon/invariants/standard_model.md`.
- No new packages, dependencies, or external imports.

---

### Task 1: Initialize Invariants Registry

**Files:**
- Create: `/home/Jake/dev/hadron/.hadron/memory/invariants.md`

**Interfaces:**
- Consumes: None
- Produces: Persistent registry file on disk containing operational constraints.

- [ ] **Step 1: Write invariants file to disk**

Write the following content to `/home/Jake/dev/hadron/.hadron/memory/invariants.md`:
```markdown
# Invariants Registry

Tracks non-negotiable operational and structural constraints for the Hadron project.

## GUI & Rendering Constraints
- **Vulkan / Lavapipe Software Fallback**: LAVAPIPE is the only Vulkan ICD on target WSL/native machines; GPUI rasterizes in CPU software. Frame lag is environment-based, not a code-level regression.
- **Scroll Viewport Cross-Axis Constraints**: Scrollable flex elements in cross-axis stretch layouts must have explicit max dimensions (e.g. `max_h`/`max_w`) on themselves to calculate scroll bounds correctly.
- **Scroll Deferral**: Synced scroll adjustments and focus jumps must be deferred to `window.on_next_frame` to allow input state bounds to update first.
- **Absolute Positioning Coordinate Anchoring**: 
  - Absolute completion overlays must omit `left_0` to avoid blocking mouse events on parent components.
  - Calling `relative()` after `absolute()` overrides the positioning type back to relative.
- **Char Boundary Safety**: Byte slices/offsets into labels/inputs must never fall mid-character. Use character-boundary validation helpers when drawing text.

## IPC & Swarm Protocol
- **NDJSON Message Framing**: Daemon communication relies on newline-delimited JSON (NDJSON) rather than LSP-like `Content-Length` headers. LSP framing will cause clients to hang indefinitely.
- **Resident ACP Spawning**: Working directories passed to ACP client execution must be absolute paths to prevent relative paths breaking subprocess execution.
- **Token Spend Tracking**: Telemetry spend is calculated by fresh spent tokens (`TokenSpend::fresh()`). Absent usage fields from ACP clients must be handled as absent, not zero.
```

- [ ] **Step 2: Verify file existence**

Run: `ls -la /home/Jake/dev/hadron/.hadron/memory/invariants.md`
Expected: File lists successfully with correct permissions and size.

---

### Task 2: Initialize Feature Map

**Files:**
- Create: `/home/Jake/dev/hadron/.hadron/memory/features.md`

**Interfaces:**
- Consumes: None
- Produces: Persistent feature map file on disk.

- [ ] **Step 1: Write features file to disk**

Write the following content to `/home/Jake/dev/hadron/.hadron/memory/features.md`:
```markdown
# Feature Map

Tracks primary high-level features of Hadron, their implementation status, entrypoints, and responsibilities.

## PTY Terminal
- **Status**: Live
- **Files**: `crates/hadron-chamber/src/pty.rs`, `crates/hadron-chamber/src/sys.rs`
- **Logic**: Stateful execution (e.g., `cd` persists directory changes). Headless-tested VTE grid in `pty.rs` mapped to Alacritty grid logic.

## Live Card / Activity Tracker
- **Status**: Live
- **Files**: `crates/hadron-lattice/src/live.rs`, `crates/hadron-chamber/src/app/widgets.rs`, `crates/hadron-chamber/src/app/render/chat.rs`
- **Logic**: Stacks all active adopted/enabled quarks using `widgets::active_quarks`. Chamber client triggers repaint via file-watcher and `cx.notify()`.

## Unified Chat Autocomplete
- **Status**: Live
- **Files**: `crates/hadron-chamber/src/text.rs`, `crates/hadron-chamber/src/app.rs`
- **Logic**: Completion menu is driven by `text::completion_candidates` and rendered as native `Chamber::completion_card_overlay`.

## Stats / Telemetry
- **Status**: Live
- **Files**: `crates/hadron-chamber/src/model/stats.rs`, `crates/hadron-chamber/src/app/render/stats.rs`
- **Logic**: Gated by `StatsWindow` mode (Current vs Session/Week/Month).

## Settings Fallback Defaults
- **Status**: Live
- **Files**: `crates/hadron-chamber/src/app/settings/mod.rs`, `crates/hadron-chamber/src/app/settings/providers.rs`
- **Logic**: Loads secret values from store and falls back to Catalogue definitions (`self.global.get`) for non-adopted quarks.
```

- [ ] **Step 2: Verify file existence**

Run: `ls -la /home/Jake/dev/hadron/.hadron/memory/features.md`
Expected: File lists successfully with correct permissions and size.

---

### Task 3: Update Standard Model Rules

**Files:**
- Modify: `crates/hadron-gluon/invariants/standard_model.md`
- Test: `crates/hadron-gluon/src/engine/tests.rs`

**Interfaces:**
- Consumes: None
- Produces: Updated prompt instructions compiled into hadron-gluon binary.

- [ ] **Step 1: Write standard model rule modification**

Modify `crates/hadron-gluon/invariants/standard_model.md` around lines 82-94 to replace the original Rule 9 with:
```markdown
## 9. Maintain the memory: Index, Features, and Invariants.

At the start of every turn, you are handed the memory **index** — the only thing carrying state between sessions. Keep the memory ecosystem clean:
1. **Lessons Index (`index.md`)**: A cheap ledger of mistakes and post-mortems. One short line per lesson: `- [<slug>](notes/<slug>.md) — <the lesson, in one sentence>`. Notes go in `notes/`.
2. **Feature Map (`features.md`)**: Track high-level features, their status, and their entrypoint files. Update this map whenever you add, modify, or deprecate functionality.
3. **Invariants Registry (`invariants.md`)**: Track operational constraints, rendering rules, environment quirks, and protocol boundaries. Update this registry when you discover a new codebase invariant.
```

- [ ] **Step 2: Run test suite to verify compilation and rules test**

Run: `cargo test -p hadron-gluon`
Expected: Compilation passes and standard model prompt assertions pass.

- [ ] **Step 3: Commit rules changes**

Run:
```bash
git add crates/hadron-gluon/invariants/standard_model.md
git commit -m "feat(memory): enforce feature map and invariants registry in standard model Rule 9"
```
