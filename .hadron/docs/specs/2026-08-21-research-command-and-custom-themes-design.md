# `/research` Command & Custom Theme Engine Design Specification

- **Date**: 2026-08-21
- **Status**: Validated / Approved (Bypass Autonomous Mode)
- **Target Crates**: `hadron-lattice`, `hadron-gluon`, `hadron-forge`, `hadron-forge-mcp`, `hadron-chamber`

---

## 1. Executive Summary & Problem Statement

This specification addresses two core user requirements in the Hadron developer ecosystem:

1. **Autonomous `/research` Command & Document Lifecycle**:
   Developers and autonomous quarks frequently need deep exploratory research, architectural investigations, dependency trade-off analysis, and protocol evaluations *before* writing formal specifications or implementation plans. Currently, exploratory findings either clutter chat history or are loosely placed in temporary scratchpads. We establish `/research <topic>` as a first-class slash command and workflow, persisting structured research documents under `.hadron/docs/research/YYYY-MM-DD-<topic>-research.md`, integrated into task tracking, breadcrumbs, and MCP tooling.

2. **Custom Themes & Comprehensive Color Customization**:
   Hadron Chamber currently provides 4 static dark presets (`Obsidian`, `Oled`, `Midnight`, `Tokyo`) and 6 fixed accent colors stored via atomic integer lookups. Users require complete customization of interface surfaces, typography tiers, glowing corner whispers, file icon mappings, and granular code syntax highlighting tokens (keywords, functions, types, comments, operators, etc.), with custom theme creation, import/export, and live visual preview in Settings.

---

## 2. System Architecture & Component Specifications

```
+---------------------------------------------------------------------------------------+
|                                    HADRON CHAMBER                                     |
|                                                                                       |
|  +--------------------+   +------------------------+   +---------------------------+  |
|  | /research Command  |   | Settings -> Appearance |   | Dynamic Theme Engine      |  |
|  | - Chat Parser      |   | - Custom Theme Builder |   | - Base & Surface Tokens   |  |
|  | - Autocomplete     |   | - Hex/Color Inputs     |   | - Syntax Highlighting     |  |
|  | - Breadcrumb Nav   |   | - Live Syntax Preview  |   | - Thread-safe ArcSwap     |  |
|  +---------+----------+   +-----------+------------+   +-------------+-------------+  |
+------------|--------------------------|------------------------------|----------------+
             |                          |                              |
             v                          v                              v
+-----------------------+   +------------------------+   +---------------------------+
|  .hadron/docs/        |   | ~/.hadron/themes/      |   | hadron-lattice / gluon    |
|  research/*.md        |   | <custom-theme>.json    |   | - Research event routing  |
|  - Research schema    |   | - Full palette schema  |   | - Forge MCP research tools|
|  - Task HUD linkage   |   | - chamber.json pref    |   | - Prompt injection & index|
+-----------------------+   +------------------------+   +---------------------------+
```

---

## 3. Subsystem 1: `/research` Command & Document Lifecycle

### 3.1 Slash Command Definition & Grammar
- **Command Registration**: Added to `hadron_chamber::text::COMMANDS` (SSOT):
  ```rust
  Command {
      name: "research",
      detail: "Research a topic or architecture and save findings to .hadron/docs/research/",
      arity: Arity::Line,
      arg: ArgSource::None,
      listed: true,
  }
  ```
- **Execution Semantics**:
  - `/research <topic>` opens an autonomous research workflow.
  - When executed by the human or orchestrator, it routes an investigation request to a research specialist quark or handles it via the `research` skill.
  - Generates a timestamped markdown document: `.hadron/docs/research/YYYY-MM-DD-<slug>-research.md`.

### 3.2 Research Document Schema & Template
Each research document adheres to a standardized structure:
```markdown
# Research: <Topic Title>

- **Date**: YYYY-MM-DD
- **Author**: @<Quark> / Human
- **Status**: Completed | In Progress
- **Target Area**: crates/<crate-name> or subsystem

---

## 1. Executive Summary
High-level overview of the investigated domain and core conclusions.

## 2. Key Findings & Current State Analysis
Detailed breakdown of codebase mechanisms, external libraries, or protocols evaluated.

## 3. Technical Constraints & Invariants
Operational boundaries, performance considerations, and Standard Model invariants affecting this domain.

## 4. Approaches & Trade-offs
Comparison table or structured analysis of 2-3 potential design choices with pros/cons.

## 5. Architectural Recommendations & Next Steps
Actionable path forward (e.g. transitioning to `/spec` or `/plan`).

## 6. References & File Pointers
Specific code symbols, documentation links, and commit references.
```

### 3.3 Integration with Chamber Task Tracker & Breadcrumbs
- **Task Retitling**: `hadron-chamber::model::tasks` extends `retitle_from_plan` to also scan `.hadron/docs/research/` via `retitle_from_research`, giving clear titles to active research runs.
- **Breadcrumb Navigation**: Clicking a research reference in chat opens the file in the user's configured editor.
- **MCP Tool Exposure (`hadron-forge-mcp`)**:
  - `write_research`: Autonomous tool for quarks to create and update research documents.
  - `list_research`: Returns existing research papers and topics.
  - `read_research`: Reads specific research findings.

---

## 4. Subsystem 2: Custom Themes & Complete Color Customization

### 4.1 Theme Data Schema (`ThemeDefinition`)
A unified, serializable theme definition supporting full palette coverage:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeDefinition {
    pub id: String,
    pub name: String,
    pub is_dark: bool,
    pub surfaces: SurfacePalette,
    pub accents: AccentPalette,
    pub text: TextPalette,
    pub syntax: SyntaxPalette,
    pub terminal: TerminalPalette,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfacePalette {
    pub canvas_base: String,        // e.g. "#050505"
    pub bg_base: String,            // e.g. "#0b0b0b"
    pub bg_surface: String,         // e.g. "#101010"
    pub bg_surface_raised: String,  // e.g. "#1c1c1c"
    pub bg_elevated: String,        // e.g. "#242424"
    pub input_bg: String,           // e.g. "#181818"
    pub border: String,             // e.g. "#444444"
    pub popover: String,            // e.g. "#141414"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccentPalette {
    pub primary: String,            // Active accent (e.g. "#c084fc")
    pub glow_blue: String,          // Working / excited corner glow
    pub glow_pink: String,          // Thinking / reasoning corner glow
    pub glow_green: String,         // Success / online corner glow
    pub glow_amber: String,         // Waiting / attention corner glow
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPalette {
    pub primary: String,            // Primary text "#e8e8e8"
    pub secondary: String,          // Secondary text "#a8a8a8"
    pub muted: String,              // Muted text "#707070"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyntaxPalette {
    pub keyword: String,            // "#f97583"
    pub function: String,           // "#b392f0"
    pub r#type: String,             // "#79b8ff"
    pub string: String,             // "#9ecbff"
    pub number: String,             // "#79b8ff"
    pub comment: String,            // "#7e888c"
    pub operator: String,           // "#f97583"
    pub variable: String,           // "#e1e4e8"
    pub constant: String,           // "#79b8ff"
    pub attribute: String,          // "#b392f0"
    pub tag: String,                // "#85e89d"
    pub boolean: String,            // "#79b8ff"
    pub delimiter: String,          // "#f97583"
    pub punctuation: String,        // "#bbbebf"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalPalette {
    pub bg: String,                 // "#080808"
    pub fg: String,                 // "#e8e8e8"
    pub prompt: String,             // "#4ade80"
}
```

### 4.2 Dynamic Runtime Theme Engine (`hadron-chamber::theme`)
- **Fast Lockless Access**: Rather than hardcoded match arms over `ThemePreset`, `theme.rs` holds an `ArcSwap<ResolvedTheme>` (or `RwLock<ResolvedTheme>`), pre-calculating GPUI `Rgba` and `Hsla` values.
- **Preset Backward Compatibility**: The 4 built-in presets (`Obsidian`, `Oled`, `Midnight`, `Tokyo`) and 6 accent colors are converted into default `ThemeDefinition` instances, ensuring zero regression for existing user configurations.
- **Custom Theme Storage**:
  - Presets and user themes are loaded from `~/.hadron/themes/<name>.json`.
  - `ChamberPrefs` stores `custom_theme: Option<String>` or active theme identifier.

### 4.3 Chamber Settings UI: Theme Customization Interface
In `Settings -> Appearance`:
- **Theme Mode Switcher**: Select between built-in presets or user-defined custom themes.
- **Custom Theme Creator / Editor**:
  - Color Pickers / Hex text inputs for Surface tiers, Accents, and Text colors.
  - Granular Syntax Highlighting Color Matrix (Keyword, Function, Type, String, Comment, Variable, etc.).
  - **Live Preview Panel**: Real-time rendering of a code sample and UI badge/card strip demonstrating color combinations before saving.
  - **Actions**: "Create New Theme", "Duplicate", "Export Theme JSON", "Import Theme JSON", "Reset to Default".

### 4.4 Chamber Plan Rail Viewport Layout & Bottom Clearance
- **Root Cause**: `RightRailTab::Plan` in `crates/hadron-chamber/src/app/render/terminal.rs` uses `.size_full()` on its outer wrapper instead of `.flex_1().min_h_0()`. Inside a parent flex card with a fixed header (`px_3() py_2()`), `.size_full()` causes the scroll container to overflow by the header height (~36px), clipping the bottom-most task item out of the viewport.
- **Remediation**:
  - Convert outer wrapper in `RightRailTab::Plan` to `.flex_1().min_h_0().w_full().relative()`, matching the sibling `Tasks` and `Changes` tabs.
  - Add explicit bottom clearance padding (`pb_8()`) to `list` so task items and DAG wave graphs scroll comfortably clear of the bottom border.

---

## 5. Testing & Verification Strategy

1. **Unit Tests**:
   - `hadron-chamber::text`: Test `/research` command parsing, arity validation, and listed status in `every_listed_command_is_handled`.
   - `hadron-chamber::theme`: Test `ThemeDefinition` serialization/deserialization, hex parsing safety, fallback resolution, and runtime theme switching.
   - `hadron-chamber::model::tasks`: Test research document discovery and task retitling.
   - `hadron-forge-mcp`: Test `write_research` and `list_research` MCP tools.
2. **Integration Verification**:
   - Verify `/research <topic>` correctly creates `.hadron/docs/research/` directory and writes standardized template.
   - Verify saving custom theme updates `~/.hadron/chamber.json` and immediately propagates colors across Chamber view elements without requiring application restart.
3. **Standard Model Gate**:
   - Full workspace test suite `cargo test --workspace` must pass with zero failures.
