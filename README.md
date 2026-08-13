# 🌌 Hadron

<div align="center">

**The Native Desktop Workspace for Multi-Provider LLM Swarms**

*Orchestrate Claude, Antigravity, OpenAI, Ollama, and CLI Agents in One Workspace*

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/Language-Rust_2021-orange.svg)](https://www.rust-lang.org/)
[![UI](https://img.shields.io/badge/GUI-GPUI-purple.svg)](https://zed.dev)
[![Protocol](https://img.shields.io/badge/Protocol-Agent_Client_Protocol_%28ACP%29-green.svg)](https://agentclientprotocol.com)
[![Architecture](https://img.shields.io/badge/Architecture-Decoupled_Zero--CPU_Bus-red.svg)](docs/architecture/decoupled-architecture.md)

  <br />

  <img src="assets/demo_2.png" alt="Hadron Orchestrated Multi-Provider Chat Workspace" width="900" />

</div>

---

## ⚡ What is Hadron?

**Hadron** is a native, GPU-accelerated desktop workspace built in Rust for **orchestrated multi-provider LLM chat**. It brings models from **Anthropic (Claude)**, **Google Antigravity**, **OpenAI**, **Ollama**, **OpenRouter**, and custom **coding CLIs** into a unified room where agents collaborate, exchange context, and delegate tasks using `@mentions`.

Behind the interface, agents (called **Quarks**) execute concurrently over a zero-CPU filesystem event bus (`field.jsonl`), work safely in isolated git worktrees, and submit code changes through an automated **Merge Gate** that runs tests before landing anything on `main`.

---

## 💡 Key Superpowers

- 💬 **Collaborative Multi-LLM Chat**: Mix models in a single thread — `@claude`, `@ollama`, `@agy`, and `@openai` see each other's output, share context, and execute parallel sub-tasks.
- 🛡️ **Git Worktrees & Automated Merge Gate**: Agents work in isolated git worktree branches. The Merge Gate automatically rebases onto `main` and runs your test suite before clean code lands.
- ⚡ **Native GPUI Desktop App**: Lightning-fast GPU-accelerated UI with interactive PTY terminals, live git visualizer, and real-time per-agent token telemetry.
- 🛠️ **Hadron Forge (Edit-by-Hash)**: AST-level precision code edits using `blake3` cryptographic hashes for zero-drift mutations.
- 🧠 **Context7 & Superpowers**: Integrated documentation search and 15 bundled workflow skills (TDD, systematic debugging, design specs).

---

## 🚀 Quick Start

### Prerequisites

- **Rust**: Toolchain (edition 2021, tested on Rust 1.80+).
- **OS**: Linux (X11/Wayland), macOS, or Windows (native MSVC or WSL2).
- **LLM Access**: Any ACP agent (e.g. Claude Code), Antigravity, local Ollama, or API key for OpenAI / OpenRouter / Anthropic.

### 1. Install Hadron

```bash
# Install the full binary suite (--locked is mandatory)
cargo install --locked --git https://github.com/s0lda/hadron.git hadron
```

### 2. Launch in Any Repo

```bash
# Open or create a swarm workspace in your project directory
cd ~/dev/my_project && hadron
```

---

## 🔌 Provider Ecosystem

Hadron seamlessly bridges different model architectures and transport protocols into one chat:

| Provider / Model | Transport Protocol | Capabilities |
| :--- | :--- | :--- |
| **Anthropic (Claude)** | ACP (`claude-code`) / HTTP API | Code editing, multi-file reasoning, sub-agent delegation |
| **Google Antigravity** | ACP / CLI Transport / SDK | Architecture design, multi-agent orchestration, web research |
| **Ollama** | Local HTTP (`localhost:11434`) | Offline execution, privacy-first local model swarms |
| **OpenAI & OpenRouter** | OpenAI Compatible HTTP | GPT-4o, Claude 3.5 Sonnet, DeepSeek, custom endpoints |
| **Custom Coding CLIs** | Resident PTY / Stdio Transport | Script execution, custom CLI coding wrappers |

---

## 📚 Documentation & Deep Dives

Explore Hadron's mechanics, architecture, and developer guides in the [`/docs`](docs/) directory:

| Section | Description | Links |
| :--- | :--- | :--- |
| 📜 **Changelog** | Release history & version notes | [`Changelog`](docs/CHANGELOG.md) |
| 🏛️ **Architecture** | 2-Tier Daemon/GUI split & Swarm Event Loop | [`Decoupled Architecture`](docs/architecture/decoupled-architecture.md) · [`System Diagrams`](docs/architecture/system-diagrams.md) |
| ⚛️ **Concepts & Physics** | Particle Physics Mental Model & Glossary | [`Physics Metaphor`](docs/concepts/physics-metaphor.md) · [`Vocabulary`](docs/concepts/vocabulary.md) |
| 🛠️ **Hadron Forge** | AST Edit-by-Hash Engine & MCP Server | [`Edit-by-Hash Engine`](docs/forge/edit-by-hash.md) |
| 🌿 **Development** | Building from Source & Git Worktrees | [`Building from Source`](docs/development/building-from-source.md) · [`Git Worktrees`](docs/development/git-worktree-usage.md) |

---

## ⌨️ Essential Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+Tab` / `Ctrl+\` | Toggle focus between Chat Input and PTY Terminal |
| `Alt+Left` / `Alt+Right` | Switch Chat column tabs |
| `Alt+PageUp` / `Alt+PageDown` | Switch Right-Rail Inspector tabs (Git / Files / Telemetry) |
| `Alt+Up` / `Alt+Down` | Cycle Telemetry & Stats time windows |
| `F6` | Cycle global permission mode (`Ask` / `Write` / `Auto` / `Bypass`) |

---

## 📄 License & Community

- **License**: [Apache License 2.0](LICENSE)
- **Contributing**: [CONTRIBUTING.md](.github/CONTRIBUTING.md)
- **Security**: [SECURITY.md](.github/SECURITY.md)
- **Code of Conduct**: [CODE_OF_CONDUCT.md](.github/CODE_OF_CONDUCT.md)
- **Glossary**: [Physics Metaphor & Vocabulary](docs/concepts/vocabulary.md)
