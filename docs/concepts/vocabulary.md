# 📖 Hadron Glossary & Vocabulary

This document provides definitive definitions for terms and concepts used across the Hadron codebase, logs, protocol schemas, and user interface.

---

## Glossary

### **Hadron**
The complete multi-agent operating environment and workspace orchestrator.

### **Quark**
A seat in the swarm representing an active agent process, its underlying transport layer (ACP, CLI, SDK bridge, or HTTP for Ollama/OpenRouter/LM Studio), and its configured permission mode.

### **Preon**
Addressable markdown instructions and prompt fragments dynamically loaded into a Quark's context window (`~/.hadron/preons`, `<repo>/.hadron/preons`).

### **Field**
The shared, append-only NDJSON event bus (`field.jsonl`) that acts as the single source of truth for all communication between humans, daemons, and agents.

### **Event**
A single structured JSON record posted to the Field. Events represent user prompts, agent turns, tool calls, status changes, and token spend telemetry.

### **Gluon**
The headless daemon process (`hadron-gluon`) responsible for file watching, event routing, worktree creation, security verification, and git merge execution.

### **Lattice**
The shared Rust protocol library (`hadron-lattice`) defining event structures, NDJSON serialization rules, intent schemas, and Edit-by-Hash data types.

### **Chamber**
The native GPU-accelerated desktop application (`hadron-chamber`) built with Rust and GPUI. It provides human operators with chat UI, multi-tab PTY terminal access, git inspection, live plan tracking, and telemetry graphs.

### **Nucleus**
The persistent single-source-of-truth knowledge base shared across all Quarks in a workspace (`.hadron/nucleus/`):
- **Index (`index.md`)**: Compact routing table of one-line lesson pointers within the configured budget limit.
- **Notes (`notes/*.md`)**: Distilled post-mortem learnings, user preferences, and non-obvious constraints.
- **Invariants (`invariants.md`)**: Permanent operational and structural codebase constraints.
- **Features (`features.md`)**: High-level capability and entrypoint map.

### **Flavor**
The operational role assigned to a Quark:
- **Orchestrator**: Evaluates user prompts, maintains master execution plans, and @mentions worker agents.
- **Worker**: Executes focused code edits, tests, or commands inside an isolated git worktree branch.

### **Energy & Ledger**
Real-time token spending metrics and SQLite budget ledgers used to track, visualize, and cap LLM consumption.

### **Excited / Ground**
Presence state of a Quark:
- **Excited**: Mid-turn, actively thinking or executing tool calls.
- **Ground**: Idle and waiting for new events or @mentions in `field.jsonl`.

### **Standard Model**
The non-negotiable base invariants (`standard_model.md`) injected into every agent turn (e.g. prove it runs, know your baseline, evidence not adjectives).

### **Merge Gate**
An automated verification step in `hadron-gluon` that rebases completed agent branches onto `main`, runs the native project test suite, and fast-forwards clean code.

### **Hadron Forge**
An AST-level code editing and developer environment power tools suite:
- **AST Edit-by-Hash**: `blake3` item-block hashing to perform race-free compare-and-swap edits across 14+ languages.
- **Background Process Supervisor**: Managed long-running servers and background processes with PGID isolation and ring-buffer streaming logs.
- **3-Tier Polyglot Code Intelligence**: Universal syntax inspection via embedded Tree-Sitter ASTs (Tier 1), generic STDIO LSP clients (Tier 2), and ecosystem documentation (Tier 3).
- **Headless Browser Bridge**: Built-in CDP local verification bridge for DOM snapshots, visual checking, and UI interaction.
- **Jailed Screenshot Engine**: Strict `<repo_root>/.hadron/screenshots/` containment for window and desktop inspection with zero leakage.
- **Interactive PTY Session Manager**: Unix pseudo-terminal allocation for interactive terminal apps and raw keystroke streaming.
- **In-Process Mock Server**: Loopback HTTP/WS mock endpoints for webhook, API contract, and frontend verification.
- **Local SQLite Engine**: In-process database introspection, query, and migration testing.

### **Universal Assistant Absorption (`/absorb`)**
Automated discovery and distillation engine that scans foreign assistant configurations (`.agents/`, `.claude/`, `CLAUDE.md`, `.cursor/`, `.windsurf/`, `.kimi/`, `.superpowers/`, etc.) and imports memories, invariants, skills, and plans cleanly into `.hadron/`.

### **Hub-and-Spoke**
The swarm communication architecture where the **Orchestrator Quark** acts as the central hub: fanning out parallel tasks to Worker Quarks, receiving completion reports and blocker escalations, and maintaining the single master execution plan.

### **Swarm Commands**
Interactive slash commands available in Chamber chat (e.g., `/goal`, `/loop`, `/absorb`, `/release`, `/git-init`, `/clear`, `/resume`, `/help`). See [`docs/commands.md`](../commands.md) for the complete reference.
