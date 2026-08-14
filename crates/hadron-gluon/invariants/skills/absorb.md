---
name: absorb
description: Use when migrating or absorbing foreign assistant configurations, skills, memories, lessons, rules, or plans from other AI tools into .hadron/
---

# Absorbing Foreign Assistant Knowledge (.hadron/ Migration)

Universal procedure for discovering, extracting, distilling, and unifying foreign AI assistant configurations into Hadron's single source of truth (`.hadron/`).

## Phase 1: Dynamic Discovery (Universal Repo Scan)

Do NOT rely solely on a fixed list of paths. Perform a comprehensive workspace search to discover all foreign assistant context:

1. **Known Assistant Directories:**
   - `.agents/` (Antigravity / Gemini CLI)
   - `.claude/` (Anthropic Claude Code)
   - `.remember/` (Legacy memory stores)
   - `.superpowers/` (Superpowers skill suites)
   - `.cursor/` (Cursor IDE rules & context)
   - `.windsurf/` (Windsurf IDE rules & memories)
   - `.kimi/` (Kimi AI assistant guidelines)
   - `.continue/` (Continue.dev prompts & configs)
   - `.aide/`, `.cline/`, `.roo/`, `.zed/` (Aide, Cline, Roo Code, Zed AI)
2. **Root & Config Instruction Files:**
   - `CLAUDE.md`, `.cursorrules`, `AGENTS.md`, `COPILOT.md`, `INSTRUCTIONS.md`, `PROMPT.md`, `RULES.md`, `MEMORY.md`
   - `.github/copilot-instructions.md`, `.github/instructions.md`, `.vscode/settings.json` (AI sections)
3. **Dynamic Path Heuristics:**
   - Scan for any dot-directories or config folders containing `skills/`, `memory/`, `memories/`, `rules/`, `specs/`, `plans/`, `prompts/`, or `invariants/`.

## Phase 2: Categorization & Distillation

Categorize discovered artifacts into Hadron target structures:

### 1. Memories & Post-Mortem Lessons → `.hadron/nucleus/notes/<slug>.md` & `.hadron/nucleus/index.md`
- **Standard Model Rule 9 Compliance:** One fact per note, strictly post-mortem lessons (hard-won discoveries, non-intuitive constraints, user preferences).
- **Format:** Frontmatter with `name`, `description`, `metadata.type` (`user | feedback | project | reference`), followed by the lesson body with **Why:** and **How to apply:** lines.
- **Index Pointer:** Add a single routing line per note to `.hadron/nucleus/index.md`:
  `- [<slug>](notes/<slug>.md) — <hook>` (hook capped at ~100 characters).
- **Budget Control:** Keep `.hadron/nucleus/index.md` within the workspace's configured budget limit (default 32 KB, or 16 / 64 / 128 KB configured in Settings / `team.json`).
- **Deduplication:** Check existing notes in `notes/` before creating a new one. Update existing notes rather than creating duplicate entries.

### 2. Invariants, Quirks & Operational Constraints → `.hadron/nucleus/invariants.md`
- Extract codebase rules, environment quirks (e.g. Vulkan, Lavapipe, OS limits), security boundaries, and protocol constraints.
- Merge into the appropriate sections of `.hadron/nucleus/invariants.md`.
- Never duplicate an invariant already present.

### 3. Procedural Skills → `.hadron/skills/<slug>.md`
- Convert foreign skill files (e.g. from `.superpowers/skills/` or `.claude/skills/`):
  - Ensure standard YAML frontmatter (`name: <slug>`, `description: Use when...`).
  - Strip provider-specific wrappers (e.g. Anthropic tool XML tags, Antigravity-specific subagent headers).
  - Ensure the procedure is self-contained and actionable.

### 4. Architecture Specs & Master Plans → `.hadron/docs/specs/` & `.hadron/docs/plans/`
- Relocate active or reference design specifications to `.hadron/docs/specs/`.
- Relocate execution plans to `.hadron/docs/plans/`.

### 5. Personas & Custom Prompts → `.hadron/preons/<name>.md`
- Convert custom assistant personas, specialized reviewer prompts, or system instructions into Hadron preons with `preferred_role` frontmatter.

## Phase 3: Verification & Reporting

1. **Verify Integrity:**
   - Verify all generated notes have valid YAML frontmatter and pointers in `index.md`.
   - Verify `index.md` size is within the configured budget limit.
   - Run `cargo test` if applicable to ensure no syntax/schema invariants are violated.
2. **Preserve Original Data:**
   - Keep source files intact by default (or move to an archive folder if explicitly requested). Never perform destructive deletions.
3. **Structured Summary Report:**
   - List discovered source directories and files.
   - Summary of absorbed items (notes added, invariants merged, skills imported, plans relocated).
   - Exact file diffs and new paths in `.hadron/`.
