---
name: brainstorming
description: "You MUST use this before any creative work - creating features, building components, adding functionality, or modifying behavior. Explores user intent, requirements and design before implementation."
---

# Brainstorming Ideas Into Designs

Turn ideas into validated designs and specs through structured collaborative dialogue before implementation.

<HARD-GATE>
Do NOT invoke any implementation skill, write any code, scaffold any project, or take any implementation action until you have presented a design and the user has approved it. This applies to EVERY project regardless of perceived simplicity.
</HARD-GATE>

## Mandatory Rule

Every project must go through this process regardless of size (todo lists, utilities, config changes). Simple projects carry hidden assumptions. The design may be brief for simple tasks, but MUST be presented and approved.

## Checklist

Create a task for each item and complete sequentially:

1. **Explore project context** — check files, docs, recent commits
2. **Ask clarifying questions** — one at a time, understand purpose/constraints/success criteria
3. **Propose 2-3 approaches** — with trade-offs and explicit recommendation
4. **Present design** — in sections scaled to complexity, get user approval after each section
5. **Write design doc** — save to `.hadron/docs/specs/YYYY-MM-DD-<topic>-design.md` and commit
6. **Spec self-review** — inline check for placeholders, contradictions, ambiguity, scope
7. **User reviews written spec** — ask user to review the spec file before proceeding
8. **Transition to implementation** — invoke `writing-plans` skill to create implementation plan

The ONLY skill to invoke after brainstorming is `writing-plans`. Do NOT invoke implementation skills directly.

## Execution Rules

### 1. Scope & Understanding
- Check current codebase state (files, docs, recent commits) first.
- If request spans multiple independent subsystems, flag immediately and decompose into sub-projects before detailing questions.
- Ask clarifying questions **one at a time**. Prefer multiple-choice options when possible.
- Focus on: purpose, constraints, and success criteria.

### 2. Proposing Approaches
- Always offer 2-3 approaches with trade-offs.
- Lead with recommended option and explain rationale.

### 3. Presenting Design
- Scale each section to complexity (few sentences if simple, 200-300 words if complex).
- Confirm user approval after each section.
- Cover: architecture, components, data flow, error handling, testing.
- Enforce modular boundaries: each unit must have one clear purpose, well-defined interfaces, and independent testability.

### 4. Working in Existing Codebases
- Follow existing patterns. Include targeted structural improvements if pre-existing flaws affect current work.
- Avoid unrelated refactoring.

## Post-Design Artifacts & Gates

### Documentation
- Save validated spec to `.hadron/docs/specs/YYYY-MM-DD-<topic>-design.md` (or user-specified location).
- Commit design doc to git.

### Spec Self-Review Loop
1. **Placeholder scan:** Fix any "TBD", "TODO", or vague items inline.
2. **Internal consistency:** Verify architecture matches feature descriptions.
3. **Scope check:** Ensure spec is focused enough for single plan.
4. **Ambiguity check:** Resolve any dual-interpretation requirements explicitly.

### User Review Gate
Ask user explicitly before proceeding to plan:
> "Spec written and committed to `<path>`. Please review it and let me know if you want to make any changes before we start writing out the implementation plan."

If changes are requested, update spec, re-run self-review, and re-request approval.

### Implementation Handoff
Invoke `writing-plans` skill. Do NOT invoke implementation skills directly.

## Key Principles
- One question per message.
- Prefer multiple choice options.
- Ruthless YAGNI.
- 2-3 alternative approaches per design.
- Incremental section approval.
