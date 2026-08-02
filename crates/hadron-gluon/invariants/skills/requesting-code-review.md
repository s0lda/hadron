---
name: requesting-code-review
description: Use when completing tasks, implementing major features, or before merging to verify work meets requirements
---

# Requesting Code Review

## Core Principle
Dispatch a code reviewer subagent with isolated context to audit completed work against plan specifications before merging or advancing.

## Applicability

### Mandatory
- After completing each task in subagent-driven development.
- After completing a major feature.
- Prior to merging into target base branch (`main` / `master`).

### Optional
- When blocked or seeking fresh perspective on complex refactoring.

## Execution Sequence

### Step 1: Determine Git Revision Range
```bash
BASE_SHA=$(git rev-parse HEAD~1)  # or origin/main
HEAD_SHA=$(git rev-parse HEAD)
```

### Step 2: Dispatch Code Reviewer Subagent
Dispatch a `general-purpose` subagent using this exact prompt contract:

```text
You are a Senior Code Reviewer. Review completed work against plan/requirements.

## What Was Implemented
[DESCRIPTION — concise summary of changes]

## Requirements / Plan
[PLAN_OR_REQUIREMENTS — plan file path or task spec]

## Git Range to Review
Base: [BASE_SHA]   Head: [HEAD_SHA]
Inspect: git diff --stat [BASE_SHA]..[HEAD_SHA] and git diff [BASE_SHA]..[HEAD_SHA]

## Read-Only Review Invariant
Read-only checkout. Do NOT mutate working tree, index, HEAD, or branch. Use git show/diff/log.

## Evaluation Checklist
- Plan Alignment: Implementation matches plan specifications; deviations justified.
- Code Quality: Separation of concerns, error handling, type safety, DRY.
- Architecture: Sound design, performance, security, clean integration.
- Testing: Tests verify real behavior (no mocks), edge cases covered, all passing.
- Production Readiness: Migration/backward-compat, docs, zero obvious bugs.

## Output Contract Format
### Strengths — [specific well-built items]
### Issues
#### Critical (Must Fix) — bugs, security flaws, data loss, broken functionality
#### Important (Should Fix) — architecture flaws, missing features, test gaps
#### Minor (Nice to Have) — style, optimization, documentation
For each issue: file:line, what is wrong, why it matters, how to fix.
### Recommendations — quality/process improvements
### Assessment
Ready to merge? [Yes | No | With fixes]
Reasoning: [1-2 sentence technical assessment]
```

### Step 3: Triage and Act on Findings
- **Critical Issues**: Fix immediately before any further progress.
- **Important Issues**: Fix before proceeding to subsequent plan tasks.
- **Minor Issues**: Note for polish or defer.
- **Pushback Protocol**: If reviewer assessment is flawed, push back with empirical code/test evidence.

## Invariants & Red Flags
- **NEVER** skip code review because changes seem "simple".
- **NEVER** ignore Critical or Important issues reported by reviewer.
- **ALWAYS** enforce read-only execution constraints on reviewer subagents.
