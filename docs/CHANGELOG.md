# Changelog

All notable changes to Hadron will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
