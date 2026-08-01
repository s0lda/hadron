# Hadron UI Redesign Specification: The Unified Swarm Command Deck

**Date**: 2026-08-01  
**Status**: Approved Specification  
**Architecture**: Native Rust + GPUI (`crates/hadron-chamber`)  

---

## 1. Executive Summary & Design Vision

Hadron is a native, GPU-accelerated multi-agent execution environment and swarm operating system built in Rust using GPUI.

This specification defines the redesign of the Hadron GUI (`hadron-chamber`) into **The Unified Swarm Command Deck**. The redesign eliminates rigid 1px panel borders in favor of a borderless glass surface hierarchy, introduces responsive spatial layouts for multi-agent interaction, and guarantees **zero-lag rendering performance** during high-frequency AI turn streaming.

---

## 2. Visual System & Zero-Lag Performance Architecture

### 2.1 Borderless Glass Surface Hierarchy
The layout replaces hard 1px box borders with a 3-tier elevation system:
* **Layer 0 (Canvas Base)**: Ultra-dark cosmic obsidian (`#090B10`).
* **Layer 1 (Panels & Rails)**: Translucent glass containers (`#111520` at 85% opacity) with rounded corners (`rounded_xl()`) floating over the canvas base.
* **Layer 2 (Floating Cards & Modals)**: Elevated cards (`#181E2C`) with a subtle 1px highlight rim (`rgba(255, 255, 255, 0.06)`) for pop-out depth without heavy box shadows.

### 2.2 Zero-Lag Performance Invariants
To prevent frame drop and stutter during high-frequency token streaming:
1. **No Animated Full-Panel Blurs**: Avoid animating multi-pass box-shadows or window-wide backdrop blurs on tick updates.
2. **Static Vector Indicators**: Quark execution states are expressed through lightweight GPU-native elements:
   - **Avatar Halo Dot**: 8px vector indicator (Cyan = executing tool, Violet = reasoning/drafting, Emerald = turn clean, Red = error).
   - **Draft Accent Strip**: Static 2px colored top bar on active draft cards (`bg(quark_color)`).
3. **Low-Overhead Streaming Drafts**: Live draft text streams as plain text within pre-allocated containers, bypassing markdown parsing and layout recalcs during streaming. Markdown is parsed once upon turn completion.

---

## 3. Structural Components

### 3.1 Left Rail: Swarm & Quark Fleet Roster
* **Header**: Floating Swarm & Worktree Selector chip displaying current workspace directory and active Git branch (`main` vs `worktree/quark-1`).
* **Quark Fleet Cards**:
  - Borderless glass cards displaying avatar/glyph, status halo dot, Quark name, model tag (`gemini-3.6-flash`, `claude-3.5-sonnet`), and transport protocol (`ACP`, `CLI`, `SDK`).
  - Subdued telemetry metrics (`1.4k t/m` throughput, turn duration).
  - Quick-select shortcut hints (`Alt+1..9`).
* **Foot Deck**:
  - **F6 Security Mode Pill**: Quick toggle displaying security posture (`ASK` / `WRITE` / `AUTO` / `BYPASS`).
  - **Add Seat Button**: Trigger to spawn new ACP daemons, CLI seats, or SDK agents.

### 3.2 Center Deck: Swarm Event Stream & Floating Command Bar
* **Header**: Segmented floating capsule tabs (`Chat` | `Event Log` | `Timeline`).
* **Event Cards**:
  - **Human Prompts**: Sleek dark glass cards with timestamps and file/quark `@` mentions.
  - **Quark Responses**: GPUI-accelerated markdown rendering, code blocks, syntax highlighting, and 1-click copy actions.
  - **AST Forge Diff Widget**: Embedded card for `Edit-by-Hash` modifications showing target file, `blake3` hash verification badge, and expandable side-by-side/unified AST diff preview.
  - **Live Draft Card**: Pinned above input bar during streaming with low-overhead plain text rendering.
* **Floating Command Capsule**:
  - Suspended 12px off the window bottom over a soft gradient fade.
  - Multi-line autosizing prompt input field with `Ctrl+Tab` navigation hint.
  - Toolbar with `@` Quark mention selector, `@` File context picker, `F6` mode badge, and submit button.

### 3.3 Right Rail: Inspector & Multitool
* **Segmented Header Tabs**: `PTY Terminal` | `Git & Merge Gate` | `Telemetry & Stats`.
* **Multi-Terminal PTY Inspector**:
  - Support for multiple tabbed terminal sessions (`bash #1`, `npm run dev`, `cargo test`).
  - `+` New Terminal Tab button and split terminal view support.
  - Keyboard shortcuts: `Ctrl+Shift+T` (new terminal tab), `Ctrl+Shift+W` (close tab), `Alt+1..4` (switch terminal tabs).
* **Git & Merge Gate Visualizer**:
  - Worktree tree view for active isolated Quark branches.
  - Visual status of automated test suite verification runs.
  - One-click Merge Gate release button to fast-forward clean branches into `main`.
* **Swarm Telemetry & Stats**:
  - Per-Quark token consumption breakdown (input/output tokens, cost estimation).
  - Real-time `field.jsonl` zero-CPU event bus throughput ticker.

---

## 4. Keyboard Navigation & Accessibility Matrix

| Shortcut | Scope | Action |
| :--- | :--- | :--- |
| `Ctrl+Tab` / `Ctrl+\`` | Global | Toggle focus between Chat Command Input and Active PTY Terminal |
| `Alt+Left` / `Alt+Right` | Center Deck | Switch Chat / Event Log / Timeline tabs |
| `Alt+PageUp` / `Alt+PageDown` | Right Rail | Switch Inspector tabs (Terminal / Git / Telemetry) |
| `Ctrl+Shift+T` / `Ctrl+Shift+W` | PTY Terminal | Create / Close terminal instances |
| `F6` | Global | Cycle security permission mode (`ASK` $\rightarrow$ `WRITE` $\rightarrow$ `AUTO` $\rightarrow$ `BYPASS`) |
| `Alt+1..9` | Left Rail | Quick-focus Quark agent stream |

---

## 5. Implementation Strategy & Verification

1. **Phase 1: Design Tokens & Glass Theme (`crates/hadron-chamber/src/theme.rs`)**
   - Implement borderless glass elevation colors, rounded corner utility specs, and zero-blur halo dot components.
2. **Phase 2: Roster & Command Capsule (`crates/hadron-chamber/src/app/render/roster.rs` & `chat.rs`)**
   - Refactor Left Rail and Floating Command Capsule to use new elevation containers.
3. **Phase 3: Multi-Terminal Inspector (`crates/hadron-chamber/src/app/render/terminal.rs` & `pty.rs`)**
   - Add tabbed terminal management in `PtyState` / `terminal_pane`.
4. **Phase 4: AST Forge Diff Widget & Merge Gate Integration (`crates/hadron-chamber/src/app/render/git.rs`)**
   - Enhance inline diff cards with `blake3` verification chips and worktree release controls.
