# `/research` Command & Custom Theme Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `/research` slash command with structured document lifecycle in `.hadron/docs/research/`, and deliver a complete custom theme and color customization system with syntax highlighting customization and live preview in Hadron Chamber.

**Architecture:** 
- `/research` command is registered in `hadron_chamber::text::COMMANDS` (SSOT), persisting structured research documents with frontmatter and analysis sections under `.hadron/docs/research/`, integrated into `hadron_chamber::model::tasks` for retitling and breadcrumb navigation.
- Themes are represented by `ThemeDefinition` with granular surfaces, accents, text tiers, terminal tokens, and 28+ syntax highlighting tokens. The runtime engine in `hadron_chamber::theme` provides lockless access, with preset fallback and live customization controls in Settings -> Appearance.

**Tech Stack:** Rust, GPUI, Serde JSON, Hadron Chamber, Gluon, Lattice, Forge MCP.

---

## Phase 1: `/research` Command Grammar & Document Lifecycle

### Task 1: Command Table & Chat Parsing for `/research` (commit `848b1c5a`)
**Files:**
- Modify: `crates/hadron-chamber/src/text.rs`
- Modify: `crates/hadron-chamber/src/app/actions.rs`
- Test: `crates/hadron-chamber/src/app/input.rs` (`every_listed_command_is_handled`)

- [x] Step 1.1: Add `research` to `hadron_chamber::text::COMMANDS` with `Arity::Line`, `ArgSource::None`, and `listed: true`.
- [x] Step 1.2: Add dispatch handling for `/research` in `crates/hadron-chamber/src/app/actions.rs`.
- [x] Step 1.3: Run `cargo test -p hadron text` to verify command table consistency.

### Task 2: Research Document Template & Task Tracker Retitling (commit `9acd9834`)
**Files:**
- Modify: `crates/hadron-chamber/src/model/tasks.rs`
- Modify: `crates/hadron-lattice/src/nucleus.rs`
- Test: `crates/hadron-chamber/src/model/tasks.rs`

- [x] Step 2.1: Add `retitle_from_research` in `hadron-chamber::model::tasks` to scan `.hadron/docs/research/` and upgrade task titles.
- [x] Step 2.2: Add unit tests in `tasks.rs` verifying research document title extraction.

### Task 3: Forge & MCP Research Tooling (commit `e2df77e5`)
**Files:**
- Create: `crates/hadron-forge/src/research.rs`
- Modify: `crates/hadron-forge/src/lib.rs`
- Create: `crates/hadron-forge-mcp/src/tools/research.rs`
- Modify: `crates/hadron-forge-mcp/src/lib.rs`

- [x] Step 3.1: Implement `write_research`, `list_research`, and `read_research` in `hadron-forge`.
- [x] Step 3.2: Expose research tools in `hadron-forge-mcp`.
- [x] Step 3.3: Run `cargo test -p hadron-forge -p hadron-forge-mcp` to verify tool handlers.

---

## Phase 2: Custom Theme Schema & Runtime Engine

### Task 4: Unified `ThemeDefinition` Data Model (commit `00763236`)
**Files:**
- Modify: `crates/hadron-chamber/src/config.rs`
- Modify: `crates/hadron-chamber/src/theme.rs`
- Test: `crates/hadron-chamber/src/config.rs`

- [x] Step 4.1: Define `ThemeDefinition`, `SurfacePalette`, `AccentPalette`, `TextPalette`, `SyntaxPalette`, `TerminalPalette` with Serde support.
- [x] Step 4.2: Implement preset-to-definition conversions for built-in themes (`Obsidian`, `Oled`, `Midnight`, `Tokyo`).
- [x] Step 4.3: Add unit tests verifying serialization, deserialization, and color hex parsing.

### Task 5: Dynamic Runtime Theme Swapping (commit `d6ce7ca8`)
**Files:**
- Modify: `crates/hadron-chamber/src/theme.rs`
- Test: `crates/hadron-chamber/src/theme.rs`

- [x] Step 5.1: Replace static preset lookup with active `ThemeDefinition` resolution.
- [x] Step 5.2: Update all semantic token functions (`canvas_base`, `bg_surface`, `syntax_keyword`, `syntax_function`, `syntax_string`, etc.) to read from the active theme.
- [x] Step 5.3: Add unit tests verifying dynamic theme switching and token fidelity.

---

## Phase 3: Settings Appearance UI & Live Theme Editor

### Task 6: Custom Theme Editor in Settings Overlay (commit `31d4d158`)
**Files:**
- Modify: `crates/hadron-chamber/src/app/settings/mod.rs`
- Modify: `crates/hadron-chamber/src/app/settings/overlay.rs`
- Modify: `crates/hadron-chamber/src/app/settings/providers.rs`
- Test: `crates/hadron-chamber/src/app/settings/tests.rs`

- [x] Step 6.1: Add "Custom Themes" section to Appearance settings with "New Custom Theme", "Import Theme", and "Export Theme" actions.
- [x] Step 6.2: Build palette color picker controls and hex inputs for surfaces, accents, and syntax tokens.
- [x] Step 6.3: Implement theme persistence to `~/.hadron/themes/<id>.json` and active theme activation.

### Task 7: Live Syntax Highlighting & UI Preview Panel (commit `31d4d158`)
**Files:**
- Modify: `crates/hadron-chamber/src/app/settings/overlay.rs`
- Test: Manual UI inspection / unit tests

- [x] Step 7.1: Add a live code snippet preview block inside the Appearance settings panel showcasing syntax tokens.
- [x] Step 7.2: Add UI component preview strip (buttons, badge, status halo dots, prompt text) dynamically reacting to theme edits.
- [x] Step 7.3: Verify saving updates `~/.hadron/chamber.json` and repaints UI immediately.

---

## Phase 4: Chamber Plan Rail Viewport Bugfix

### Task 8: Plan Rail Scroll Viewport & Bottom Margin Clearance (commit `21192072`)
**Files:**
- Modify: `crates/hadron-chamber/src/app/render/terminal.rs`
- Test: `crates/hadron-chamber/src/app/render/terminal.rs`

- [x] Step 8.1: Change `RightRailTab::Plan` outer container from `.size_full()` to `.flex_1().min_h_0().w_full().relative()` to prevent flex height overflow past card header.
- [x] Step 8.2: Add bottom clearance padding (`pb_16()`) to the plan checklist `list` container ensuring the bottom-most item is fully visible and not cut off on scroll.
- [x] Step 8.3: Run `cargo test -p hadron` to verify render compilation and regression safety.

---

## Phase 5: Full Workspace Verification & Nucleus Documentation

### Task 9: Workspace Gate & Nucleus Feature Map Update
**Files:**
- Modify: `.hadron/nucleus/features.md`

- [ ] Step 9.1: Run `cargo test --workspace` to ensure zero regressions across all crates.
- [ ] Step 9.2: Update `.hadron/nucleus/features.md` with `/research` command and Custom Theme Engine documentation.

