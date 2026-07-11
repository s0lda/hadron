# 🌌 Project Hadron: Master Execution Plan

## Powered by **Quark**: The fundamental particle of intelligence.

A single Quark cannot build complex software in isolation. It requires an environment to bind it to tools, files, and other agents. Hadron is that environment.

# 🔭 The Vision

`The ultimate endgame of this project is the integration of Quark, your custom, highly-specialized AI model.
To prepare for Quark, we are building Hadron—a hyper-fast, natively compiled Rust multi-agent operating system. Until Quark is ready, Hadron will securely orchestrate existing models (Claude, Llama, OpenAI) using a zero-CPU filesystem bus. The day your model is ready, it will slot perfectly into the "Orchestrator" seat.

# 🏛️ The Architecture: Decoupled Workspace

The system is a two-tier architecture communicating strictly through the field.jsonl File Bus and local SQLite databases.

```
hadron/
├── Cargo.toml
├── crates/
│   ├── hadron-lattice/   (The Shared Protocol: Structs, Intents, & Edit-by-Hash schemas)
│   ├── hadron-gluon/     (The Headless Daemon: Watches files, routes tasks, runs Git)
│   └── hadron-chamber/   (The GPUI Glass: The 120 FPS Native Visualizer)
```

## The Vocabulary

Hadron uses particle physics as a metaphor for its architecture. This creates a cohesive, single-source-of-truth vernacular.

| Term | Meaning in Hadron | Physics Metaphor |
|---|---|---|
| **Hadron** | The whole environment/studio | A composite particle that binds quarks |
| **Quark** | An agent or citizen (e.g., Claude, Antigravity) | The fundamental unit of intelligence |
| **Field** | The shared append-only bus (`field.jsonl`) | Particles interact through fields |
| **Event** | One line in the field | A detected particle interaction |
| **Gluon** | The headless daemon (`hadron-gluon`) | The force carrier that binds quarks |
| **Lattice** | The shared protocol/schema crate | The framework of quark interactions |
| **Chamber** | The GPUI viewer / chat app | A bubble chamber, where tracks are observed |
| **Nucleus** | Persistent per-project SSOT knowledge | The dense stable core quarks orbit |
| **Flavor** | A quark's role (Orchestrator, Worker) | Quark flavors (up, down, charm...) |
| **Energy** | Token / cost budget tracking | Running a quark costs energy |
| **Ledger** | The SQLite database that records token usage and quotas for quarks. | Conservation of energy ledger |
| **Excite** | Waking a sleeping quark to run | Exciting a field produces a particle |
| **Standard Model** | Base invariants (`standard_model.md`) | The baseline laws of physics |

