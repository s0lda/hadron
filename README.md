# 🌌 Hadron

<div align="center">

**A Native Rust Multi-Agent Operating System & Swarm Workspace**

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/Language-Rust_2021-orange.svg)](https://www.rust-lang.org/)
[![UI](https://img.shields.io/badge/GUI-GPUI-purple.svg)](https://zed.dev)
[![Protocol](https://img.shields.io/badge/Protocol-Agent_Client_Protocol_%28ACP%29-green.svg)](https://agentclientprotocol.com)
[![Architecture](https://img.shields.io/badge/Architecture-Decoupled_Zero--CPU_Bus-red.svg)](docs/architecture/decoupled-architecture.md)

  <br />

  <img src="assets/hadron_full_size.jpg" alt="Hadron Architecture Banner" width="900" />

</div>

---

## ⚡ What is Hadron?

**Hadron** is a native, GPU-accelerated multi-agent execution environment built in Rust. It orchestrates autonomous AI agent swarms—called **Quarks**—over a zero-CPU filesystem event bus (`field.jsonl`) and local SQLite ledgers.

A seat takes any agent that speaks the **[Agent Client Protocol](https://agentclientprotocol.com)**, any coding **CLI** (through our resident CLI transport), or **[Antigravity](https://antigravity.google/)** — over its CLI _and_ over our SDK bridge. Hadron provides parallel execution, isolated git worktree branching, and race-free AST code edits via **Hadron Forge (Edit-by-Hash)**.

### What Makes It Different
- **A Swarm, Not a Chat**: Autonomous agents take turns on a shared, append-only event bus.
- **Nothing Lands Unreviewed**: Turns run in isolated git worktrees; the Merge Gate rebases onto `main` and runs your test suite before merging.
- **Native & Fast**: Built with Rust + GPUI desktop app featuring interactive PTY terminals, live git inspection, and per-quark token telemetry.
- **Vendor-Neutral**: ACP, CLI, or SDK bridge — swap agents seamlessly behind any seat.

---

## ⚡ Key Superpowers
* **🚀 Zero-CPU File Bus**: OS-level kernel file notifications (`notify`) wake waiting daemons with 0% CPU idle overhead.
* **🛠️ Hadron Forge**: AST-level compare-and-swap code edits using `blake3` cryptographic hashes.
* **🧠 Context7 & Superpowers**: Real-time documentation lookups and 15 bundled workflow skills (brainstorming, TDD, systematic debugging).
* **🛡️ Git Worktree & Merge Gate**: Concurrent branch isolation with automatic test verification before fast-forwarding clean code.

---

## 🚀 Quick Start

### Prerequisites
* **Rust**: Toolchain (edition 2021, tested on 1.96.0+).
* **OS**: Linux (X11/Wayland), macOS, or Windows via **WSL2**.
* **Agents**: Any ACP agent, coding CLI, or Antigravity SDK bridge.

### Installation
```bash
# Install binary suite (--locked is mandatory)
cargo install --locked --git https://github.com/s0lda/hadron.git hadron
```

### Run It Anywhere
```bash
# Open or create a swarm in the current working directory
cd ~/dev/my_project && hadron
```

---

## 📚 Documentation & Deep Dives

For technical breakdowns of Hadron's physics mental model, architecture, and developer guides, explore the [`/docs`](docs/) directory:

| Section | Description | Links |
| :--- | :--- | :--- |
| 🏛️ **Architecture** | 2-Tier Daemon/GUI split, Swarm Event Loop, & System Diagrams | [`Decoupled Architecture`](docs/architecture/decoupled-architecture.md) · [`Swarm Event Loop`](docs/architecture/swarm-event-loop.md) · [`System Diagrams`](docs/architecture/system-diagrams.md) |
| ⚛️ **Concepts & Physics** | Particle Physics Mental Model & Workspace Glossary | [`Physics Metaphor`](docs/concepts/physics-metaphor.md) · [`Vocabulary & Glossary`](docs/concepts/vocabulary.md) |
| 🛠️ **Hadron Forge** | AST Edit-by-Hash Precision & Stdio MCP Server | [`Edit-by-Hash Engine`](docs/forge/edit-by-hash.md) |
| 🌿 **Development** | Building from source & Git Worktree Merge Gate | [`Building from Source`](docs/development/building-from-source.md) · [`Git Worktree & Merge Gate`](docs/development/git-worktree-usage.md) |

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+Tab` / `Ctrl+\`` | Toggle focus between Chat Input and PTY Terminal |
| `Alt+Left` / `Alt+Right` | Switch Chat column tabs |
| `Alt+PageUp` / `Alt+PageDown` | Switch Right-Rail Inspector tabs |
| `Alt+Up` / `Alt+Down` | Switch Telemetry & Stats time windows |
| `F6` | Cycle global permission mode (Ask / Write / Auto / Bypass) |

---

## 🛠️ Built With

* **Core Frameworks**: [Rust](https://www.rust-lang.org/), [GPUI](https://zed.dev), [gpui-component](https://github.com/longbridge/gpui-component), [ACP](https://agentclientprotocol.com), [Tree-Sitter](https://github.com/tree-sitter/tree-sitter).
* **Tooling**: [Context7](https://context7.com), [Superpowers Skill Library](https://github.com/obra/superpowers), Hadron Forge MCP.
* **Transports**: ACP, CLI, and Antigravity SDK bridge.

---

## 📄 License & Community

* **License**: [Apache License 2.0](LICENSE)
* **Contributing**: [CONTRIBUTING.md](.github/CONTRIBUTING.md)
* **Security**: [SECURITY.md](.github/SECURITY.md)
* **Code of Conduct**: [CODE_OF_CONDUCT.md](.github/CODE_OF_CONDUCT.md)
* **Glossary**: [Physics Metaphor & Vocabulary](docs/concepts/vocabulary.md)
