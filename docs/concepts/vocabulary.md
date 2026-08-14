# 📖 Hadron Glossary & Vocabulary

This document provides definitive definitions for terms and concepts used across the Hadron codebase, logs, protocol schemas, and user interface.

---

## Glossary

### **Hadron**
The complete multi-agent operating environment and workspace orchestrator.

### **Quark**
A seat in the swarm representing an active agent process, its underlying transport layer (ACP, CLI, SDK bridge, or HTTP for Ollama/OpenRouter/LM Studio), and its configured permission mode.

### **Preon**
Addressable markdown instructions and prompt fragments dynamically loaded into a Quark's context window.

### **Field**
The shared, append-only NDJSON event bus (`field.jsonl`) that acts as the single source of truth for all communication between humans, daemons, and agents.

### **Event**
A single structured JSON record posted to the Field. Events represent user prompts, agent turns, tool calls, status changes, and token spend telemetry.

### **Gluon**
The headless daemon process (`hadron-gluon`) responsible for file watching, event routing, worktree creation, security verification, and git merge execution.

### **Lattice**
The shared Rust protocol library (`hadron-lattice`) defining event structures, NDJSON serialization rules, intent schemas, and Edit-by-Hash data types.

### **Chamber**
The native GPU-accelerated desktop application (`hadron-chamber`) built with Rust and GPUI. It provides human operators with chat UI, terminal access, git inspection, and live telemetry graphs.

### **Nucleus**
The persistent single-source-of-truth knowledge base shared across all Quarks in a workspace (`.hadron/nucleus/`).

### **Flavor**
The operational role assigned to a Quark:
- **Orchestrator**: Evaluates user prompts, plans task distribution, and @mentions worker agents.
- **Worker**: Executes focused code edits or commands inside an isolated git worktree branch.

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
An AST-level code editing tool that uses `blake3` item block hashing to perform race-free compare-and-swap edits across 14+ languages.

### **Hub-and-Spoke**
The swarm communication architecture where the **Orchestrator Quark** acts as the central hub: fanning out parallel tasks to Worker Quarks, receiving completion reports and blocker escalations, and maintaining the single master execution plan.

### **Swarm Commands**
Interactive slash commands available in Chamber chat:
- `/goal <prompt>`: Synthesize an end-to-end mission, generate a structured plan in `.hadron/docs/plans/`, and fan out tasks across worker Quarks.
- `/loop [N] <prompt>`: Execute an autonomous iterative evaluation loop until completion criteria are met or iterations expire.
- `/git-init`: Initialize a Git repository on `main` with standard `.gitignore` and initial commit to enable worktree isolation.
- `/clear`: Clear the active chat field history.
- `/resume [id]`: Switch to or restore a previous session history.
- `/help`: Display the single-source-of-truth command reference and shortcuts table.
