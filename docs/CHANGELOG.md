# Changelog

All notable changes to Hadron will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
