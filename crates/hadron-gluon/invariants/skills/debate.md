---
name: debate
description: Conduct structured architectural debate and trade-off analysis without code changes, producing an ADR
---

# Architectural Debate & Dialectic Analysis

Evaluate contentious technical questions, competing implementation approaches, and architectural trade-offs through structured dialectic discourse before any code or plans are written.

<HARD-GATE>
Tool Restriction & Posture Gate:
- **Read-Only Posture:** The debate phase is strictly non-generative. Modifying repository files, creating git branches, and drafting execution plans are prohibited.
- **Allowed Tools:** Inspection and navigation only (`read_file`, `grep_search`, `file_search`, `list_directory`, symbol outlines).
- **Prohibited Tools:** `write_file`, `replace_file_content`, edit-by-hash, command execution that writes or deletes state, `git_commit`.
- **Exit Gate:** Debate continues until settled (via `/settle` or human consensus). The debate produces an Architectural Decision Record (ADR) saved to `.hadron/docs/adr/`.
</HARD-GATE>

## Dialectic Roles

When multiple quarks participate, the orchestrator assigns distinct dialectic stances:

1. **Thesis (Proponent):**
   - Steel-mans the primary proposal.
   - Highlights design elegance, ergonomics, velocity, and core benefits.
   - Cites concrete architectural precedents in the codebase.

2. **Antithesis (Devil's Advocate / Skeptic):**
   - Stress-tests failure modes, edge cases, and long-term maintenance costs.
   - Evaluates worst-case performance, lock contention, and operational overhead.
   - Questions hidden assumptions and unmodeled dependencies.

3. **Synthesis (Orchestrator):**
   - Compares both stances against Standard Model invariants (SSOT, Make invalid states unrepresentable, Simplicity first).
   - Identifies the decisive trade-offs and decision boundary.
   - Formulates the final recommendation.

## Debate Checklist

1. **Frame the Question:** State the architectural dilemma clearly in 1-2 sentences.
2. **Surface Existing Precedents:** Check current types, modules, and nucleus notes (`.hadron/nucleus/`) for prior art.
3. **Present Arguments:**
   - Option A vs Option B with concrete trade-off matrix.
   - Non-negotiable constraints and failure boundaries.
4. **Identify Invariants Impacted:** Which Standard Model or project invariants apply?
5. **Resolution via `/settle`:**
   - Summarize verdict.
   - Record ADR in `.hadron/docs/adr/YYYY-MM-DD-<topic>.md`.
   - Hand off to `/brainstorm` or `/writing-plans` only if implementation is desired.
