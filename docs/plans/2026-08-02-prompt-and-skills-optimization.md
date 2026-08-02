# Hadron Prompt Cache Optimization & Skill Payload Distillation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-order `build()` in `crates/hadron-gluon/src/adapter/prompt/mod.rs` for maximum LLM prompt caching, enforce zero-essay response formatting, distill all 15 skills in `crates/hadron-gluon/invariants/skills/`, and update the unit test suite.

**Architecture:** Split prompt rendering into a Cache-Stable Prefix (Invariants, Forge Tools, Authority/Mode, Response Format, Nucleus Digest) and a Dynamic Suffix (Identity, CWD, Nucleus Index, Live Activity, Skill, Task, Field Window, Diff).

**Tech Stack:** Rust 2021 (`hadron-gluon`), Markdown skills library.

## Global Constraints

- Preserve 100% of standard model invariant rules (Rules 0–11) in `standard_model.md`.
- Preserve 100% of mandatory skill execution gates, checklists, and output contracts across all 15 skills.
- Maintain full compatibility with `hadron-lattice::Projection` and single-string prompt CLI transports.
- Ensure prompt cache prefix stability before line 8 (`# Who you are`).

---

### Task 1: Prompt Adapter Restructuring & Zero-Essay Directives

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/prompt/mod.rs:92-438`

**Interfaces:**
- Consumes: `hadron_lattice::Projection`, `hadron_lattice::QuarkId`
- Produces: `pub fn build(projection: &Projection, self_id: &QuarkId) -> String`

- [ ] **Step 1: Update `build()` section ordering in `mod.rs`**

Update `build()` in `crates/hadron-gluon/src/adapter/prompt/mod.rs` so that:
1. `CRITICAL_DIRECTIVE_HEADER`
2. Working Protocol (Invariants)
3. Available Hadron Forge Tools (if `has_forge_tools`)
4. Authority (`mode_guidance(projection.mode)`)
5. Response Format & Zero-Essay Anti-Preamble Directive
6. Role & Escalation Directives (Worker vs Orchestrator responsibilities, Bypass execution loop)
7. Nucleus Digest (`projection.nucleus_digest`)
are rendered FIRST as the Cache-Stable Prefix.

Then render the Dynamic Suffix:
8. Who you are (`# Who you are`)
9. Where you are (`# Where you are`)
10. What the swarm has learned (`# What the swarm has learned (nucleus index)`)
11. Live Activity (`# Live Activity`)
12. Broadcast Status (`# This is a broadcast, not an assignment`)
13. Active Skill (`projection.active_skill`)
14. Your task (`# Your task`)
15. Recent field (`# Recent field`)
16. Current working diff (`# Current working diff`)

- [ ] **Step 2: Add strict zero-essay response format directive**

In `mod.rs`, update the response instructions block to strictly forbid narrative introductory text or closing summaries:

```rust
p.push_str(
    "# CRITICAL Response Requirement: No Preamble or Essays\n\
     Your entire response MUST consist ONLY of the structured report format below (or a direct brief answer if you changed nothing).\n\
     DO NOT write any narrative explanation, conversational intro (e.g. 'I have completed the requested changes...'), summary-of-your-summary, or closing essay before or after the report.\n\n\
     If you CHANGED files/refs this turn, format strictly as:\n\n\
     **Done**: [Brief outcome summary, including commit hash]\n\n\
     - **Done**:\n\
       - [Brief list of key completed tasks and files changed]\n\
     - **Evidence**: [Exact command run + concise summary output per Standard Model Rule 6]\n\
     - **Risks**: [Security impact per Rule 7, or omit bullet]\n\
     - **What I did not verify / clean up**: [Explicit unverified items]\n\n\
     If you changed NOTHING (read-only / Q&A), reply directly with a brief answer (Standard Model Rule 11).\n\n"
);
```

- [ ] **Step 3: Check compilation**

Run: `cargo check -p hadron-gluon`
Expected: Clean compilation or expected test failure in `prompt/tests.rs` due to section re-ordering.

---

### Task 2: Distill Heavy Workflow Skills

**Files:**
- Modify: `crates/hadron-gluon/invariants/skills/subagent-driven-development.md`
- Modify: `crates/hadron-gluon/invariants/skills/writing-skills.md`
- Modify: `crates/hadron-gluon/invariants/skills/systematic-debugging.md`
- Modify: `crates/hadron-gluon/invariants/skills/test-driven-development.md`
- Modify: `crates/hadron-gluon/invariants/skills/brainstorming.md`

**Interfaces:**
- Consumes: Skill markdown files
- Produces: Compact, high-density machine prompt execution skills

- [ ] **Step 1: Distill `subagent-driven-development.md`**

Strip human tutorial prose, long conversational explanations, and large ASCII block diagrams. Retain 100% of subagent dispatch rules, checklist steps, and review gates. Target size: ~7.5 KB.

- [ ] **Step 2: Distill `writing-skills.md`**

Strip meta-tutorial prose and long example blocks. Retain exact skill structure requirements, YAML schema rules, and verification checklist. Target size: ~8.0 KB.

- [ ] **Step 3: Distill `systematic-debugging.md`**

Retain 4-phase investigation workflow (Root Cause, Reproduce, Fix, Verify) and empirical log inspection rules. Target size: ~4.5 KB.

- [ ] **Step 4: Distill `test-driven-development.md`**

Retain Red-Green-Refactor enforcement & failing test output proof requirements. Target size: ~4.5 KB.

- [ ] **Step 5: Distill `brainstorming.md`**

Retain 9-step checklist, hard gate, and visual companion rules; strip tutorial prose. Target size: ~3.5 KB.

---

### Task 3: Distill Remaining 10 Workflow Skills

**Files:**
- Modify: `crates/hadron-gluon/invariants/skills/dispatching-parallel-agents.md`
- Modify: `crates/hadron-gluon/invariants/skills/executing-plans.md`
- Modify: `crates/hadron-gluon/invariants/skills/finishing-a-development-branch.md`
- Modify: `crates/hadron-gluon/invariants/skills/receiving-code-review.md`
- Modify: `crates/hadron-gluon/invariants/skills/requesting-code-review.md`
- Modify: `crates/hadron-gluon/invariants/skills/reviewing-work.md`
- Modify: `crates/hadron-gluon/invariants/skills/using-git-worktrees.md`
- Modify: `crates/hadron-gluon/invariants/skills/using-superpowers.md`
- Modify: `crates/hadron-gluon/invariants/skills/verification-before-completion.md`
- Modify: `crates/hadron-gluon/invariants/skills/writing-plans.md`

**Interfaces:**
- Consumes: Skill markdown files
- Produces: Compact, high-density machine prompt execution skills

- [ ] **Step 1: Compact remaining 10 skills**

Distill conversational preambles and redundant text across the remaining 10 skill files while keeping all procedural steps, commands, and output schemas intact.

---

### Task 4: Unit Test Updates & Gate Verification

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/prompt/tests.rs`

**Interfaces:**
- Consumes: `crates/hadron-gluon/src/adapter/prompt/mod.rs`
- Produces: Passing unit test suite in `hadron-gluon`

- [ ] **Step 1: Update prompt tests in `tests.rs`**

Update section ordering assertions in `prompt_renders_active_skill_immediately_before_your_task`, `prompt_cache_prefix_strictly_identical_across_dynamic_turn_variations`, and `measure_and_build_agree_on_section_boundaries` to match the new cache-stable prefix order.

- [ ] **Step 2: Run `cargo test -p hadron-gluon`**

Run: `cargo test -p hadron-gluon`
Expected: PASS (all tests green)

- [ ] **Step 3: Run full workspace check**

Run: `cargo check --workspace`
Expected: PASS with 0 errors.

- [ ] **Step 4: Commit implementation**

```bash
git add crates/hadron-gluon/src/adapter/prompt/ crates/hadron-gluon/invariants/skills/
git commit -m "feat(prompt): optimize prompt cache prefix and distill skill payloads"
```
