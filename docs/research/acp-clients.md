# ACP Clients Competitive Research

Based on the [Agent Client Protocol Clients registry](https://agentclientprotocol.com/get-started/clients), here is the landscape of existing ACP clients.

## Does anything do exactly what Hadron does?

**No.** No existing client explicitly implements Hadron's exact paradigm: **multiple heterogeneous agents in one shared workspace, taking turns on a shared bus with per-agent telemetry.**

Most multi-agent systems listed either silo agents into parallel sessions/tabs or orchestrate them opaquely without a shared, transparent bus or granular per-agent telemetry.

### The Closest Alternatives:

1. **Jockey**: Described as an "open-source multi-agent orchestrator (Tauri + Rust + SolidJS) that coordinates Claude Code, Gemini CLI, and Codex CLI via ACP." It orchestrates heterogeneous agents, but it's unclear if they share a single visible bus/turn-based system or if it has per-agent telemetry. 
2. **Codeg**: A "collaborative multi-agent coding workbench that unifies ACP agents... with session aggregation." It unifies agents, but "session aggregation" sounds different from Hadron's shared event bus where agents explicitly take turns.
3. **Braide**: Features "Parallel sessions, worktrees, personas and interactive agent responses." The focus is on parallel sessions rather than a shared workspace/bus.
4. **Obsidian Agent Console plugin**: Provides a "tabbed multi-session workspace" to run several ACP agents in parallel, which explicitly separates them rather than putting them on a shared bus.

## Do any expose ACP slash-commands?

**There is no mention of ACP slash-commands** in any of the client descriptions on the registry page. While some tools like *Obsidian Agent Console* mention "quick prompts", there is no explicit standard or feature listed for exposing ACP-native slash commands to the user in the way Hadron intends to implement them.

## General Landscape
The ACP ecosystem is growing rapidly across several categories:
- **Editors/IDEs**: Plugins for Neovim (CodeCompanion, agentic.nvim, avante.nvim, hermes.nvim), VS Code (ACP Client, ACP Pro), JetBrains, Emacs, Obsidian, Unity, and Pulsar.
- **CLI/TUI**: acpx, Hash, Hydra, Nori CLI, pool, Toad.
- **Desktop/Web/GUI**: ACP UI, Agent Studio, AgentRQ, Braide, Codeg, Jockey, etc.
- **Messaging Bridges**: Discord, Slack, Telegram, WeChat, Matrix (Zooid), Lark, QQ.
- **Frameworks**: AgentPool, LangChain/LangGraph, LlamaIndex, Mastra, Pydantic AI adapters.
