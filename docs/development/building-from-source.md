# 🛠️ Building & Development Guide

This guide covers prerequisites, installation options, local workspace execution, and building Hadron from source.

---

## Prerequisites

- **Rust Toolchain**: 
  - A recent stable `rustup` toolchain (edition 2021; verified on Rust 1.96.0).
- **Supported Platforms**:
  - **Linux** (X11 or Wayland)
  - **macOS**
  - **Windows** (native MSVC or WSL2 / WSLg)
- **Seated Agents**:
  - At least one supported agent: ACP agent (e.g. `npx @agentclientprotocol/claude-agent-acp`), a coding CLI on your `PATH`, the Antigravity SDK bridge, or local/cloud HTTP providers (Ollama, LM Studio, OpenRouter).

---

## Installation Options

### Standard Installation via Cargo

```bash
cargo install --locked --git https://github.com/s0lda/hadron.git hadron
```

> **Note**: `--locked` is **mandatory**. `cargo install` ignores `Cargo.lock` by default, but Hadron depends on `zed-industries/zed` by branch rev. Using `--locked` ensures you build against tested dependency revisions.

This installs three sibling binaries in `~/.cargo/bin`:
- `hadron` (The Chamber desktop visualizer)
- `hadron-gluon` (The headless daemon)
- `hadron-forge-mcp` (The stdio MCP server)

---

## Workspace Execution

Hadron operates directly on whatever directory you launch it in:

```bash
# Launch swarm in project_1
cd ~/dev/project_1 && hadron

# Launch swarm in project_2
cd ~/dev/project_2 && hadron
```

On first run, Hadron automatically initializes a local `.hadron/` directory containing `.hadron/field.jsonl`, `.hadron/team.json`, `.hadron/nucleus/`, and isolated worktrees.

---

## Building from Source

```bash
# 1. Clone the repository
git clone https://github.com/s0lda/hadron.git
cd hadron

# 2. Run the test gate
cargo test --workspace

# 3. Launch the Chamber (GUI + Daemon)
cargo run --bin hadron
```
