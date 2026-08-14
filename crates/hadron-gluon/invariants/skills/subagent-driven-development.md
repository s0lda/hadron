---
name: subagent-driven-development
description: Use when executing implementation plans with independent tasks in the current session
---

# Subagent-Driven Development

Execute an implementation plan by leveraging the Hadron Swarm (Quark Dispatch) alongside Subagents for maximum parallelism, quality, and speed.

**Core Principle:** Quark Swarm Dispatch (`@<quark-id>`) across worktrees is Hadron's primary mechanism for parallel task execution. The Orchestrator is the single swarm dispatcher, and worker Quarks communicate via Hub-and-Spoke with `@orchestrator`. Subagents can be deployed by the Orchestrator (for isolated research/verification or fallback) AND by Worker Quarks (to break down tasks into subagent workflows).

**Execution Directives:**
- **Swarm Priority (Orchestrator):** The Orchestrator is the single swarm dispatcher. If worker Quarks are active and free, Orchestrator delegates tasks via `@<quark-id> <task>` first. Quarks run concurrently across isolated worktrees.
- **Hub-and-Spoke Communication:** Worker Quarks communicate exclusively with `@orchestrator`. Workers do NOT dispatch peer worker Quarks directly (peers cannot see isolated worktree diffs). Workers report completion or escalate blockers, assistance requests, and research needs to `@orchestrator`.
- **Subagent Flexibility:** Both Orchestrator and worker Quarks may freely invoke internal subagents within their own runtime to explore codebases, implement sub-components, or run verification in parallel.
- **Continuous Execution:** Do NOT pause to check in with the human partner between tasks in Bypass mode. Drive all plan tasks to 100% completion autonomously.

## Pre-Flight Plan Review

Before dispatching Task 1, scan the plan once for contradictions or global constraint conflicts. If clean, proceed directly to dispatching tasks across Quarks and subagents.

## Model Selection Rules

Always specify `Model` explicitly when invoking a subagent (omitted model inherits session default).
- **Cheap tier:** Mechanical single-file implementations or spec-to-code transcriptions.
- **Standard tier:** Multi-file integration tasks, debugging, and task reviewers.
- **Most Capable tier:** Architecture decisions, design tasks, and final whole-branch reviewer.

*Rule:* Turn count beats token price. Use standard tier as floor for reviewers and prose implementers.

## Workflow Execution Steps

### 1. File Handoffs & File Paths
Do NOT paste large specs, diffs, or accumulated history into subagent prompts. Pass artifacts as file paths:
- **Task Brief File:** Extract task text from plan into `.hadron/scratch/task-N-brief.md`.
- **Report File:** Implementer writes full report to `.hadron/scratch/task-N-report.md`.
- **Diff Package File:** For reviews, redirect `git log --oneline BASE..HEAD`, `git diff --stat BASE..HEAD`, and `git diff -U10 BASE..HEAD` into `.hadron/scratch/task-N-diff.txt`. Pass path to reviewer. (Use recorded `BASE`, NEVER `HEAD~1`).

### 2. Progress Ledger (`.hadron/nucleus/agents/progress.md`)
- Check for existing ledger at start; resume from first incomplete task.
- Upon clean review, append line: `Task N: complete (commits <base7>..<head7>, review clean)`.
- Ledger is primary recovery map across context compactions.

### 3. Handling Implementer Status
- **DONE:** Generate diff package file using recorded `BASE` SHA -> dispatch task reviewer.
- **DONE_WITH_CONCERNS:** Evaluate concerns. If correctness/scope issue, resolve before review; if minor observation, proceed to review.
- **NEEDS_CONTEXT:** Provide requested context -> re-dispatch.
- **BLOCKED:** Assess blocker. Provide context, upgrade model, split task, or escalate to human partner if plan is broken. Never retry blindly.

### 4. Reviewer ⚠️ Items & Pre-Judging Rules
- Resolve "⚠️ Cannot verify from diff" items yourself against unchanged code before marking task complete.
- **NO PRE-JUDGING:** Never instruct reviewer to ignore issues, rate at most Minor, or skip checks.
- Hand reviewer exact Global Constraints verbatim from spec/plan.
- Dispatch fix subagents for Critical/Important findings. Record Minor findings in progress ledger.
- For plan-mandated defects: present finding and plan text to human partner to decide.

## Prompt Templates

### Implementer Subagent Template
```
You are implementing Task N: [task name].

## Task Description
Read task brief first: [BRIEF_FILE] — full task text and exact requirements/values.

## Context
[Where task fits, dependencies, architectural decisions from earlier tasks]

## Before You Begin
If requirements or approach are unclear — ask before starting work.

## Your Job
1. Implement spec exactly.
2. Write tests following TDD if specified.
3. Verify implementation works.
4. Commit changes.
5. Perform self-review (Completeness, Quality, Discipline, Testing).
6. Report results.
Work in: [directory].

## Report Contract
Write full report to [REPORT_FILE]. Return back ONLY (under 15 lines):
- Status: DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT
- Commits (short SHA + subject)
- One-line test summary
- Concerns (if any)
- Report file path
```

### Task Reviewer Subagent Template
```
You are reviewing Task N implementation: spec compliance and code quality.

## What Was Requested
Read task brief: [BRIEF_FILE]
Global constraints: [GLOBAL_CONSTRAINTS]

## What Implementer Claims
Read report: [REPORT_FILE]

## Diff Under Review
Base: [BASE_SHA] Head: [HEAD_SHA] Diff file: [DIFF_FILE]
Read diff file once. Do not re-run git commands or crawl unchanged code unless verifying a concrete named risk. Read-only execution.

## Tests
Do not re-run full test suite. Run a test only if reading code raises specific doubt unaddressed by report. Warnings/stray logs in output are findings.

## Part 1: Spec Compliance
Check Missing, Extra, Misunderstood. Report unverifiable items as ⚠️.

## Part 2: Code Quality
Check separation of concerns, error handling, edge cases, real test behavior vs mocks. Cite file:line for all findings.

## Calibration
Critical = must fix; Important = incorrect/fragile behavior, missed requirement, duplicated logic block, swallowed errors, empty assertions; Minor = polish/broad coverage.

## Output Format
### Spec Compliance
- ✅ Spec compliant | ❌ Issues found: [details, file:line]
- ⚠️ Cannot verify from diff: [items]
### Strengths — [specific]
### Issues — #### Critical / #### Important / #### Minor
### Assessment — Task quality: [Approved | Needs fixes] + 1-2 sentence reason.
```

## Final Whole-Branch Review

After all tasks complete:
1. Generate whole-branch review package against `MERGE_BASE` (`git merge-base main HEAD`).
2. Pass package and Minor findings list from progress ledger to `requesting-code-review` skill on most capable model.
3. If findings returned, dispatch ONE fix subagent covering all findings, then verify.
4. Finish branch using `finishing-a-development-branch`.

## Red Flags - FORBIDDEN Actions

- Starting on main/master branch without explicit user consent.
- Skipping task review or accepting report missing either verdict (Spec Compliance AND Code Quality).
- Using `HEAD~1` instead of recorded task `BASE` for diff generation.
- Pasting accumulated prior task history into subagent prompts.
- Pre-judging reviewer findings or instructing reviewer what to ignore.
- Moving to next task with open Critical or Important issues.
- Re-dispatching tasks already marked complete in progress ledger.

## Integration Skills

- `using-git-worktrees` - Isolated workspace setup.
- `writing-plans` - Plan creation.
- `requesting-code-review` - Final review template.
- `finishing-a-development-branch` - Branch integration.
- `test-driven-development` - Subagent implementation TDD.
