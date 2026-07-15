---
name: requesting-code-review
description: Use when completing tasks, implementing major features, or before merging to verify work meets requirements
---

# Requesting Code Review

Dispatch a code reviewer subagent to catch issues before they cascade. The reviewer gets precisely crafted context for evaluation — never your session's history. This keeps the reviewer focused on the work product, not your thought process, and preserves your own context for continued work.

**Core principle:** Review early, review often.

## When to Request Review

**Mandatory:**
- After each task in subagent-driven development
- After completing major feature
- Before merge to main

**Optional but valuable:**
- When stuck (fresh perspective)
- Before refactoring (baseline check)
- After fixing complex bug

## How to Request

**1. Get git SHAs:**
```bash
BASE_SHA=$(git rev-parse HEAD~1)  # or origin/main
HEAD_SHA=$(git rev-parse HEAD)
```

**2. Dispatch a `general-purpose` code reviewer subagent** with this prompt
(fill the bracketed placeholders):

```
You are a Senior Code Reviewer with expertise in software architecture,
design patterns, and best practices. Review completed work against its plan
or requirements and identify issues before they cascade.

## What Was Implemented
[DESCRIPTION — brief summary of what was built]

## Requirements / Plan
[PLAN_OR_REQUIREMENTS — what it should do: plan path, task text, or requirements]

## Git Range to Review
Base: [BASE_SHA]   Head: [HEAD_SHA]
Inspect with: git diff --stat [BASE_SHA]..[HEAD_SHA] and git diff [BASE_SHA]..[HEAD_SHA]

## Read-Only Review
Your review is read-only on this checkout. Do not mutate the working tree,
index, HEAD, or branch state. Use git show/diff/log to inspect history. To
inspect another revision, check it out into a separate temp dir
(git worktree add) — never move HEAD on this checkout.

## What to Check
- Plan alignment: implementation matches the plan; deviations are justified,
  not problematic; all planned functionality present.
- Code quality: clean separation of concerns, proper error handling, type
  safety where applicable, DRY without premature abstraction, edge cases.
- Architecture: sound design, reasonable performance/scalability, security,
  clean integration with surrounding code.
- Testing: tests verify real behavior (not mocks), edge cases covered,
  integration tests where they matter, all tests passing.
- Production readiness: migration/backward-compat if schema changed, docs,
  no obvious bugs.

## Calibration
Categorize by ACTUAL severity — not everything is Critical. Acknowledge what
was done well before listing issues (accurate praise earns trust). Flag
significant plan deviations specifically so the implementer can confirm
intent. If the problem is with the plan itself rather than the code, say so.

## Output Format
### Strengths — [specific, what's well done]
### Issues
#### Critical (Must Fix) — bugs, security, data-loss, broken functionality
#### Important (Should Fix) — architecture problems, missing features, poor
    error handling, test gaps
#### Minor (Nice to Have) — style, optimization, doc polish
For each issue: file:line, what's wrong, why it matters, how to fix.
### Recommendations — improvements for quality/architecture/process
### Assessment
Ready to merge? [Yes | No | With fixes]
Reasoning: [1-2 sentence technical assessment]

## Critical Rules
DO: categorize by real severity; be specific (file:line, not vague); explain
WHY each issue matters; acknowledge strengths; give a clear verdict.
DON'T: say "looks good" without checking; mark nitpicks Critical; review code
you didn't read; be vague ("improve error handling"); dodge the verdict.
```

**Reviewer returns:** Strengths, Issues (Critical / Important / Minor),
Recommendations, and a clear merge verdict.

**3. Act on feedback:**
- Fix Critical issues immediately
- Fix Important issues before proceeding
- Note Minor issues for later
- Push back if reviewer is wrong (with reasoning)

## Example

```
[Just completed Task 2: Add verification function]

You: Let me request code review before proceeding.

BASE_SHA=$(git log --oneline | grep "Task 1" | head -1 | awk '{print $1}')
HEAD_SHA=$(git rev-parse HEAD)

[Dispatch code reviewer subagent]
  DESCRIPTION: Added verifyIndex() and repairIndex() with 4 issue types
  PLAN_OR_REQUIREMENTS: Task 2 from docs/superpowers/plans/deployment-plan.md
  BASE_SHA: a7981ec
  HEAD_SHA: 3df7661

[Subagent returns]:
  Strengths: Clean architecture, real tests
  Issues:
    Important: Missing progress indicators
    Minor: Magic number (100) for reporting interval
  Assessment: Ready to proceed

You: [Fix progress indicators]
[Continue to Task 3]
```

## Integration with Workflows

**Subagent-Driven Development:**
- Review after EACH task
- Catch issues before they compound
- Fix before moving to next task

**Executing Plans:**
- Review after each task or at natural checkpoints
- Get feedback, apply, continue

**Ad-Hoc Development:**
- Review before merge
- Review when stuck

## Red Flags

**Never:**
- Skip review because "it's simple"
- Ignore Critical issues
- Proceed with unfixed Important issues
- Argue with valid technical feedback

**If reviewer wrong:**
- Push back with technical reasoning
- Show code/tests that prove it works
- Request clarification
