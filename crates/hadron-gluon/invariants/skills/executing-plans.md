---
name: executing-plans
description: Use when you have a written implementation plan to execute in a separate session with review checkpoints
---

# Executing Plans

## Overview
Load plan, review critically, execute all tasks, report when complete.

**Announce at start:** "I'm using the executing-plans skill to implement this plan."

> **Recommendation:** If subagents are available, prefer `superpowers:subagent-driven-development`.

## Execution Workflow

### Step 1: Load and Review Plan
1. Load plan, review critically - identify any questions or concerns about the plan before starting.
2. If concerns exist: raise them with human partner before starting.
3. If clear: create todo list per task item.

### Step 2: Task-by-Task Execution
For each task in the plan:
1. Mark task status as `in_progress`.
2. Follow bite-sized steps exactly without jumping ahead or skipping code blocks.
3. Execute required verification commands (`cargo test`, `pytest`, etc.).
4. Mark task status as `completed`.

### Step 3: Branch Completion
When all tasks are complete and verified:
- **Announce:** "I'm using the finishing-a-development-branch skill to complete this work."
- **REQUIRED SUB-SKILL:** Execute `superpowers:finishing-a-development-branch`.

## Stop Conditions (Hard Gates)
STOP execution immediately and request clarification when:
- Hitting a blocker (missing dependency, test failure, unclear instruction).
- Verification commands fail repeatedly.
- Fundamental approach requires rethinking.

## Critical Invariants
- **Branch Protection:** Never start implementation on `main` or `master` branch without explicit user consent.
- **Strict Verification:** Never skip verification steps specified in task steps.

## Workflow Integration
- **Workspace Isolation:** `superpowers:using-git-worktrees`
- **Plan Authoring:** `superpowers:writing-plans`
- **Branch Finalization:** `superpowers:finishing-a-development-branch`
