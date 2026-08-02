# 🏛️ System Architecture & Workspace Layout

Hadron is organized as a Cargo workspace with modular crates separating protocol definition, background daemon execution, desktop GUI presentation, AST editing, and security verification.

---

## Workspace Layout

```text
hadron/
├── Cargo.toml
├── crates/
│   ├── hadron-lattice/     (Shared Protocol: Structs, Intents, & Edit-by-Hash schemas)
│   ├── hadron-gluon/       (Headless Daemon: File watcher, turn router, Git runner)
│   ├── hadron-chamber/     (package `hadron` — GPUI Glass, PTY Terminal, and all
│   │                        three bin targets: hadron, hadron-gluon, hadron-forge-mcp)
│   ├── hadron-forge/       (Edit-by-Hash Engine: AST block parsing & blake3 hashing)
│   ├── hadron-forge-mcp/   (Stdio MCP Server: Tool protocol adapters for ACP agents)
│   └── hadron-gatekeeper/  (Security Engine: Verification, permissions, & policy checks)
```

---

## Crate Responsibilities

- **`hadron-lattice`**: The foundational protocol crate containing shared data structures, event models, NDJSON wire protocols, and Edit-by-Hash schemas.
- **`hadron-gluon`**: The background daemon engine that manages process execution, git worktrees, turn routing, skills injection, and event bus watching. Its entrypoint is `hadron_gluon::cli::run`.
- **`hadron-chamber`**: The primary workspace crate (`package hadron`) containing the GPUI desktop interface, PTY terminal components, telemetry graphs, and binary target entrypoints (`hadron`, `hadron-gluon`, `hadron-forge-mcp`).
- **`hadron-forge`**: The AST parser and hash-indexing engine. It parses source files into AST item blocks and computes `blake3` hashes for precise edits.
- **`hadron-forge-mcp`**: The stdio Model Context Protocol (MCP) server that exposes Hadron Forge tools to ACP agents. Its entrypoint is `hadron_forge_mcp::run`.
- **`hadron-gatekeeper`**: Security policy enforcement engine that manages global permission states (Ask / Write / Auto / Bypass) and path sandbox limits.

---

## External Dependencies & GUI Components

- **`gpui-component`**: The desktop widget library by Longbridge ([longbridge/gpui-component](https://github.com/longbridge/gpui-component)). Hadron uses a targeted fork ([s0lda/gpui-component](https://github.com/s0lda/gpui-component)), consumed via the `[patch]` section in `Cargo.toml` to support custom text mark styling for @mentions.

---

## Visualizer Interface

<div style="display: flex; flex-direction: row; justify-content: center; align-items: center; gap: 16px;">
    <img src="../../assets/demo.png" alt="Hadron Chamber UI Demo" width="450" style="max-width: 48%; height: auto;" />
    <img src="../../assets/demo_2.png" alt="Hadron Chamber UI Demo" width="450" style="max-width: 48%; height: auto;" />
</div>
