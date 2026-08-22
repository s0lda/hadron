# Changelog

All notable changes to Hadron will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.17.0] - 2026-08-22

### Added
- **Universal Full-Color Emoji Font Fallback (`gpui_wgpu`, `s0lda/zed`, `hadron-chamber`)**:
  - Patched Zed's `gpui_wgpu` (`cosmic_text_system.rs`) in the `s0lda/zed` fork (`84ce4d78`) to prevent fontdb purges of `Noto Color Emoji` during charmap probes.
  - Added automatic font fallback routing for Unicode emoji codepoints and symbols (`😘`, `❤️`, `☺`, `⚡`, `☕`, `⭐`), bypassing monochrome dingbat glyphs in primary UI and monospace fonts.
  - Ensured emoji runs are correctly tagged with `is_emoji: true` and rendered with swash BGRA color bitmap sources across all bundled fonts (`Inter`, `Geist`, `Noto Sans`, `Cascadia Code`, `JetBrains Mono`, `Fira Code`).
- **Theme Editor Overhaul & 33 Color Tokens (`hadron-chamber`)**:
  - Added named custom theme creation (`theme_name_input`), direct save to `~/.hadron/themes`, new theme cloning, and preset resetting in Appearance Settings.
  - Built interactive color swatch editor card with live Hex code input (`#rrggbb`), GPUI ColorPicker modal integration, and a curated 20-swatch quick palette.
  - Populated all 33 design and syntax color tokens categorized across Surfaces (8), Text & Accents (8), Terminal (3), and Syntax Highlighting (14).
- **Multi-Terminal Grid Dynamic Viewport Fitting & Scrollback (`hadron-chamber`)**:
  - Constrained 1–4 terminal grids with `flex_1()`, `min_h_0()`, and `min_w_0()` so multi-terminal layouts divide viewport space evenly without stacking or overflowing.
  - Added dedicated vertical scrolling for large terminal collections ($\ge 5$) or compact windows.
  - Implemented per-card mouse wheel scroll forwarding directly to VTE scrollback buffers (`term.scroll()`).
  - Added rich ANSI snapshot color rendering, cursor positioning, and interactive click-to-select terminal tabs.
- **File Tree Markdown & Code Preview Viewport Clearance (`hadron-chamber`)**:
  - Added bottom clearance padding (`pb_16`) and strict flex container bounds (`min_h_0()`, `min_w_0()`, `size_full()`) to File Tree preview and scroll containers so previewed documents remain visible above the viewport edge.

### Fixed
- **Emoji Dingbat Glyph Collision**:
  - Prevented monochrome vector outlines from Inter and system text fonts from intercepting emoji codepoints.
- **Multi-Terminal Grid Stacking**:
  - Resolved flex expansion causing terminal grid cards to overflow vertically offscreen when 3 or more terminals are active.
- **Workspace Git Hygiene**:
  - Untracked internal `.hadron/docs/` and plans from Git index and ensured `.gitignore` coverage.

## [0.16.0] - 2026-08-22

### Added
- **`/research` Slash Command & Architectural Research Lifecycle (`hadron-chamber`, `hadron-forge`, `hadron-forge-mcp`, `hadron-gluon`, `hadron-lattice`)**:
  - Added `/research <topic>` slash command in chat and autocomplete to trigger in-depth codebase/topic investigations.
  - Implemented structured research document engine in `hadron-forge` and exposed via MCP tools (`hadron_forge_research_write`, `hadron_forge_research_list`, `hadron_forge_research_read`) targeting `.hadron/docs/research/`.
  - Added automatic skill injection (`research_ref`) in Gluon and document title synchronization with active task models (`retitle_from_research`).
- **Custom Theme Engine & Live Appearance Editor (`hadron-chamber`)**:
  - Implemented `ThemeDefinition` schema supporting custom RGB/hex color overrides across surface, accent, text, syntax, and terminal palettes.
  - Built dynamic runtime theme resolver and active custom theme lock enabling instantaneous palette swaps without restarting the chamber.
  - Added interactive Appearance Settings editor with custom theme creation, palette picking, and a live syntax/UI control preview card.
- **Native Markdown Badges & Responsive Image Rendering (`hadron-chamber`)**:
  - Implemented native `BadgePlugin` rendering shields.io and badgen SVG/URL badges as interactive colored GPUI pills with click-to-open links.
  - Implemented responsive `ImagePlugin` with local file resolution against workspace root, HTML `<img>` tag support, and presentation `<div>` unwrapping.
  - Added zero-copy image dimensions metadata probing (PNG, JPEG, GIF, WebP) with formatted file size and pixel dimension tags.
  - Locked preview images into responsive `Fit` layout with `min_w_0()` and `overflow_hidden()` to prevent container blowout.
  - Implemented WSL-aware external viewer launcher (`open_path_or_url`) for one-click image opening and link navigation without PowerShell or snapd errors.

### Fixed
- **Plan Rail Viewport Clearance (`hadron-chamber`)**:
  - Added bottom clearance padding (`pb_16`) and strict flex container bounds (`flex_1().min_h_0()`) to the Plan tab list so the final task is never clipped by the viewport boundary.
- **Emoji Font Preservation under Font Probes (`hadron-chamber`)**:
  - Added `is_emoji_or_symbol_font` guard preventing font pickers and bold probes from removing `Noto Color Emoji` from the cosmic-text font database.

## [0.15.0] - 2026-08-21

### Added
- **Next-Gen Swarm Orchestration Suite (`hadron-gluon`, `hadron-lattice`, `hadron-gatekeeper`, `hadron-forge`, `hadron-chamber`)**:
  - **Hybrid DAG Auto-Dispatch Engine (`hadron-gluon`)**: Automated multi-quark topological task graph scheduler with wave rank resolution and cycle-safe parallel dispatching.
  - **Reactive Wakeup Event Stream (`hadron-lattice`)**: Push-based lockless cross-worktree reactive wakeup signal broadcast system for zero-polling instant quark activation.
  - **Real-Time Heartbeat & Stall Recovery (`hadron-gluon`)**: Live activity heartbeat monitor and proactive recovery mechanisms for unresponsive or stalled worker turns.
  - **In-Flight Worktree Rebase Streamer (`hadron-gluon`)**: Live git rebase synchronization applying upstream `main` changes to running worktrees without interrupting turn execution.
  - **File-Level Intent Locks & Collision Detection (`hadron-lattice`)**: Multi-quark file intent locking mechanism preventing concurrent edit collisions across worktrees.
  - **Shared-Target Build Cache Guard & Metadata Salting (`hadron-gatekeeper`)**: Workspace build cache validator preventing stale foreign rlib reuse across concurrent worktrees.
  - **Zero-Friction Branch Sweeping & Auto-Pruning (`hadron-gluon`)**: Safe branch garbage collection with automated fast-forward checking, patch-id verification, and archive tagging.
  - **Atomic Multi-File Batch Edit Transactions (`hadron-forge`)**: All-or-nothing multi-file AST edit transactions with automatic single-commit rollback on verification failure.
  - **Ephemeral Zero-Footprint Scout Quarks (`hadron-gluon`)**: Lightweight scout agents for rapid repository reconnaissance and symbol discovery without creating disk worktree footprints.
  - **Automated Visual Smoke Testing & Layout Assertions (`hadron-forge`)**: Headless visual layout tester asserting GPUI element bounds, text clipping, and overflow constraints.
  - **Unified 3-Way Diff & Semantic Merge Resolver (`hadron-chamber`)**: Interactive 3-way visual diff inspector comparing Base (`main`), Ours (Worktree), and Theirs (Branch) with clean tree status.
  - **Swarm-Wide Vectorized Nucleus Semantic Search (`hadron-lattice`)**: Vector embeddings and cosine similarity retrieval engine across long-term memory notes.
  - **Time-Travel Debugging & State Rewind (`hadron-lattice`)**: Turn-by-turn state recording, snapshot diffing, and session branch fork exploration.
  - **Swarm Artifact Bus (`hadron-lattice`)**: Zero-copy bus for sharing binary artifacts, test logs, and structured payloads between quarks.
  - **Elastic Quota Redistribution & Budget Guardrails (`hadron-lattice`)**: Dynamic token quota balancer automatically reallocating unused budget across active model providers.
- **Chamber Roster & Plan Workspace Ergonomics (`hadron-chamber`)**:
  - **Interactive Plan DAG Visualizer**: Visualized multi-step plans as dependency DAGs with wave rank grouping, step status coloring, and task navigation.
  - **Scrollable Multi-Suite Plan Dropdown**: Added structured suite section grouping, active suite indicators, visual separators, and adaptive label width truncation.
  - **Roster Active / All Capsule Tabs**: Segmented filter tabs in Roster rail distinguishing repo-adopted active quarks from full catalogue entries, with keyboard shortcuts (`Alt+Shift+Left` / `Alt+Shift+Right`).
  - **Alphabetical Quark Sorting**: Automatically sorted quarks alphabetically by resolved display name across expanded roster cards and collapsed rail avatar bars.
  - **Inlined Rail Headers**: Unified Roster capsule tabs and Inspector sub-tabs inline with panel toggle buttons (`PanelLeftClose`/`PanelRightClose`) in horizontal header bars.
  - **Streaming Draft Speaking View Bounds**: Constrained streaming speaking draft cards to 5 scrollable lines (`max_h(px(100.0))`) with `overflow_y_scroll` to prevent occluding chat history.
  - **Gitignored Non-Heavy File Completions**: Included lightweight gitignored configuration/source files in quick-open completions while filtering heavy dependencies.
- **Autonomous Learning & Memory Invariants (`hadron-gluon`, `hadron-chamber`, `nucleus`)**:
  - Mandated 100% autonomous memory distillation directly into `.hadron/nucleus/` (`notes/` and `index.md`) with zero proposal documents or approval bottlenecks.
  - Codified Standard Model Rule 9 autonomous distillation invariant across all prompt adapters and skill definitions.

## [0.14.0] - 2026-08-21

### Added
- **Symbols File & Folder Icon Theme (`hadron-chamber`)**:
  - Integrated 355 multi-colored Symbols SVG assets (250 file icons and 105 folder icons) ported from VS Code / Zed Symbols.
  - Zero-allocation static theme resolver matching exact filenames (`Cargo.toml`, `package.json`, `Dockerfile`, `README.md`, etc.), special directories (`.git`, `.github`, `src`, `tests`, `docs`, `target`, `node_modules`), and file extensions (`.rs`, `.ts`, `.py`, `.go`, `.json`, `.toml`, `.md`, etc.).
  - Rendered via `gpui::img` with full multi-color SVG fills, strokes, and language brand badges across File Tree, Diff Inspector, and Quick Open search overlay.
  - Embedded asset loader and MIT attribution for Miguel Solorio and Joan S. Garcia in `THIRD_PARTY_NOTICES.md`.
- **GitHub Dark Semantic Syntax Theme (`hadron-chamber`)**:
  - Custom semantic syntax theme wired into global `gpui_component::Theme` (`HighlightTheme`), providing rich syntax highlighting for Markdown code blocks, chat messages, file previews, and Event Log entries.
  - Category-based file tree icon tinting and right-aligned Git status badges (`M` amber, `+` mint, `D` coral).
  - Syntax-preserving diff styling with semi-transparent green/red gutter overlays in Git and 3-way diff inspectors.
- **Chat Input Ergonomics & Auto-Scroll (`hadron-chamber`)**:
  - Configured chat input viewport to auto-scroll to the active cursor position on multi-line text paste and Shift+Enter newlines.
- **Unified Chamber Console Logging (`hadron-chamber`)**:
  - Converted raw terminal/PTY `println!`/`eprintln!` calls across `pty`, `terminal`, `sys`, and `widgets` modules to structured timestamped Hadron format (`HH:MM:SS chamber <message>`) via `hadron_lattice::term`.
- **Code-Wrapped Mention Isolation (`hadron-gluon`, `hadron-chamber`)**:
  - Markdown code fences (```/~~~) and inline backtick spans are automatically stripped from mention scanning across all routing and addressee parsers (`human_mentions`, `parse_addressee`, `task_names_card_specifically`), preventing unintentional Quark turn excitation when code snippets contain `@mentions`.

## [0.13.0] - 2026-08-20

### Added
- **Chamber Workspace & Activity Streamlining (`hadron-chamber`)**:
  - Implemented 2-tier activity model: dedicated live `Tasks` tab for swarm dispatches and expanded `Event Log` inspector for turn history and replay.
  - Sibling plan dropdown selector in `Plan` tab enabling instant switching between master plans and sub-phase plans.
  - Interactive collapsible `Plan & Nucleus Context` accordion in Plan view with one-click file jumping and automatic tab return upon closing file preview.
  - Interactive collapsible `Plan Overview` accordion for high-level mission objectives.
  - 3-Way Visual Diff Inspector in Git rail comparing Base (`main`), Ours (Worktree), and Theirs (Branch/HEAD) with real-time clean tree state detection.
  - Swarm audio cues and telemetry feedback indicators.
- **Next-Gen Swarm Capabilities & Engine Extensions (`hadron-gluon`, `hadron-lattice`, `hadron-gatekeeper`, `hadron-forge`, `hadron-forge-mcp`)**:
  - Speculative dual-execution merge gate and zero-copy CoW worktree provisioning (`hadron-gluon`).
  - Predictive token load balancer and semantic CAS cache (`hadron-gluon`).
  - Cross-worktree gossip bus and headless turn replay and bisection (`hadron-lattice`).
  - One-click field message to nucleus note promotion (`hadron-lattice`, `hadron-forge-mcp`).
  - Continuous background mutation quark and ephemeral cgroup/container gate sandbox (`hadron-gatekeeper`).
  - Autonomous codebase invariant synthesis engine (`hadron-gatekeeper`, `hadron-forge-mcp`).
  - Adaptive AST context slicing for prompt optimization (`hadron-forge`).
  - Multiplexed multi-quark PTY terminal grid and token spend/latency flamecharts (`hadron-chamber`).
  - Subfolder plan references and multi-file hierarchy support (`skills`).
- **Release Automation & Tag History Sync**:
  - Full annotated Git release tags with complete markdown changelog descriptions.
  - Automated GitHub Release publication synchronization via `gh release create`.

## [0.12.1] - 2026-08-16

### Fixed
- **Mermaid Source Code Toggle Crash (`hadron-chamber`)**:
  - Captured the `use_keyed_state` entity handle during `MermaidCard::render` in `crates/hadron-chamber/src/mermaid/render.rs` so the interactive `on_click` handler updates the pre-bound `Entity<bool>` directly, preventing panics caused by calling `self.current_view()` outside the GPUI render prepaint pass.

## [0.12.0] - 2026-08-16

### Added
- **Native GPUI Mermaid Diagram Rendering in Markdown (`hadron-chamber`)**:
  - Integrated Mermaid parser, topological layer ranker & layout engine, and interactive GPU/Lavapipe-rendered `MermaidCard` components directly into Markdown (`.md`) chat messages, message inspection cards, and log rows.
  - Comprehensive diagram format support:
    - Flowcharts (`flowchart` / `graph` TD, TB, BT, LR, RL) with standard and specialized node shapes (`[rect]`, `(rounded)`, `([stadium])`, `[[subroutine]]`, `[(cylinder)]`, `((circle))`, `[//parallelogram//]`, `[\\parallelogram\\]`, `[/trapezoid\\]`, `[\\trapezoid/]`, `{rhombus}`) and connector styles (`-->`, `---`, `-.->`, `==>`, labels `|label|`, multi-targets, subgraphs).
    - Sequence Diagrams (`sequenceDiagram`, `autonumber`, `actor`/`participant`, `->>`, `-->>`, notes).
    - Pie Charts (`pie title`, `showData`, numeric slices and percentages).
    - State Diagrams (`stateDiagram-v2`) and Class Diagrams (`classDiagram`).
  - Interactive features: Card header with diagram badge, metrics (nodes, edges), interactive `Diagram View` / `Source Code` toggle, and Copy action button.
  - Preview support for `.mermaid` and `.mmd` files in file previewers.

### Fixed
- **Color Emoji Fallback Chain (`hadron-chamber`)**:
  - Registered `EMOJI_FAMILY` (`"Noto Color Emoji"`) in GPUI and cosmic-text font fallback chains (`default_fallbacks()`), resolving emoji rendering (e.g. `🤣`, `🔥`, `✨`, `🎉`) and eliminating tofu replacement boxes across Linux and WSL environments.

## [0.11.0] - 2026-08-16

### Added
- **Next-Gen Swarm Capabilities Suite (`hadron-lattice`, `hadron-gatekeeper`, `hadron-gluon`, `hadron-forge`, `hadron-forge-mcp`)**:
  - `hadron_forge_time_travel`: Lattice turn rewind, state snapshot diffing, and session branch exploration (`hadron-lattice::time_travel`).
  - `hadron_forge_mutation_gate`: Adversarial AST mutation runner and test harness kill-rate validator (`hadron-gatekeeper::mutation`).
  - `hadron_forge_benchmark_guard`: Baseline execution timing and CPU flamegraph regression guard (`hadron-gatekeeper::benchmark_guard`).
  - `hadron_forge_topology_graph`: Spatial DAG and crate dependency topology visualizer/exporter (`hadron-forge::topology`).
  - `hadron_forge_task_scheduler`: Topological task graph optimizer and parallel Quark dispatch scheduler (`hadron-lattice::task_graph`, `hadron-forge::task_scheduler`).
  - `hadron_forge_preon_evolution`: Recurring failure pattern analyzer and autonomous preon rule synthesizer (`hadron-gluon::preon_evolution`).
  - `hadron_forge_prompt_distiller`: Per-model context compressor and token spend optimizer (`hadron-gluon::prompt_distiller`).
  - `hadron_forge_swarm_mesh`: Remote ACP worker container offload, status inspector, and runner (`hadron-gluon::mesh`).
  - `hadron_forge_pty_pairing`: Interactive shared PTY pairing broker with live steer and multi-quark observer sessions (`hadron-forge::pty_pairing`).
  - `hadron_forge_tool_breakpoints`: Dynamic pause/resume breakpoints on MCP tool execution and subprocess commands (`hadron-gluon::breakpoints`).
  - **Distributed P2P Sync (`hadron-lattice`)**: In-memory and network delta replication protocol for distributed event streams (`hadron-lattice::sync`).

## [0.10.0] - 2026-08-16

### Added
- **Hadron Forge Net-New MCP Tools Suite (`hadron-forge` & `hadron-forge-mcp`)**:
  - `hadron_forge_blast_radius`: Static workspace crate dependency graph, reverse caller traversal, and affected test suite impact calculator.
  - `hadron_forge_git_bisect`: Automated binary regression locator executing custom test predicates across commit spans.
  - `hadron_forge_wiretap`: Real-time NDJSON and HTTP IPC packet monitor, filter, and frame asserter for Quark-daemon communications.
  - `hadron_forge_ast_rewrite`: Tree-sitter powered structural pattern search and cross-file code transformations.
  - `hadron_forge_secret_vault`: Ephemeral sandboxed credential vault with automated stdout/stderr secret masking.
  - `hadron_forge_flamegraph_profiler`: Universal CPU and memory allocation profiler generating interactive SVGs and folded stack hotspot reports.
  - `hadron_forge_fuzz_harness`: Property-based randomized fuzz test runner for IPC codecs, serializers, and boundary conditions.
  - `hadron_forge_nucleus_graph`: Bidirectional knowledge graph analyzer for `.hadron/nucleus/` note connections, orphans, and invariant coverage.
  - `hadron_forge_binary_bloat`: ELF/DWARF symbol size analyzer, section breakdown, and binary bloat regression detector.
  - `hadron_forge_release_sync`: Conventional commit analyzer, SemVer bump calculator, and Keep-a-Changelog generator.

## [0.9.1] - 2026-08-16

### Added
- **Windows CLI Argv Channel Safety (`hadron-gluon`)**:
  - Gated `MAX_ARG_STRLEN` (32 KiB) and `SAFE_ARG_BYTES` (24 KiB) for Windows in `hadron-gluon::adapter::cli`, preventing `CreateProcessW` command-line length overflow (`os error 206`) when dispatching long prompt channels.

### Changed
- **Quark Lifecycle & Global Catalogue Filtering (`hadron-chamber`)**:
  - Configured Quark removal to purge definitions cleanly from the global catalogue (`~/.hadron/team.json`), repository seating overrides, and OS credential store.
  - Enforced Roster view projection (`project_with_team`) to render only catalogue-defined Quarks, dropping historical or orphan repo overrides.
- **3D Swarm Visualizer Topology & Draw Batching (`hadron-chamber`)**:
  - Precomputed static 64-vertex Fibonacci lattice nodes and static edge connectivity tables (`LATTICE_STATIC_EDGES`), eliminating >2,000 runtime Euclidean distance calculations per frame.
  - Replaced per-point Euler angle trigonometry with unified 3x3 `Rotation3D` composite rotation matrix.
  - Batched lattice line segments, parallels, and meridians into compound `PathBuilder` vector strokes, reducing GPUI path allocations and paint calls by >96%.

### Fixed
- **Release Build Warning Hygiene (`hadron-chamber`)**:
  - Gated test-only `Point3D` rotation helper methods behind `#[cfg(test)]`, eliminating dead code warnings during release compilation.

## [0.9.0] - 2026-08-16

### Added
- **Interactive 3D Cosmic Swarm Visualizer (`hadron-chamber`)**:
  - Added `RightRailTab::Visualizer` rendering mathematical 3D perspective projection $(\theta, \phi) \to (x, y, z)$ with yaw/pitch rotation matrices and depth scaling.
  - Implemented 64-vertex Fibonacci neural surface constellation wireframe, depth-attenuated cyan/indigo lattice lines, and glowing vertex data points.
  - Added multi-layered glowing singularity core (nebula haze, cosmic corona, inner radiant quantum core, pinpoint center) and deep-space cosmic star dust particle field.
  - Built inclined celestial equator belt with 16 traveling quantum photon pulses orbiting along the ring.
  - Added full interactive viewport controls: mouse drag orbit rotation, scroll-wheel zooming (50%–300%), camera reset, and click-to-focus Quark telemetry HUD card.
  - Accurate active/excited state calculation evaluating live files and in-flight tasks (`TaskState::Working`), displaying `"All Quarks Idle"` when no turns are executing.
- **Multi-Protocol Telemetry & Cost Reporting (`hadron-chamber` & `hadron-lattice`)**:
  - Added multi-protocol breakdown in `SessionStats` and `QuarkStats` tracking protocol-level turns across ACP, CLI, HTTP, and SDK transports.
  - Granular token metrics for input, output, cache-read, cache-write, and total cache hit rate %.
  - Aggregated event kind counters for file edits, terminal commands, and snapshots in `fold_stats`.
  - Added interactive warning badge and tooltip to Cost stat tile indicating partial provider spend reporting.
- **Streamlined Quark Onboarding & Seating Flow (`hadron-chamber`)**:
  - Added 1-click auto-connect, active model probe, seat creation, and instant persistence for keyless local providers (Ollama, LM Studio).
  - Added visual category badges (`Local HTTP`, `Resident ACP`, `CLI Subprocess`) to the preset catalog for rapid identification.
  - Implemented unified `save_and_add_http_quark` helper for direct seating across all HTTP endpoints.
- **Categorized Tool Invariants & Prompt Exposure (`hadron-gluon` & `hadron-forge`)**:
  - Expanded prompt tool documentation across all 62 Forge tools categorized by capability family.
  - Distilled invariants registry and memory notes.

### Fixed
- **Markdown & Code Block Styling Polish (`hadron-chamber`)**:
  - Added subtle border framing, padding, and rounded corner radii for markdown tables.
  - Refined code block border styling and background integration for improved readability in the chat field.

## [0.8.0] - 2026-08-15

### Added
- **Deterministic DAG Plan Engine (`hadron-gluon`)**:
  - Implemented `PlanDocument`, `parse_plan_markdown`, and `sync_plan_checkbox` in `hadron-gluon::engine::dag` for deterministic task graph parsing and bidirectional disk state synchronization with `.hadron/docs/plans/*.md`.
- **Autonomous Self-Healing Merge Gate (`hadron-gluon`)**:
  - Added structured compiler diagnostic extraction (`extract_structured_diagnostics`) and automated remediation instructions (`format_remediation_instructions`) into `hadron-gluon::engine::merge` to guide Quarks through autonomous self-healing loops upon gate failures.
- **Prompt-to-Product Autonomy Suite (`hadron-forge` & MCP)**:
  - **Autonomous Spec & DAG Plan Compiler (`hadron_forge_spec_compile`)**: Translates high-level natural language prompts into formal Design Specifications and validated Gluon DAG plans.
  - **Visual & Behavioral E2E Asserter (`hadron_forge_e2e_assert`)**: Automates multi-step headless browser navigation, form interactions, DOM text assertions, and visual screenshot verification.
  - **Packaging & Live Preview Launcher (`hadron_forge_preview_launch`)**: Detects build stacks (`cargo`, `npm`, `vite`), builds release artifacts, supervises isolated background servers with process-group teardown, and verifies loopback health endpoints.
- **Autonomous Lifecycle Powerpack (`hadron-forge` & MCP)**:
  - **Autonomous Scaffolder & Dependency Resolver (`hadron_forge_scaffold`)**: Standardizes project initialization across Rust, Vite (React, Vue, Svelte, Vanilla TS), Python, and Next.js stacks with lockfile verification.
  - **Security & Secret Scanner Gate (`hadron_forge_security_audit`)**: Static AST and dependency scanner detecting hardcoded API secrets, private keys, command/SQL injection risks, and unjailed path traversals.
  - **Runtime Service & Crash Watchdog (`hadron_forge_service_watchdog`)**: Real-time service log analysis detecting runtime panics, unhandled promise rejections, port binding conflicts, and emitting remediation guidance.
- **Multi-Modal Verification & Continuous Conflict Detection (`hadron-forge` & MCP)**:
  - Added multi-modal acceptance test suite execution with timeout and output capture (`hadron_forge_acceptance_gate`).
  - Added continuous cross-worktree conflict detection inspecting overlapping file edits and exported symbols before merge gate submission (`hadron_forge_peers_detect_conflicts`).
  - Added autonomous post-mortem lesson distillation into `notes/<slug>.md` while maintaining 1-line pointer formatting in `index.md` (`hadron_forge_nucleus_distill_lesson`).

## [0.7.0] - 2026-08-15

### Added
- **Developer Pre-Flight Merge Gate (`hadron-forge` & MCP)**:
  - Added in-worktree merge gate simulation (`hadron-forge::gate`) refreshing crate entrypoint mtimes to avoid stale `.rlib` test execution, performing `merge::sync` rebases against base, and running cargo test suites before yielding turns (`hadron_forge_preflight_gate` MCP tool).
- **Cross-Worktree Peer Inspector (`hadron-forge` & MCP)**:
  - Added peer worktree status querying across `.hadron/trees/*` (`git log base..HEAD`, uncommitted diffs, export symbol inspection) via `hadron_forge_peer_inspect` MCP tool without manual path navigation.
- **Nucleus Memory & Budget Linter (`hadron-forge` & MCP)**:
  - Added `hadron_forge_nucleus_lint` MCP tool enforcing the 32 KB `.hadron/nucleus/index.md` budget ceiling, validating YAML frontmatter schema (`name`, `description`, `metadata.type`), detecting unlinked notes, and verifying index routing invariants.
- **AST & LSP Symbol Hierarchy Intelligence (`hadron-forge` & MCP)**:
  - Added structured AST symbol extraction with line ranges and parent-child hierarchy across 14+ languages (`hadron_forge_symbol_hierarchy`) and generic in-process LSP queries (`hadron_forge_lsp_query`).
- **Atomic Coupled Multi-Branch Swarm Gate (`hadron-gluon`)**:
  - Implemented `land_coupled_branches` in `hadron-gluon::merge` supporting coordinated atomic validation and landing of coupled multi-branch features spanning multiple Quarks.
- **Headless Chamber Replay & State Projection Harness (`hadron-chamber`)**:
  - Added `HeadlessReplaySession` verifying model projection, list state invariants, and chat/log index resynchronization headless without requiring a display server.
- **Attention Required Spotlight & Event (`hadron-lattice` & `hadron-chamber`)**:
  - Added `AttentionRequired` field event and Chamber spotlight visual cue for immediate human attention on unrecoverable blockers or critical requests.
- **Automated Preset Seating & First-Quark Orchestrator Promotion (`hadron-chamber`)**:
  - Automated preset connection and immediate saving in the Add Quark wizard, automatically designated the 1st added Quark as Orchestrator on empty teams, and added an Orchestrator network icon badge with tooltip to the Roster rail.

### Fixed
- **Chamber Compiler Diagnostics & Badge Polish**:
  - Cleared all compiler warnings in `hadron-chamber` and streamlined Roster Orchestrator badge presentation to a clean icon chip with hover tooltip.

## [0.6.1] - 2026-08-15

### Fixed
- **Appearance Font Size Styling**:
  - Connected chat markdown messages, headings, and terminal file tree items to configured user Appearance font size preferences (`ui_font_size` and `mono_font_size`).
- **GPUI LeakDetector Exit Panic**:
  - Scoped GPUI `test-support` crate feature to `[dev-dependencies]` in `hadron-chamber`, preventing `LeakDetector` panics on normal application exit in binary builds.

## [0.6.0] - 2026-08-15

### Added
- **8 Built-in Engineering Skills & Slash Commands**:
  - Embedded first-class skill procedures and slash commands: `security-review` (`/security-review`, `/security`), `architecture-audit` (`/architecture-audit`, `/arch`), `chaos-testing` (`/chaos-testing`, `/chaos-test`), `performance-audit` (`/performance-audit`, `/perf-audit`, `/optimize`), `code-simplification` (`/code-simplification`, `/simplify`, `/refactor`), `api-design` (`/api-design`, `/contract`), `incident-investigation` (`/incident-investigation`, `/triage`, `/investigate`), and `memory-curation` (`/memory-curation`, `/curate-memory`).
  - Integrated specialist role routing (`architect`, `reviewer`, `qa`, `optimizer`, `triage`, `scribe`, `executor`) for automatic Quark turn selection.
- **Alphabetical Slash-Command Autocomplete**:
  - Implemented predictable A–Z alphabetical ordering for slash commands with prefix-match ranking and deduplicated alias presentation in the autocompletion menu.
- **Subcategorized General Settings Overlay**:
  - Deconstructed monolithic General settings into structured sub-pages: **Appearance** (Themes, Accent color, Typography), **Execution** (Max exchanges, Nucleus budget, Git merge strategy), and **Environment** (Code editor, Default mode, Close Gluon on Exit).
  - Added expandable sidebar navigation hierarchy with indented sub-category rows.
- **Quark Avatar Aura Rings**:
  - Styled avatar state rings in chat to dynamically render the active Quark's unique identity color during excited/in-flight states.
- **Git Inspector Keyboard Navigation & Bundled Emoji Font**:
  - Added keyboard navigation to Git Inspector tabs and bundled `Noto Color Emoji` fallback font ensuring reliable glyph rendering on Linux and WSL.

### Changed
- **Skill Path & Invariant Alignment**:
  - Replaced legacy foreign assistant references (`AGENTS.md`) with `.hadron` conventions and `Invariants (always.md / Standard Model)` priority hierarchy across all built-in skills.

## [0.5.0] - 2026-08-15

### Added
- **Developer Environment Power Tools Suite (`hadron-forge` & MCP)**:
  - **Jailed Desktop & Window Screenshot Engine**: Jailed screen capture manager enforcing strict containment within `<repo_root>/.hadron/screenshots/` (gitignored) with window-specific scoping and MCP tools (`hadron_forge_screenshot_capture`, `hadron_forge_screenshot_list`, `hadron_forge_screenshot_prune`).
  - **Interactive PTY Session Manager**: Native pseudo-terminal management (`libc::openpty`) with non-blocking stream readers, terminal resizing, ANSI pass-through, and MCP tools (`hadron_forge_pty_start`, `hadron_forge_pty_write`, `hadron_forge_pty_read`, `hadron_forge_pty_resize`, `hadron_forge_pty_kill`, `hadron_forge_pty_list`).
  - **In-Process Mock HTTP/WebSocket Server**: Local-origin mock server strictly bound to `127.0.0.1` supporting dynamic routes, request journaling, response assertion matching, and MCP tools (`hadron_forge_mock_start`, `hadron_forge_mock_route_add`, `hadron_forge_mock_requests_list`, `hadron_forge_mock_assert`, `hadron_forge_mock_stop`, `hadron_forge_mock_list`).
  - **Local SQLite Engine & Migration Runner**: Embedded SQLite engine with schema introspection, transactional migrations, multi-format export (Markdown/CSV/JSON), and MCP tools (`hadron_forge_sqlite_query`, `hadron_forge_sqlite_schema`, `hadron_forge_sqlite_migrate`, `hadron_forge_sqlite_export`).
- **Universal Assistant Absorption (`/absorb`)**: Universal assistant skill and `/absorb` chat command to dynamically scan and migrate foreign assistant memory and configurations (`.agents/`, `.claude/`, `CLAUDE.md`, `.cursor/`, `.windsurf/`, `.kimi/`, `.superpowers/`) into standard Hadron Nucleus notes, invariants, and skills respecting configured budget limits (16–128 KB).
- **Canonical Commands & Power Tools Documentation**: Added comprehensive documentation in `docs/commands.md` and `docs/forge/power-tools.md` covering all slash commands and Forge MCP tools.

### Changed
- **Neutral Quark Naming**: Standardized all command completions, glosses, error messages, and documentation on neutral `@Quark` identifiers.
- **Automated Documentation Release Protocol**: Added mandatory documentation audits and verification to the release procedure (`.hadron/nucleus/release.md`).

## [0.4.0] - 2026-08-14

### Added
- **Background Process Manager**: Added managed thread-safe supervisor (`ProcessManager`) in `hadron-forge` with process-group isolation (`libc::kill(-pgid)`), non-blocking ring-buffer streaming logs, and MCP tools (`process_start`, `process_logs`, `process_list`, `process_send_stdin`, `process_kill`).
- **3-Tier Polyglot Code Intelligence**: Added embedded Tree-Sitter AST symbol engine covering 15+ programming languages for instant caller and definition resolution, generic STDIO JSON-RPC 2.0 LSP client in `hadron-forge`, and MCP tools (`symbols_definition`, `symbols_references`, `symbols_document`, `symbols_callers`).
- **Polyglot Tooling & Structured Diagnostics**: Expanded jailed execution allowlist to JavaScript/TypeScript, Python, and Go runtimes (`node`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, `deno`, `python`, `python3`, `pytest`, `ruff`, `go`), and added multi-language structured compiler and test diagnostic parser (`diagnostics.rs`).
- **Headless Browser Verification Bridge**: Added local-origin CDP browser automation bridge in `hadron-forge-mcp` with accessibility tree snapshotting, full-page screenshots, element clicks, form fills, and script evaluations for autonomous UI and web verification.

## [0.3.1] - 2026-08-14

### Added
- **Keyboard-First Workflow & History Navigation**: Added global prompt focus (`Cmd+L` / `Ctrl+L`), chat input history navigation (`Up` / `Down` arrows), Tab autocompletion for slash commands/arguments, circular wrap-around autocomplete cycling, and Escape-to-dismiss for overlays and completions.
- **Modern Chamber UI Suite & Theme Presets**: Added dynamic Theme Presets (Obsidian Neutral, OLED True Black, Midnight Slate, Tokyo Dark) and customizable Accent Color picker in Settings and via `/theme` slash command, floating toast notifications, and GitHub-style markdown callouts (`[!NOTE]`, `[!TIP]`, `[!WARNING]`, `[!IMPORTANT]`, `[!CAUTION]`).
- **Surface Gluon Operations & Multi-Stage Gate Progress**: Real-time heartbeat tracking and live status reporting for Gluon daemon operations, worktree provisioning, syncs, test suite runs, AST conflict checks, and branch pruning.
- **Nucleus Linter & Core DAG Engine**: Added `NucleusLinter` in `hadron-lattice` enforcing memory budgets and pointer validity, channel-based event segmentation for tool bursts, `TaskDag` scheduler in `hadron-gluon`, and auto-reconciliation in `hadron-forge`.

### Changed
- **Vector Icons & UI Polish**: Replaced Unicode emoji across autocompletions, action pills, and callouts with crisp vector icons, refined avatar state rings, diff gutters, and streamlined live activity columns with fixed-width alignment.
- **Documentation Suite & Keybinding Mapping**: Updated documentation across README, vocabulary, architecture, and forge edit-by-hash guide, restoring `Ctrl+Tab` / `Ctrl+\`` chat-terminal focus toggling and multi-terminal tab navigation.

### Fixed
- **Terminal Focus & Escape Handling**: Preserved active terminal focus and forwarded Escape sequences directly to PTY sessions without erroneously stealing focus back to chat.
- **Async Stats & Chart Downsampling**: Offloaded telemetry stats aggregation to background executor and implemented chart point downsampling to eliminate software rasterization lag under Lavapipe.
- **Theme Persistence & Dynamic Token Updates**: Resolved theme and accent label parsing and token reactivity during configuration updates.

## [0.3.0] - 2026-08-14

### Added
- **Hub-and-Spoke Swarm Routing & Orchestrator Dispatch**: Restricted swarm-level quark delegation (`@<quark>`) exclusively to the Orchestrator prompt, and directed worker Quarks to communicate exclusively with `@orchestrator` to prevent unmerged worktree collisions and runaway loops while permitting internal subagents.
- **Git Workspace Initializer (`/git-init`)**: Added `/git-init` slash command and interactive "Git Workspace Required" banner with one-click repository initialization, automatic `.gitignore` seeding, and initial commit creation.
- **Autonomous Mission & Iteration Commands (`/goal` & `/loop`)**: Added `/goal` slash command for objective-driven planning and `/loop` command for iterative autonomous cycles with mode-aware execution recommendations.
- **Automatic Plan Tab Directory Scanning**: Enhanced Plan inspector tab to automatically scan and resolve the newest plan across `.hadron/docs/plans/`, `docs/plans/`, and worktrees.

### Changed
- **Posture-Aware Workflow Skills**: Refactored `brainstorming`, `subagent-driven-development`, and `finishing-a-development-branch` to support seamless autonomy in Bypass mode while maintaining interactive gates in Ask mode and aligning with the automated Gluon Merge Gate.

## [0.2.9] - 2026-08-13

### Added
- **Interactive File Mentions in Chat**: Converted `@file` path mentions (e.g. `@docs/CHANGELOG.md`, `@src/main.rs:123`) into clickable markdown links displaying the filename basename and showing full paths on hover.

### Fixed
- **Worktree Release Remote Synchronization**: Fixed release workflow to push `HEAD:main` and release tags directly to origin, ensuring worktree release commits are published to remote GitHub `main` branches immediately.

## [0.2.8] - 2026-08-13

### Added
- **Git Graph Overflow Ref Tooltips**: Render hidden overflow git branch and tag refs (`+N`) as a vertical column of distinct badges styled by ref kind (HEAD, Local, Remote, Tag).
- **Streaming Draft Preview Line Cap**: Constrain streaming draft preview text in chat pane to 5 lines (~100px max height) with vertical scrolling to prevent viewport expansion during long generations.

### Fixed
- **Stats Tab Scroll Anchoring**: Isolated `Stats` tab scrolling to a dedicated scroll handle initialized at top (offset 0) and removed errant background auto-scroll triggers in reload ticks and chat postings.

## [0.2.7] - 2026-08-13

### Added
- **Automated Peer Review & Quorum Gate (`/review`)**: Added `ReviewGate` enforcing peer approvals before merge gate execution, and registered `/review` slash command in Chamber and Gluon router.
- **Adversarial Cross-Examination Lane**: Added `CrossExaminationLane` with structured critic prompt synthesis and change triage rules.
- **Dynamic Smart Nucleus Prompt Injector**: Added `DynamicNucleusInjector` in `hadron-lattice` for relevance-ranked prompt memory injection within configured token budgets.
- **Dynamic Preon Synthesis**: Added on-the-fly markdown specialist persona generator in `hadron-gluon` for domain-tailored ad-hoc tasks.
- **Isolated `sccache` Builds & Worktree Pool**: Added per-worktree target directory configuration with shared `sccache` compiler cache, and thread-safe `WorktreePool` recycling in `hadron-gluon`.
- **AST-Aware Semantic Merge & Test Impact Analysis**: Added 3-way `merge_rust_ast` and test impact analysis (`compute_impacted_tests`) in `hadron-forge`.
- **Live Swarm Topology DAG & Chamber Time-Travel**: Added GPUI live swarm topology visualizer, `/replay` and `/fork-field` time-travel commands, interactive 3-way AST conflict resolver, and live token gauge.
- **Embedded Semantic Code & Memory Graph**: Added in-memory `SemanticGraphIndex` in `hadron-lattice` and `hadron_forge_semantic_search` MCP tool in `hadron-forge-mcp`.

### Changed
- **README & Provider Ecosystem Overhaul**: Reorganized provider docs around transports (ACP, HTTP, Local/Cloud) with updated architecture diagram and 400+ OpenRouter model support.

### Fixed
- **Git Push Stderr Output Handling**: Fixed stderr capture and empty output handling in Chamber `/push` command.

## [0.2.6] - 2026-08-13

### Added
- **Built-in `/release` Command and Skill**: Added `/release` slash command and builtin `release` skill backing project release automation via `.hadron/nucleus/release.md`.
- **Operational Git Slash Commands**: Added `/git-status`, `/git-log`, `/push`, and `/pr` slash commands for fast terminal Git workflow.
- **Configurable Merge Strategy Selector**: Added FastForward, Squash, and GitHub PR merge strategy options in Settings UI and Gluon merge gate.
- **GitHub Topics Integration**: Configured repository search topics on `s0lda/hadron` (`ai`, `llm`, `ai-agents`, `multi-agent`, `developer-tools`, `rust`, `gpui`, `mcp`, `acp`, etc.).

### Fixed
- **Nucleus Feature Map Prompt Evaluation**: Explicitly checked `features.md` filesystem existence before prompt digest inclusion.
- **VCS Spec & Plan Clean Up**: Untracked internal `.hadron/docs` files from git repository index.

## [0.2.5] - 2026-08-13

### Added
- **File Tree Preview Syntax Highlighting**: Added tree-sitter language detection and syntax highlighting for file previews (`.rs`, `.cpp`, `.css`, `.json`, `.ts`, `.py`, `.sh`, etc.).
- **Unified Skills Toggle in Settings**: Combined skills selection and denied skills in Settings; skills are enabled by default and unselecting a skill toggles it on the deny list.
- **Scrollable Sessions Menu & History Cleanup**: Made the Sessions menu scrollable and added `/clear-history` command to clear archived session logs while preserving telemetry token spend.

### Changed
- **Event Log Single Line Collapse**: Collapsed Event Log rows to a clean single line with markdown header stripping, expanding on click for detailed output.
- **Git Inspector UI Polish**: Removed redundant Git subtab headers, indented expanded commit diff panels with continuous rail graph canvas rendering, and added symmetric padding in Branches and Worktrees views.
- **Roster & Settings Header Simplification**: Removed raw file path and branch labels from the Roster header and removed misleading green "Ready" status dots from configured providers.

## [0.2.4] - 2026-08-12

### Added
- **New Operational Slash Commands**: Added 10 operational slash commands (`/retry`, `/doctor`, `/prune`, `/compact-nucleus`, `/stop`, `/kill`, `/cancel`, `/gate-cancel`, `/revert`, `/unabandon`).

### Changed
- **Terminal Background Styling**: Applied terminal background (`#080808` / `theme::term_bg()`) across Roster Quark Cards, Live View Cards, and Autocomplete Overlays with identity color highlights bound to Quark titles and selected borders.
- **Obsidian Theme & Palette Refinements**: Refined surface and card tokens to neutral dark obsidian palette (`#050505`, `#141414`, `#101113`, `#fcfcfc`), aligned panel backgrounds across Roster, Chat, and Inspector panels, and updated Settings input contrast.
- **Capsule Tabs & Navigation Styling**: Standardized capsule tabs alignment across all view panels, styled terminal sub-tabs with rounded borders, and applied borderless dark tab bar background (`tab_bar_bg`).

### Fixed
- **Unit Test Execution Safety**: Isolated `test_revert_and_unabandon` inside `tempdir` repository to prevent `git revert` from running against the live working tree during `cargo test`.

## [0.2.3] - 2026-08-11

### Added
- **Unified Skills Management in Settings**: Replaced legacy Preons in Settings with a unified Skills Manager featuring collapsible accordion for all 15 standard swarm skills, custom skill loading (`.hadron/skills` and `~/.hadron/skills`), code editor integration, and side-by-side Repo/Global skill creation.
- **Slash Command Integration for Skills**: Integrated all built-in and custom skills into `/commands` slash autocomplete (`/<skill-id>`) and auto-formatted `name:` front-matter on `/add-skill`.

### Fixed
- **Antigravity SDK Trajectory Recovery**: Handled empty prompt payloads and reset agent state on ACP turn errors to prevent `400: Requests ending with a model turn are not supported` failures.
- **Worktree Invariant Paths**: Updated `using-git-worktrees` skill procedure to prioritize `.hadron/trees/` and `.hadron/worktrees/` directory structures.

## [0.2.2] - 2026-08-11

### Added
- **Dynamic Model Selection & Live SDK Discovery**: Live SDK model listing via `google.genai.Client.models.list()` in Antigravity ACP Python bridge, and CLI model probing via `CliSpec` `model_probe` command.
- **Capability-Gated Advanced Model Parameters**: Capability detection (`supports_model_params()`) to hide temperature/top_p/max_tokens inputs when unsupported by a Quark, grouped inside a collapsible accordion in Settings.
- **Expanded Replace-by-Hash Language Support**: AST block parsing for 11 languages (Rust, Python, TS/TSX, Go, C, C++, Java, C#, JS, Ruby, PHP, HTML, CSS, SQL) and universal blank-line structural fallback chunking for opaque text files (`Lang::Opaque`).

### Fixed
- **ACP Agent Model Resolution & Fallback**: Removed non-existent models (`gemini-3.6-pro`) from ACP bridge and added auto-healing fallback to `gemini-3.6-flash` on unrecognized model strings.

## [0.2.1] - 2026-08-09

### Fixed
- **Antigravity Python Bridge & Windows Python Launcher**: Fixed Python launcher (`py.exe`) resolution on Windows, filtered out Microsoft Store execution alias stubs, added `--clear` flag for idempotent `venv` creation, and expanded bridge error output (`stdout`/`stderr`).

## [0.2.0] - 2026-08-08

### Fixed
- **Windows PTY Terminal & Execution Engine**: Fixed Windows MSVC winres duplicate icon resource linking (`CVT1100`), restored interactive shell execution (PowerShell/CMD), removed verbose stream repaints, and fixed PTY double prompt on startup.

## [0.1.9] - 2026-08-07

### Fixed
- **Windows ConPTY & Taskbar Icon**: Enforced 80x24 initial PTY grid, preserved SlavePty handle lifetime, and added Win32 WM_SETICON window enumeration for Windows taskbar.

## [0.1.8] - 2026-08-02

### Added
- **Worker Prompt Orchestration Tag**: Added `@orchestrator` tag to worker response format template to ensure clear worker-to-orchestrator communication across the swarm.

### Fixed
- **Prompt Cache Prefix Stability & Skill Distillation**: Standardized worker response output format and prompt structure for cache efficiency and distilled remaining workflow skills.

## [0.1.7] - 2026-08-02

### Added
- **Lexical Nucleus Recall**: Recalls relevant lessons from `notes/*.md` using BM25 lexical ranking based on assignment text (`Assign.task`), pinning `## How we get things wrong` first and capping injected output within a 1/8 budget ceiling while preserving prompt cache prefix stability.

### Fixed
- **Partial Text Preservation on Turn Cancel**: Preserves partial text streaming output on graceful ACP session cancellation instead of discarding it.
- **Git Graph Remote Branch Names**: Preserves remote branch names like `origin/main` in git graph ref pills.

## [0.1.6] - 2026-08-02

### Added
- **Bundled Fonts**: Inter (UI) and Cascadia Code (mono) ship inside the binary and are registered at startup, so bold weights and the terminal grid render the same on a machine with no fonts installed.
- **Forge Tool Loop for HTTP Quarks**: Ollama, LM Studio and OpenAI-compatible seats get a real, bounded, streaming tool surface (read/list/grep/blocks/edit/create/git_diff/exec) jailed to the turn's worktree, instead of narrating tool calls as prose.
- **Per-Seat Model Parameters**: `temperature`, `top_p` and `max_tokens` are settable per quark in Settings and persist in `team.json`; every field is optional, and absent means "let the vendor decide".
- **Delegation View**: a new Git-rail subtab showing who asked whom to do what — resolved from both explicit addressing and line-start `@mention` fan-outs — rendered with each quark's display name and identity colour.

### Changed
- **Tasks Tab**: restyled as glass cards matching the Delegation view, keeping the live elapsed clock, the asked-at timestamp and the four task states.
- **Terminal Cursor**: drawn as a line beam on the left edge of the cell instead of a colour-inverted block, so the character under the cursor stays readable.

### Fixed
- **An HTTP Quark's Tools Now Obey the Permission Mode**: `edit_block`, `create_file` and `exec` were reachable from any mode, including `Ask` ("talk, don't act"). A turn now declares only what its mode permits, with a runtime backstop for a call the model was never offered, and an `Auto` turn is told that `exec` is a jailed `cargo`/`git` allowlist rather than the ungated shell the guidance refuses.
- **A Message Arriving Mid-Turn Adds Work**: interrupting a quark used to replace its task; the interrupted task is now carried into the next dispatch instead of dropped.
- **A Dirty Worktree No Longer Wedges the Swarm**: uncommitted work from a previous assignment is snapshotted onto its own branch before the next branch is cut, instead of refusing the turn.
- **Gluon's Own Notices Quote Paths and Refs**: bare paths and branch names in daemon messages are wrapped in backticks so they no longer render as commands; `@mention` routing targets are left alone.
- **Ollama Tool Arguments**: an echoed tool call's `arguments` is sent as an object, not a string, which Ollama rejected with a 400 for the whole request.
- **Reasoning Streams**: an empty `content` field no longer swallows the entire reasoning phase of an OpenAI-SSE stream.
- **Absolute Paths in Tool Arguments**: a path inside the worktree written absolutely is treated as a spelling, not a jail escape.
- **Ghost Roster Rows**: the mock quark is deleted, and a `team.json` that seats nobody is no longer answered with mock quarks.

## [0.1.5] - 2026-08-02

### Fixed
- **In-App Changelog Said "Unreleased"**: the Changelog overlay carried its own hand-written copy of the release list, so 0.1.4 shipped to users still wearing the "Unreleased" badge and missing four entries. The overlay's list is now a single `RELEASES` table whose newest entry is checked against `docs/CHANGELOG.md` and `CARGO_PKG_VERSION` by a test, so a release that forgets it fails the gate.

## [0.1.4] - 2026-08-02

### Added
- **HTTP Quark Adapters**: Support for `Ollama`, `LM Studio`, and OpenAI-compatible Cloud providers (`OpenRouter`, `Groq`, `DeepSeek`, `Together`) via `hadron_lattice::Transport::Http`.
- **Add-Quark Wizard HTTP Rows**: Connect and configure local Ollama, LM Studio, and Cloud OpenAI endpoints directly in the wizard with keyring-secured API keys.
- **Searchable Model Selection**: Unified searchable model list with pinned "Default" row across both the Add-Quark wizard and HTTP-seat Settings panel.
- **CLI Seat Streaming**: `CliQuark` streams end to end and publishes `agy`'s step/tool feed, so the Live card moves during a CLI turn and names the file or command each step is working on.
- **Gitignored Files in `@` Mentions**: The `@file` completion offers gitignored paths, with an icon and path detail on each row.

### Fixed
- **HTTPS TLS Connectivity**: Enabled `rustls-tls` feature on `reqwest` so cloud endpoints (`OpenRouter` etc.) connect over HTTPS without scheme errors.
- **LM Studio `/v1` Endpoint Handling**: Fixed LM Studio `/v1` endpoint resolution and prevented empty/error response bodies from crashing model discovery.
- **Worker Roster Action**: Restored "Make Orchestrator" context menu item for worker quarks in the Roster rail.
- **See-Through Overlays**: The model dropdown and the Settings modal now paint on opaque surfaces instead of a translucent card.
- **HTTP Seat Live Card**: Ollama and OpenRouter seats are wired to `live_dir`, so their activity shows in the Live card.
- **Chat Input Auto-Scroll**: Auto-scroll chat input on paste of large text blocks.

## [0.1.3] - 2026-07-27

### Added
- **Unified Swarm Command Deck**: Full Hadron UI redesign featuring obsidian dark theme, floating capsule tab bars, and refined overlays.
- **`/Command` Picker Chip**: Quick slash command chip selection alongside `@Quark` and `@File` mentions.
- **Task Time Scrubbing & Gate Heartbeats**: Real-time merge-gate heartbeat monitoring and historical instant scrubbing in the Tasks tab.
- **Live Streamed Replies**: Streaming agent replies directly in chat while tool activity publishes to the Live card.

### Changed
- **Theme Polish**: Obsidian Graphite theme, soft amethyst accents, and metallic pastel git graph palette.
- **Catalogue Seating**: Improved bootable agent detection and catalogue error handling.

## [0.1.2] - 2026-07-26

### Added
- **Turn Interruption & Cancellation**: Resident ACP sessions support graceful turn cancellation and mid-turn interruption.
- **Chamber Auto-Restart**: Chamber automatically stops daemon and relaunches on new binary build after successful self-update.
- **Orchestrator Chat Lane**: Independent chat lane preserved across reseats for active orchestrators.
- **Silence-Based Turn Watchdog**: Replaced wall-clock turn deadline with silence-based activity watchdog (`TURN_DEADLINE`).

### Fixed
- **Worktree Preservation**: Interrupted turns snapshot worktrees instead of stranding uncommitted edits.
- **CLI Uninterruptible Notices**: Prevented background notices from prematurely waking unaddressed seats.

## [0.1.1] - 2026-07-25

### Fixed
- **Self-Update Tag Pinning**: Fixed update workflow to install the exact released tag offered rather than `main`'s latest HEAD.

## [0.1.0] - 2026-07-24

### Added
- **Hadron Swarm Architecture**: Multi-quark swarm orchestration daemon (`hadron-gluon`), IPC data layer (`hadron-lattice`), and ACP adapter engine.
- **Chamber GUI**: Native GPUI desktop application for managing swarms, PTY terminals, git branches, tasks, and telemetry.
