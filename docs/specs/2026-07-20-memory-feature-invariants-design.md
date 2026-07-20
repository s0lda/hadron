# Design Spec: Memory Feature Map & Invariants Registry

## Context & Rationale
Currently, the shared memory file `index.md` acts as a dual ledger tracking both historical mistakes/bug fixes and ongoing architectural rules. Over time, this leads to prompt bloat and makes it hard for quarks to quickly distinguish between historical warnings and active architectural boundaries.

To resolve this, we will decompose the shared codebase memory into three distinct documents under `.hadron/memory/`:
1. **Lessons Index (`index.md`)**: A cheap ledger of post-mortems and mistakes.
2. **Feature Map (`features.md`)**: Maps high-level features to entrypoints and files.
3. **Invariants Registry (`invariants.md`)**: Tracks operational rules and boundaries.

To enforce this system, we will modify **Rule 9** in the compiled Standard Model rules.

---

## 1. Document Schema & Initialization

### A. Invariants Registry (`.hadron/memory/invariants.md`)
Tracks non-negotiable codebase constraints. It is organized into three categories:

#### GUI & Rendering Constraints
- **Vulkan / Lavapipe Software Fallback**: LAVAPIPE is the only Vulkan ICD on WSL/target machines; GPUI rasterizes in CPU software. Frame lag is environment-based, not a code-level regression.
- **Scroll Viewport Cross-Axis Constraints**: Scrollable flex elements in cross-axis stretch layouts must have explicit max dimensions (e.g. `max_h`/`max_w`) on themselves to calculate scroll bounds correctly.
- **Scroll Deferral**: Synced scroll adjustments and focus jumps must be deferred to `window.on_next_frame` to allow input state bounds to update first.
- **Absolute Positioning Coordinate Anchoring**: 
  - Absolute completion overlays must omit `left_0` to avoid blocking mouse events on parent components.
  - Calling `relative()` after `absolute()` overrides the positioning type back to relative.
- **Char Boundary Crash Prevention**: Ranges or byte slices into strings must never cut mid-character. Use character-boundary validation helpers when drawing text labels.

#### IPC & Swarm Protocol
- **NDJSON Message Framing**: Daemon communication relies on newline-delimited JSON (NDJSON) rather than LSP-like `Content-Length` headers. LSP framing will cause clients to hang indefinitely.
- **Resident ACP Spawning**: Working directories passed to ACP client execution must be absolute paths to prevent relative paths breaking subprocess execution.
- **Token Spend Tracking**: Telemetry spend is calculated by fresh spent tokens (`TokenSpend::fresh()`). Absent usage fields from ACP clients must be handled as absent, not zero.

---

### B. Feature Map (`.hadron/memory/features.md`)
Catalogs primary features, their status, entrypoints, and related notes.

#### PTY Terminal
- **Status**: Live
- **Files**: `pty.rs`, `sys.rs`
- **Callers**: `Chamber::render` in `app.rs` maps raw output escapes to the VTE grid.
- **Constraints**: stateful execution (e.g., `cd` must persist directory changes).

#### Live Card / Activity Tracker
- **Status**: Live
- **Files**: `live/mod.rs`, `widgets.rs`, `chat.rs`
- **Logic**: Stacks all active adopted/enabled quarks using `widgets::active_quarks`. Client triggers redraws using file-watcher and `cx.notify()`.

#### Unified Chat Autocomplete
- **Status**: Live
- **Files**: `text.rs`, `app.rs`
- **Logic**: Built as native `Chamber::completion_card_overlay` triggered by `text::completion_candidates`.

#### Stats / Telemetry
- **Status**: Live
- **Files**: `model/stats.rs`, `app/render/stats.rs`
- **Logic**: Gated by `StatsWindow` mode (Current vs Session/Week/Month).

#### Settings
- **Status**: Live
- **Files**: `app/settings/mod.rs`, `app/settings/providers.rs`
- **Logic**: Secret variable loading and catalogue fallback lookups (`self.global.get`).

---

## 2. Standard Model Rule 9 Update
We will update `crates/hadron-gluon/invariants/standard_model.md` to re-define Rule 9:

```markdown
## 9. Maintain the memory: Index, Features, and Invariants.

At the start of every turn, you are handed the memory **index** — the only thing carrying state between sessions. Keep the memory ecosystem clean:
1. **Lessons Index (`index.md`)**: A cheap ledger of mistakes and post-mortems. One short line per lesson: `- [<slug>](notes/<slug>.md) — <the lesson, in one sentence>`. Notes go in `notes/`.
2. **Feature Map (`features.md`)**: Track high-level features, their status, and their entrypoint files. Update this map whenever you add, modify, or deprecate functionality.
3. **Invariants Registry (`invariants.md`)**: Track operational constraints, rendering rules, environment quirks, and protocol boundaries. Update this registry when you discover a new codebase invariant.
```

---

## Verification Plan
1. **Compile Verification**: Verify that hadron compile and tests pass after modifying `standard_model.md` (which is embedded via macro/include).
2. **Standard Model Compilation Test**: Run test suite `cargo test -p hadron-gluon` to ensure standard model verification tests still pass under updated rule wording.
