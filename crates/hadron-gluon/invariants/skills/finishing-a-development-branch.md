---
name: finishing-a-development-branch
description: Use when implementation is complete, all tests pass, and you need to decide how to integrate the work - guides completion of development work by presenting structured options for merge, PR, or cleanup
---

# Finishing a Development Branch

## Core Principle

Verify tests → Commit clean changes → Rely on Hadron Gluon Merge Gate (or follow configured settings).

## Execution Steps

### Step 1: Verify Test Suite

Execute the full test suite before completing work:

```bash
cargo test --workspace / npm test / pytest / go test ./...
```

- **If tests fail:** STOP. Fix failures. Do NOT claim completion until the test suite is green.
- **If tests pass:** Proceed to Step 2.

### Step 2: Hadron Automated Merge Gate Workflow

In Hadron, quarks work in dedicated worktrees (`.hadron/trees/<quark_id>`). 
- **Autonomous Merge Gate:** The Gluon engine automatically tests, rebases, and lands the branch on the base branch according to the configured merge strategy in project settings (`team.json`).
- **Quark Responsibility:** Once tests pass, commit all changes with a structured commit message, update the active plan checkbox (`- [x] Task N (commit <hash>)`), and report the outcome with test evidence (Standard Model Rule 6).
- **Do NOT present a blocking 4-option menu** in Bypass/Auto modes — the Hadron engine handles merge gating automatically.

### Step 3: Manual / Interactive Integration (Non-Hadron or Ask Mode)

When operating outside Hadron or when explicit interactive branch resolution is requested in Ask mode, check user settings/preferences:

1. **Auto-Merge / Fast-Forward:** Rebase onto base branch and fast-forward merge if tests pass.
2. **Pull Request:** Push feature branch and output PR link.
3. **Keep / Discard:** Retain branch or discard only upon explicit user confirmation (`"discard"`).

### Step 4: Hadron Worktree Safety Invariant

In Hadron, worktrees live under `.hadron/trees/<quark_id>`. Hadron Gluon harness owns and recycles all worktrees. **Quarks must NEVER execute `git worktree remove` or delete worktree directories directly.**

## Red Flags & Mandatory Rules

- **NEVER** present integration options while tests are failing.
- **NEVER** delete code/branches without typed `"discard"` confirmation.
- **NEVER** run `git worktree remove` inside `.hadron/trees/<quark_id>`.
- **ALWAYS** re-verify tests after merging locally into base branch.
