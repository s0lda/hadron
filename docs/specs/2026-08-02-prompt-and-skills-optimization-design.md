# Design Spec: Hadron Prompt Cache Optimization, Skill Distillation, & Response Strictness

**Date:** 2026-08-02  
**Status:** Approved  
**Author:** Antigravity / Hadron Swarm Architecture  

---

## 1. Overview & Objectives

This specification defines systemic improvements to Hadron's prompt adapter (`crates/hadron-gluon/src/adapter/prompt/mod.rs`), standard model rules (`standard_model.md`), and embedded skills library (`crates/hadron-gluon/invariants/skills/`).

### Primary Goals
1. **Maximize LLM Prompt Caching**: Restructure prompt generation so that static and invariant directives form a byte-identical prefix across turns and quarks, maximizing KV-cache hit rates on Claude, Gemini, OpenAI, vLLM, and Ollama.
2. **Distill Embedded Skills Payload**: Compact all 15 embedded skills in `crates/hadron-gluon/invariants/skills/` by stripping human-facing tutorial prose while maintaining 100% of execution gates and mandatory procedures, cutting active skill prompt bloat by ~50–70% (3,000–5,500 tokens per turn).
3. **Enforce Zero-Essay Response Strictness**: Eliminate preamble text and narrative essays (specifically targeting verbose Anthropic/Claude responses) before or after structured reports.

---

## 2. Architecture & Prompt Order

`build()` in `crates/hadron-gluon/src/adapter/prompt/mod.rs` will be re-ordered into two distinct parts:

### Part A: Cache-Stable Prefix (Static across turns & seats)
This prefix contains invariant directives, seat capabilities, authority guidance, and response templates. For any given seat and permission mode, this prefix remains 100% byte-identical turn-after-turn:

1. `# CRITICAL DIRECTIVE: FOLLOW THE STANDARD MODEL AND ITS SKILLS` (`CRITICAL_DIRECTIVE_HEADER`)
2. `# Working protocol (Invariants)` (`projection.invariants`)
3. `# Available Hadron Forge Tools` (gated on `has_forge_tools`)
4. `# Your authority this turn` (`mode_guidance(projection.mode)`)
5. `# Response Format & Output Strictness` (Strict anti-essay rules & markdown template)
6. `# Role & Escalation Directives` (Worker vs Orchestrator responsibilities, Bypass execution loop)
7. `# Project knowledge (nucleus)` (`projection.nucleus_digest`)

### Part B: Dynamic Suffix (Per-turn & Per-Quark data)
All dynamic inputs, task definitions, and turn-specific state sit below the cache-stable prefix:

8. `# Who you are` (`display_for(&projection.roster, self_id)` — moved below prefix so multiple quarks share the cache prefix)
9. `# Where you are` (`projection.cwd`, isolated worktree vs shared checkout)
10. `# What the swarm has learned (nucleus index)` (`projection.nucleus_index` & task-ranked notes)
11. `# Live Activity` (parallel quark status)
12. `# Broadcast Scope Directive` (if reached via `@team` or unaddressed)
13. `active_skill` payload (`projection.active_skill` — compact execution steps)
14. `# Your task` (`projection.task`)
15. `# Recent field` (`projection.field_window`)
16. `# Current working diff` (`projection.git_diff`)

---

## 3. Skill Payload Distillation

All 15 skill files in `crates/hadron-gluon/invariants/skills/` will be audited and converted into high-density machine execution procedures:

| Skill | Current Size | Target Size | Strategy |
| :--- | :--- | :--- | :--- |
| `subagent-driven-development.md` | 28.6 KB | ~7.5 KB | Convert extended prose & ASCII diagrams to imperative steps. Retain all subagent dispatch and gate rules. |
| `writing-skills.md` | 29.8 KB | ~8.0 KB | Strip authoring meta-tutorials and long examples. Retain structural schema & verification gates. |
| `systematic-debugging.md` | 13.2 KB | ~4.5 KB | Retain 4-phase investigation workflow & log verification rules; remove redundant preamble. |
| `test-driven-development.md` | 12.1 KB | ~4.5 KB | Retain Red-Green-Refactor enforcement & failing test output proof requirements. |
| `brainstorming.md` | 8.1 KB | ~3.5 KB | Retain 9-step checklist, hard gate, and visual companion rules; strip tutorial prose. |
| *10 Remaining Skills* | 2.3–7.4 KB | 1.5–3.5 KB | Compact prose into high-density imperative bullet lists. |

---

## 4. Response Strictness & Anti-Essay Directives

To prevent models (especially Anthropic/Claude models) from writing conversational essays prior to or following the required report format, `mod.rs` will enforce the following rule:

```markdown
# CRITICAL Response Requirement: No Preamble or Essays
Your entire response MUST consist ONLY of the structured report format below (or a direct brief answer if you changed nothing). 
DO NOT write any narrative explanation, conversational intro (e.g. "I have completed the requested changes..."), summary-of-your-summary, or closing essay before or after the report.

If you CHANGED files/refs this turn, format strictly as:
**Done**: [Brief outcome summary, including commit hash]
- **Done**: [List of modified files]
- **Evidence**: [Exact command run + concise summary output per Standard Model Rule 6]
- **Risks**: [Security impact per Rule 7, or omit bullet]
- **What I did not verify / clean up**: [Explicit unverified items]

If you changed NOTHING (read-only / Q&A), reply directly with a brief answer (Standard Model Rule 11).
```

---

## 5. Testing & Verification

1. **Prompt Unit Tests (`crates/hadron-gluon/src/adapter/prompt/tests.rs`)**:
   * Update existing section position assertions to match the new cache-stable prefix order.
   * `prompt_cache_prefix_strictly_identical_across_dynamic_turn_variations`: Assert prefix stability before line 8 (`# Who you are`).
   * `measure()` vs `build()` boundary consistency tests.
2. **Workspace Cargo Gate**:
   * Execute `cargo check --workspace` and `cargo test -p hadron-gluon`.
