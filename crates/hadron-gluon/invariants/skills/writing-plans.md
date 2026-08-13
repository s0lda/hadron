---
name: writing-plans
description: Use when you have a spec or requirements for a multi-step task, before touching code
---

# Writing Plans

## Core Principle
Write comprehensive, bite-sized implementation plans with explicit file paths, exact code blocks, and verification steps.

**Save path:** `.hadron/docs/plans/YYYY-MM-DD-<feature-name>.md`

## Pre-Plan Checks & Architecture
1. **Scope Check:** If spec spans multiple independent subsystems, split into separate sub-project plans.
2. **File Structure Mapping:** Map all files to create/modify before defining tasks. Maintain clean component boundaries.
3. **Task Right-Sizing:** Each task represents the smallest independently testable deliverable (2-5 min execution steps).

## Mandatory Plan Header Template
Every plan document MUST begin with this header:

```markdown
---
author: <your quark id>
status: draft
---

# [Feature Name] Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use Swarm Quark Dispatch (recommended) or subagent-driven-development or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** [One sentence describing what this builds]

**Architecture:** [2-3 sentences about approach]

**Tech Stack:** [Key technologies/libraries]

## Global Constraints

[The spec's project-wide requirements — version floors, dependency limits,
naming and copy rules, platform requirements — one line each, with exact
values copied verbatim from the spec. Every task's requirements implicitly
include this section.]

---
```

## Mandatory Task Structure

````markdown
### Task N: [Component Name]

**Files:**
- Create: `exact/path/to/file.ext`
- Modify: `exact/path/to/existing.ext:123-145`
- Test: `tests/exact/path/to/test.ext`

**Interfaces:**
- Consumes: [Exact function signatures and types from earlier tasks]
- Produces: [Exact function names, parameter and return types exported for later tasks]

- [ ] **Step 1: Write failing test**
```python
def test_feature():
    assert feature() == expected
```

- [ ] **Step 2: Verify test fails**
Run: `pytest tests/path/test.py`
Expected: FAIL with "feature not defined"

- [ ] **Step 3: Implement minimal code**
```python
def feature():
    return expected
```

- [ ] **Step 4: Verify test passes**
Run: `pytest tests/path/test.py`
Expected: PASS

- [ ] **Step 5: Commit**
```bash
git add tests/path/test.py src/path/file.py
git commit -m "feat: add specific feature"
```
````

## No Placeholders Invariant
Never include:
- "TBD", "TODO", "implement later", "add validation".
- Vague testing instructions without actual test code.
- References to functions or types not defined in plan interfaces.
- References like "similar to Task N" (always supply complete code blocks).

## Self-Review Checklist
Before saving:
- [ ] **Spec Coverage:** Verify every spec requirement maps to a specific task.
- [ ] **Placeholder Scan:** Verify zero TBD/vague steps or missing code blocks exist.
- [ ] **Type Consistency:** Confirm function signatures match across all tasks.

## Execution Handoff Options
Offer execution options:
1. **Swarm Quark Dispatch (Recommended for Hadron Swarms):** Dispatch independent tasks to available worker Quarks (`@<quark-id> <task>`), review each result, then integrate.
2. **Subagent-Driven:** `subagent-driven-development`
3. **Inline Execution:** `executing-plans`
