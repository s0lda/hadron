# 🌌 Hadron

<div align="center">

**A Native Rust Multi-Agent Operating System & Swarm Workspace**

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/Language-Rust_2021-orange.svg)](https://www.rust-lang.org/)
[![UI](https://img.shields.io/badge/GUI-GPUI-purple.svg)](https://zed.dev)
[![Protocol](https://img.shields.io/badge/Protocol-Agent_Client_Protocol_%28ACP%29-green.svg)](https://agentclientprotocol.com)
[![Architecture](https://img.shields.io/badge/Architecture-Decoupled_Zero--CPU_Bus-red.svg)](#-how-it-works-what--how-we-do-it)

  <br />

  <img src="assets/hadron_full_size.jpg" alt="Hadron Architecture Banner" width="900" />

</div>

---

## ⚡ What is Hadron?

**Hadron** is a native, GPU-accelerated multi-agent execution environment built in Rust. It orchestrates autonomous AI agent swarms—called **Quarks**—over a zero-CPU filesystem event bus (`field.jsonl`) and local SQLite ledgers.

A seat takes any agent that speaks the **[Agent Client Protocol](https://agentclientprotocol.com)**, any coding **CLI** (through our own resident CLI transport), or **[Antigravity](https://antigravity.google/)** — over its CLI _and_ over our SDK bridge. Hadron gives them parallel execution, isolated git worktree branching, and race-free AST code edits via **Hadron Forge (Edit-by-Hash)**.

**What makes it different**

- **A swarm, not a chat.** Several coding agents take turns on one shared, append-only
  field — an orchestrator splits the work, workers run in parallel, and every reply,
  status and token report is an event you can read back.
- **Nothing lands unreviewed.** Each turn runs in its own git worktree and branch; the
  merge gate rebases onto `main` and runs _your_ test suite before anything is merged.
- **Native, not Electron.** A Rust + GPUI desktop app with a real terminal, live git
  inspection and per-quark token telemetry.
- **Vendor-neutral.** ACP, CLI, or SDK bridge — swap the agent behind a seat without
  touching the swarm.

> **The vision.** We are building Hadron—a hyper-fast, natively compiled Rust multi-agent operating system. Hadron will securely orchestrate existing models (Claude, Llama, OpenAI) using a zero-CPU filesystem bus.

---

## 🔭 How It Works (What & How We Do It)

### 1. The Decoupled 2-Tier Architecture

Hadron separates execution from presentation:

- **`hadron-gluon` (Headless Daemon)**: Watches the NDJSON file bus, dispatches turns, manages worktrees, enforces gatekeeper security policies, and executes merge gates.
- **`hadron-chamber` (Native Visualizer)**: Powered by Zed's **GPUI** framework — a GPU-accelerated desktop GUI with chat, PTY terminals, telemetry charts, and live git inspection.

### 2. The Zero-CPU File Bus & Swarm Event Loop

Agents communicate by appending NDJSON events to `field.jsonl`. File watchers (`notify`) wake waiting components with zero CPU polling overhead:

```
  Human / Chamber UI
         │  appends one event
         ▼
   field.jsonl ───── notify, zero CPU ─────▶  hadron-gluon (daemon)
   append-only                                      │  routes the turn
   event bus   ◀──── every reply, status            ▼
                     and token report        Orchestrator quark
                     is another event               │  @mentions the work out
                              ┌─────────────────────┴─────────────────────┐
                              ▼                                           ▼
                        Worker quark                                Worker quark
                   own worktree + branch                       own worktree + branch
                              └─────────────────────┬─────────────────────┘
                                                    ▼
                                    Merge Gate — rebase onto main, run the
                                    project's own tests, fast-forward or refuse
```

### 3. Hadron Forge (AST Edit-by-Hash Precision) — for ACP seats

Instead of fragile line diffs or full file rewrites, **Hadron Forge** parses source files (`Rust`, `Python`, `TypeScript`, `Go`) into AST item blocks tagged with `blake3` hashes. An agent edits a block by naming its hash: a stale hash is a rejected edit, not a silent clobber — compare-and-swap for source code.

Forge reaches an agent as a **stdio MCP server** (`hadron-forge-mcp`), and Hadron attaches it to every **ACP** session. A **CLI** seat gets no MCP channel, so it keeps its own editing tools and Hadron observes the result through git — the prompt is gated on the transport, so a CLI seat is never told about tools it cannot call.

### 4. Batteries Included: Context7 & the Superpowers Skill Library

Every ACP session is also handed the **[Context7](https://context7.com)** MCP server, so an agent looks up current library documentation instead of guessing from training data.

On top of that, every turn of every seat is built from a **Standard Model** (the non-negotiable invariants — prove it runs, know your baseline, evidence not adjectives) plus a **skill library** of 15 procedures ported from **[Superpowers](https://github.com/obra/superpowers)** by Jesse Vincent (MIT): brainstorming, writing-plans, test-driven-development, systematic-debugging, requesting-code-review and more. The engine picks the skill that matches the task and injects only that one — the rest stay out of the prompt.

### 5. Git Worktree Isolation & Merge Gate

Every quark turn executes inside an isolated git worktree (`.hadron/trees/<id>`). When work completes, the **Merge Gate** automatically rebases the branch onto `main`, executes the host project's native test suite within a strict deadline, and fast-forwards clean passing code.

---

## 🏛️ System Architecture

```
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

The forked `gpui-component` is a **separate repository**
([s0lda/gpui-component](https://github.com/s0lda/gpui-component)), consumed through the
`[patch]` table in the workspace `Cargo.toml` — it is not a directory in this repo.

`hadron-gluon` and `hadron-forge-mcp` are pure libraries; their entrypoints are
`hadron_gluon::cli::run` and `hadron_forge_mcp::run`, and the bin targets that call them
live in the one installable package so `cargo install` lands all three side by side.

<div align="center">
    <img src="assets/demo.png" alt="Hadron Chamber UI Demo" width="900" />
</div>

---

## ⚛️ The Hadron Physics Metaphor & Vocabulary

Hadron uses particle physics as a cohesive mental model for multi-agent operating system concepts.

| Term                 | Meaning in Hadron                                                  | Physics Metaphor                                     |
| :------------------- | :----------------------------------------------------------------- | :--------------------------------------------------- |
| **Hadron**           | The complete multi-agent operating environment                     | A composite particle that binds quarks together      |
| **Quark**            | A seat in the swarm — an agent, its transport, its permission mode | The fundamental particle of intelligence             |
| **Preon**            | Addressable markdown instructions loaded into a quark              | Proposed substructure inside a quark                 |
| **Field**            | The shared append-only NDJSON event bus (`field.jsonl`)            | Interactive quantum field                            |
| **Event**            | One structured record in the field bus                             | A detected particle interaction                      |
| **Gluon**            | The headless daemon (`hadron-gluon`) routing tasks                 | The gauge boson/force carrier binding quarks         |
| **Lattice**          | The shared protocol data layer (`hadron-lattice`)                  | The underlying space-time lattice                    |
| **Chamber**          | The GPUI desktop workspace (`hadron-chamber`)                      | A cloud/bubble chamber for observing particle tracks |
| **Nucleus**          | Shared persistent single-source-of-truth knowledge base            | The dense, stable core                               |
| **Flavor**           | A quark's role in the swarm (Orchestrator, Worker)                 | Quark flavors (up, down, charm...)                   |
| **Energy & Ledger**  | Real-time token spend tracking and SQLite quotas                   | Conservation of energy & energy states               |
| **Excited / Ground** | A quark mid-turn / idle — the presence states                      | Exciting a field produces a particle                 |
| **Standard Model**   | Non-negotiable base system invariants (`standard_model.md`)        | Fundamental laws of physics                          |

---

## 🚀 Getting Started

### Prerequisites

- **Rust Toolchain**: a recent stable `rustup` toolchain (edition 2021; the workspace
  declares no `rust-version` floor because that number has to be measured against older
  toolchains, not guessed — this box builds on 1.96.0)
- **Platforms**: **Linux** (X11 or Wayland) and **macOS**. Windows users run it under
  **WSL2** (WSLg), which is where Hadron is developed daily. Native Windows is _not_
  supported today: it is untested, and the timeout path that kills a runaway test
  process group is a no-op off Unix (`hadron-forge/src/exec.rs:163`).
- **At least one agent**: an ACP agent (e.g. `npx @agentclientprotocol/claude-agent-acp`),
  a coding CLI on your `PATH`, or the Antigravity SDK bridge. Seats are configured in the
  Chamber's Settings, or by hand in `.hadron/team.json`.

### Install

```bash
cargo install --git https://github.com/s0lda/hadron.git hadron
```

That places **three** binaries in `~/.cargo/bin`: `hadron` (the Chamber), `hadron-gluon`
(the headless daemon) and `hadron-forge-mcp` (the stdio MCP server). They must stay in
one directory — each finds the next as a sibling of its own executable, which is why
they ship from a single package.

### Run it anywhere

**The directory you are standing in is the workspace.** No argument, no config:

```bash
cd ~/dev/project_1 && hadron     # swarm rooted at ~/dev/project_1
cd ~/dev/project_2 && hadron     # a different swarm, rooted at ~/dev/project_2
```

Each project gets its own `.hadron/` — field, team, nucleus, worktrees — created on
first run. Pass a directory (`hadron ~/dev/project_3`) to open or create one elsewhere.

### Building from Source

```bash
git clone https://github.com/s0lda/hadron.git
cd hadron

# The gate. Compiles the GUI too — expect a few minutes on a cold cache.
cargo test --workspace

# Launch the Chamber (the default workspace member). It starts the
# hadron-gluon daemon for you, and opens the swarm in the current directory.
cargo run --bin hadron
```

---

## ⌨️ Keyboard Shortcuts

| Shortcut                      | Action                                                         |
| :---------------------------- | :------------------------------------------------------------- |
| `Ctrl+Tab` / `Ctrl+\``        | Toggle focus between Chat Input and PTY Terminal               |
| `Alt+Left` / `Alt+Right`      | Switch Chat column tabs                                        |
| `Alt+PageUp` / `Alt+PageDown` | Switch Right-Rail Inspector tabs                               |
| `Alt+Up` / `Alt+Down`         | Switch Telemetry & Stats time windows                          |
| `F6`                          | Cycle the global permission mode (Ask / Write / Auto / Bypass) |

---

## 🛠️ Built With

Hadron stands on the shoulders of giants. Everything below is a real dependency of this repository:

### Core Frameworks

- **[Rust](https://www.rust-lang.org/)** — Safe, concurrent systems programming language.
- **[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)** by **[Zed Industries](https://zed.dev)** (Apache-2.0) — GPU-accelerated UI framework.
- **[gpui-component](https://github.com/longbridge/gpui-component)** by **[Longbridge](https://longbridge.com)** (Apache-2.0) — High-performance native desktop widgets. Almost every widget you see is theirs. We run a **small fork** ([s0lda/gpui-component](https://github.com/s0lda/gpui-component), pinned to a `rev` in the workspace `[patch]` table) that adds a foreground colour to `TextMark`, so an `@mention` can be coloured text rather than a tinted block — a patch off their tree, meant to go home.
- **[Agent Client Protocol (ACP)](https://agentclientprotocol.com)** by **[Zed](https://zed.dev)** — Standardized protocol for long-running resident agents.
- **[Tree-Sitter](https://github.com/tree-sitter/tree-sitter)** — Incremental AST parsing for syntax highlighting and forge block extraction.

### Transports (how an agent takes a seat)

- **ACP** — any agent implementing the Agent Client Protocol. Resident sessions, live
  tool-call streaming, and the bundled MCP servers. The catalogue ships 37 boot presets;
  each one carries a `proven` flag, and only the ones we have actually run say `true`.
- **CLI** — our own resident CLI transport, for coding CLIs with no ACP mode. It keeps a
  conversation across turns and builds an argv `execve` will actually accept.
- **[Antigravity](https://antigravity.google/)** — over its CLI _and_ over our SDK bridge,
  a Python ACP shim in `crates/hadron-gluon/scripts/`.

### Agent Tooling We Bundle

- **[Superpowers](https://github.com/obra/superpowers)** by Jesse Vincent (MIT) — the 15
  skill procedures in `crates/hadron-gluon/invariants/skills/`, injected by the engine
  rather than by editor hooks. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- **[Context7](https://context7.com)** — documentation MCP server, attached to every ACP
  session so agents read current docs instead of recalling stale ones.
- **Hadron Forge MCP** — our own stdio MCP server: edit-by-hash, jailed `exec`, `grep`,
  `cargo` diagnostics, git history, and nucleus knowledge search.

### Ecosystem Crates

- [`tokio`](https://tokio.rs) (async runtime)
- [`serde`](https://serde.rs) / `serde_json` (NDJSON wire format)
- [`rusqlite`](https://github.com/rusqlite/rusqlite) + [SQLite](https://sqlite.org) (energy ledger)
- [`notify`](https://github.com/notify-rs/notify) (filesystem watching bus)
- [`ulid`](https://github.com/dylanhart/ulid-rs) (sortable IDs for events & turns)
- [`chrono`](https://github.com/chronotope/chrono) · [`anyhow`](https://github.com/dtolnay/anyhow) · [`futures`](https://github.com/rust-lang/futures-rs) · [`markdown`](https://github.com/wooorm/markdown-rs) · [`blake3`](https://github.com/BLAKE3-team/BLAKE3) · [`lsp-types`](https://github.com/gluon-lang/lsp-types) · [`emojis`](https://github.com/rossmacarthur/emojis) · [`tempfile`](https://github.com/Stebalien/tempfile)

Every dependency above is used under its own licence; the full set is listed in
**[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)**.

---

## 📄 Licence & Community

- **License**: Licensed under the **[Apache License 2.0](LICENSE)**.
- **Contributing**: Read **[CONTRIBUTING.md](CONTRIBUTING.md)** for build instructions, developer test gates, and Standard Model invariants.
- **Security**: Read **[SECURITY.md](SECURITY.md)** before you deploy this — how to report a vulnerability, and an honest account of what Hadron does to your machine. **Hadron runs AI agents that execute code as you, in your repository. It is not a sandbox.**
- **Code of Conduct**: Read **[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)** (Contributor Covenant 2.1).
