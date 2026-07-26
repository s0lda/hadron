# 🌌 Hadron

<div align="center">

  **Hyper-Fast, 120 FPS Native Rust Multi-Agent Operating System & Swarm Workspace**

  [![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
  [![Language](https://img.shields.io/badge/Language-Rust_1.80+-orange.svg)](https://www.rust-lang.org/)
  [![UI](https://img.shields.io/badge/GUI-GPUI_120_FPS-purple.svg)](https://zed.dev)
  [![Protocol](https://img.shields.io/badge/Protocol-Agent_Client_Protocol_(ACP)-green.svg)](https://agentclientprotocol.com)
  [![Architecture](https://img.shields.io/badge/Architecture-Decoupled_Zero--CPU_Bus-red.svg)](#-the-architecture)

  <br />

  <img src="assets/hadron_full_size.jpg" alt="Hadron Architecture Banner" width="900" />

</div>

---

## ⚡ What is Hadron?

**Hadron** is a native, GPU-accelerated multi-agent execution environment built in Rust. It orchestrates autonomous AI agent swarms—called **Quarks**—over a zero-CPU filesystem event bus (`field.jsonl`) and local SQLite ledgers.

Whether running Claude, Antigravity (Gemini), or OpenAI models via the **Agent Client Protocol (ACP)** or native CLI seats, Hadron enables parallel model execution, isolated git worktree branching, and race-free AST code edits via **Hadron Forge (AST Edit-by-Hash)**.

---

## 🔭 How It Works (What & How We Do It)

### 1. The Decoupled 2-Tier Architecture
Hadron separates execution from presentation:
- **`hadron-gluon` (Headless Daemon)**: Watches the NDJSON file bus, dispatches turns, manages worktrees, enforces gatekeeper security policies, and executes merge gates.
- **`hadron-chamber` (120 FPS Native Visualizer)**: Powered by Zed's **GPUI** framework, providing a hardware-accelerated, sub-millisecond responsive GUI with chat, PTY terminals, telemetry charts, and live git inspection.

### 2. The Zero-CPU File Bus & Swarm Event Loop
Agents communicate by appending NDJSON events to `field.jsonl`. File watchers (`notify`) wake waiting components with zero CPU polling overhead:

```
[Human / UI] ──> field.jsonl (Event) ──> hadron-gluon (Daemon)
                                                │
                                  ┌─────────────┴─────────────┐
                                  ▼                           ▼
                           Quark A (Claude)            Quark B (Gemini)
                                  │                           │
                                  └─────────────┬─────────────┘
                                                ▼
                                    AST Edit-by-Hash / Worktree
                                                │
                                                ▼
                                     Merge Gate & Rebase (main)
```

### 3. Hadron Forge (AST Edit-by-Hash Precision)
Instead of fragile line diffs or full file rewrites, **Hadron Forge** parses source files (`Rust`, `Python`, `TypeScript`, `Go`) into AST item blocks tagged with cryptographic `blake3` hashes. Agents perform Compare-And-Swap (CAS) block modifications—preventing race conditions and preserving token budgets.

### 4. Git Worktree Isolation & Merge Gate
Every quark turn executes inside an isolated git worktree (`.hadron/trees/<id>`). When work completes, the **Merge Gate** automatically rebases the branch onto `main`, executes the host project's native test suite within a strict deadline, and fast-forwards clean passing code.

---

## 🏛️ System Architecture

```
hadron/
├── Cargo.toml
├── crates/
│   ├── hadron-lattice/     (Shared Protocol: Structs, Intents, & Edit-by-Hash schemas)
│   ├── hadron-gluon/       (Headless Daemon: File watcher, turn router, Git runner)
│   ├── hadron-chamber/     (GPUI Glass: 120 FPS Native Visualizer & PTY Terminal)
│   ├── hadron-forge/       (Edit-by-Hash Engine: AST block parsing & blake3 hashing)
│   ├── hadron-forge-mcp/   (Stdio MCP Server: Tool protocol adapters for ACP agents)
│   ├── hadron-gatekeeper/  (Security Engine: Verification, permissions, & policy checks)
│   └── gpui-component/     (UI Component Library: Forked GPUI widgets & native styling)
```

<div align="center">
    <img src="assets/demo.png" alt="Hadron Chamber UI Demo" width="900" />
</div>

---

## ⚛️ The Hadron Physics Metaphor & Vocabulary

Hadron uses particle physics as a cohesive mental model for multi-agent operating system concepts.

| Term | Meaning in Hadron | Physics Metaphor |
| :--- | :--- | :--- |
| **Hadron** | The complete multi-agent operating environment | A composite particle that binds quarks together |
| **Quark** | An active agent seat in the swarm (e.g., Claude, Antigravity) | The fundamental particle of intelligence |
| **Preon** | Addressable markdown instructions loaded into a quark | Proposed substructure inside a quark |
| **Field** | The shared append-only NDJSON event bus (`field.jsonl`) | Interactive quantum field |
| **Event** | One structured record in the field bus | A detected particle interaction |
| **Gluon** | The headless daemon (`hadron-gluon`) routing tasks | The gauge boson/force carrier binding quarks |
| **Lattice** | The shared protocol data layer (`hadron-lattice`) | The underlying space-time lattice |
| **Chamber** | The 120 FPS GPUI desktop workspace (`hadron-chamber`) | A cloud/bubble chamber for observing particle tracks |
| **Nucleus** | Shared persistent single-source-of-truth knowledge base | The dense, stable core |
| **Flavor** | A quark's role in the swarm (Orchestrator, Worker) | Quark flavors (up, down, charm...) |
| **Energy & Ledger** | Real-time token spend tracking and SQLite quotas | Conservation of energy & energy states |
| **Standard Model** | Non-negotiable base system invariants (`standard_model.md`) | Fundamental laws of physics |

---

## 🚀 Getting Started

### Prerequisites
- **Rust Toolchain**: 1.80 or higher (`rustup update`)
- **System Dependencies**: Linux (X11/Wayland/WSL2) or macOS

### Building from Source

```bash
# Clone the repository
git clone https://github.com/your-username/hadron.git
cd hadron

# Run the full workspace test suite
cargo test --workspace

# Launch Hadron Chamber GUI
cargo run -p hadron-chamber
```

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+Tab` / `Ctrl+\`` | Toggle focus between Chat Input and PTY Terminal |
| `Alt+Left` / `Alt+Right` | Switch Chat column tabs |
| `Alt+PageUp` / `Alt+PageDown` | Switch Right-Rail Inspector tabs |
| `Alt+Up` / `Alt+Down` | Switch Telemetry & Stats time windows |
| `F6` | Cycle global permission security modes (Ask / Auto / Bypass) |

---

## 🛠️ Built With

Hadron stands on the shoulders of giants. Everything below is a real dependency of this repository:

### Core Frameworks
- **[Rust](https://www.rust-lang.org/)** — Safe, concurrent systems programming language.
- **[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)** by **[Zed Industries](https://zed.dev)** (Apache-2.0) — GPU-accelerated UI framework.
- **[gpui-component](https://github.com/longbridge/gpui-component)** by **[Longbridge](https://longbridge.com)** (Apache-2.0) — High-performance native desktop widgets.
- **[Agent Client Protocol (ACP)](https://agentclientprotocol.com)** by **[Zed](https://zed.dev)** — Standardized protocol for long-running resident agents.
- **[Tree-Sitter](https://github.com/tree-sitter/tree-sitter)** — Incremental AST parsing for syntax highlighting and forge block extraction.

### Supported Models & Transports
- **[Claude](https://www.anthropic.com/claude)** (Anthropic) via ACP and Claude CLI.
- **[Antigravity / Gemini](https://antigravity.google/)** (Google) via `agy` CLI transport.

### Ecosystem Crates
- [`tokio`](https://tokio.rs) (async runtime)
- [`serde`](https://serde.rs) / `serde_json` (NDJSON wire format)
- [`rusqlite`](https://github.com/rusqlite/rusqlite) + [SQLite](https://sqlite.org) (energy ledger)
- [`notify`](https://github.com/notify-rs/notify) (filesystem watching bus)
- [`ulid`](https://github.com/dylanhart/ulid-rs) (sortable IDs for events & turns)
- [`chrono`](https://github.com/chronotope/chrono) · [`anyhow`](https://github.com/dtolnay/anyhow) · [`futures`](https://github.com/rust-lang/futures-rs) · [`markdown`](https://github.com/wooorm/markdown-rs) · [`blake3`](https://github.com/BLAKE3-team/BLAKE3)

---

## 📄 Licence & Community

- **License**: Licensed under the **[Apache License 2.0](LICENSE)**.
- **Contributing**: Read **[CONTRIBUTING.md](CONTRIBUTING.md)** for build instructions, developer test gates, and Standard Model invariants.
- **Security**: Read **[SECURITY.md](SECURITY.md)** for our security disclosures and permission sandbox policies.
- **Code of Conduct**: Read **[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)** (Contributor Covenant 2.1).

