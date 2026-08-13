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

## 🔌 Provider & Model Ecosystem

Hadron connects AI agents, local models, cloud APIs, and terminal workflows into a single orchestrated chat workspace. Any agent, model, or tool speaking **ACP (Agent Client Protocol)**, **OpenAI-compatible HTTP**, or **Stdio/PTY** can be seated directly into your swarm.

### 🤖 ACP Agents & Custom Bridges
Hadron boots and orchestrates resident agents with automatic session resumption, context tracking, and tool calls:
- **ACP Agents**: Launch any Agent Client Protocol (ACP) compliant agent via built-in presets or the dynamic ACP registry (e.g. `Claude Code`, `Codex CLI`, `Gemini CLI`, `GitHub Copilot`, `Goose`, `Cursor`, `Cline`, `OpenHands`, `OpenCode`, and custom binaries).
- **Custom Bridges**: Dedicated adapters for SDK-based agents, such as **Google Antigravity** via Hadron's embedded Python bridge.

### 🏠 Local Models (Offline & Private)
Connect local HTTP model servers with zero-latency streaming:
- **Ollama**: Auto-detects local models running over `localhost:11434`.
- **LM Studio & Local Proxies**: Connect local OpenAI-compatible endpoints (`localhost:1234/v1`).

### ☁️ Cloud HTTP & OpenAI-Compatible Endpoints
Connect to any cloud model provider or API proxy via `Authorization: Bearer` keys:
- **OpenAI & OpenRouter**: Native support for OpenAI endpoints and OpenRouter (accessing 400+ models across providers).
- **OpenAI-Compatible Providers**: Support for any cloud provider or self-hosted server (e.g. Groq, Together AI, Fireworks AI, DeepSeek API, Mistral API, SambaNova, vLLM, TGI) accepting standard OpenAI chat completions format.

### 💻 Custom Coding CLIs & PTY Transports
- **CLI Transport (`CliSpec`)**: Data-driven stdio wrapper for one-shot or interactive CLI coding tools (`aider`, custom scripts, dockerized agents).
- **PTY Terminal Seats**: Interactive bash/zsh PTY terminals seated right alongside LLM agents in the chat room.

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
