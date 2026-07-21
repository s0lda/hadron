# Design: Hadron Workspace SSOT Directory Enforcement (`.hadron/`)

## Context & Purpose
In Hadron's multi-agent architecture, different LLM engines and provider SDKs (such as Claude Code, ACP agents, or custom swarms) historically created vendor-specific directories (such as `.agents/`, `.claude/`, or `.superpowers/`). This fragmentation created ambiguity about where memory, plans, specs, and operational invariants live.

This design establishes `.hadron/` as the strict **Single Source of Truth (SSOT)** for all Quarks regardless of model provider or transport mechanism.

## Operational Changes & Constraints

### 1. Standard Model Rule 3 Invariant Update
We will update **Rule 3** of `crates/hadron-gluon/invariants/standard_model.md` to explicitly mandate:
- **Workspace Single Source of Truth**: All Quarks MUST store, read, and maintain memory (`.hadron/memory/`), specs (`.hadron/docs/specs/`), plans (`.hadron/docs/plans/`), roles (`.hadron/roles/`), and scratch files exclusively within the `.hadron/` directory tree.
- **Prohibition of Provider Subfolders**: Quarks MUST NOT create or write to vendor-specific directories such as `.claude/`, `.agents/`, `.superpowers/`, `.openai/`, or `.gemini/`.

### 2. Cleanup of Obsolete Directories & Configuration
- **Directory Deletion**: Remove `.agents/`, `.claude/`, and `.superpowers/` from the repository root.
- **`.gitignore` Hygiene**: Remove `.claude/` and `.superpowers/` entries from `.gitignore` so that `.hadron/` remains the sole runtime/workspace folder.

### 3. Skill Templates Verification
- Audit all skill markdown templates under `crates/hadron-gluon/invariants/skills/` to ensure all path instructions explicitly reference `.hadron/`.

## Verification & Validation Plan
1. Check git status to confirm `.agents/`, `.claude/`, and `.superpowers/` are completely removed.
2. Run `cargo test --workspace --features gui` to ensure all workspace tests pass green.
