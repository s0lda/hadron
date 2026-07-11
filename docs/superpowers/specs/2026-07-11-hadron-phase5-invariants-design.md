# Hadron Phase 5 — Invariants (Enforced Methodology) Design Spec

- **Date:** 2026-07-11
- **Status:** Draft

## Overview

In Hadron, **Invariants** are the enforced methodology rules that quarks must uphold on every turn. Up until now, the `invariants` field in a quark's `Projection` has been a static preamble. Phase 5 modularizes this system, allowing the Orchestrator to dynamically assemble focused rule sets when assigning tasks to workers, significantly reducing context noise while maximizing provider prompt-caching efficiency.

## 1. Storage (`standard_model.md` and Modular Invariants)

Invariants are stored as Markdown files within the project's SSOT nucleus:
`.hadron/nucleus/invariants/`

1. **`standard_model.md`**: The base rules (the "standard model" of physics). If this file exists, it is **always** included in every projection for every quark.
2. **Modular Invariants**: Specialized rule sets like `ui.md`, `permissions.md`, or `db.md`. These are only included when explicitly requested.

## 2. Lattice Schema Upgrade (`Kind::Assign`)

To allow the Orchestrator to declare which invariants apply to a task, we are introducing a new event kind to the `hadron-lattice` schema:

```rust
Kind::Assign {
    task: String,
    invariants: Vec<String>, // e.g., ["ui", "permissions"]
}
```

- `Kind::Message` remains the standard for informal chat, coordination, and clarifying questions.
- `Kind::Assign` is the formal delegation mechanism.

## 3. Orchestrator Awareness

For the Orchestrator to know which invariants it can assign, the `Projection` struct is updated to include an index of available rules:

```rust
struct Projection {
    task: String,
    invariants: String,
    available_invariants: Vec<String>, // e.g. ["ui", "permissions", "db"]
    // ... existing fields (nucleus_digest, roster, field_window, git_diff)
}
```

The Gluon engine populates `available_invariants` by listing the basenames (without `.md`) of all files in `.hadron/nucleus/invariants/`, excluding `standard_model.md`.

## 4. Gluon Injection & Cache Optimization

LLM providers (like Anthropic and Google) cache identical sequences of tokens at the *start* of a prompt. To maximize this free caching, Gluon will inject the text directly into the `Projection.invariants` string in a strictly deterministic order:

1. Read `standard_model.md` (if it exists).
2. Sort the requested modular invariants from `Kind::Assign` alphabetically (e.g., `db.md`, then `ui.md`).
3. Read each requested file.
4. Concatenate them with clear markdown headers.
5. Place this combined string into `Projection.invariants`.

Because the adapters place the `invariants` block at the very top of the system prompt, this deterministic ordering ensures a highly stable prefix that maximizes cache hits.

## 5. Vocabulary Update (`README.md`)

As part of this phase, the root `README.md` must be updated to formally document the Hadron vocabulary to help new users and agents understand the physical metaphors (e.g., Quark, Gluon, Lattice, Nucleus, Chamber, Energy, Excite, Standard Model).
