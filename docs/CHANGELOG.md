# Changelog

All notable changes to Hadron will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
