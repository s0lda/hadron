---
name: finishing-a-development-branch
description: Use when implementation is complete, all tests pass, and you need to decide how to integrate the work - guides completion of development work by presenting structured options for merge, PR, or cleanup
---

# Finishing a Development Branch

## Core Principle

Verify tests → Detect environment → Present options → Execute choice → Clean up.

## Execution Steps

### Step 1: Verify Test Suite

Execute full test suite before presenting completion options:

```bash
npm test / cargo test / pytest / go test ./...
```

- **If tests fail:** STOP. Display failures. Do NOT present integration options until test suite is green.
- **If tests pass:** Proceed to Step 2.

### Step 2: Detect Environment

```bash
GIT_DIR=$(cd "$(git rev-parse --git-dir)" 2>/dev/null && pwd -P)
GIT_COMMON=$(cd "$(git rev-parse --git-common-dir)" 2>/dev/null && pwd -P)
```

| Environment                             | Menu Type            | Worktree Cleanup   |
| --------------------------------------- | -------------------- | ------------------ |
| `GIT_DIR == GIT_COMMON` (normal repo)   | Standard (4 options) | None required      |
| `GIT_DIR != GIT_COMMON` (named branch)  | Standard (4 options) | Harness-managed    |
| `GIT_DIR != GIT_COMMON` (detached HEAD) | Detached (3 options) | Externally managed |

### Step 3: Determine Base Branch

```bash
git merge-base HEAD main 2>/dev/null || git merge-base HEAD master 2>/dev/null
```

### Step 4: Present Structured Menu

**Standard Menu (4 options):**

```text
Implementation complete. What would you like to do?

1. Merge back to <base-branch> locally
2. Push and create a Pull Request
3. Keep the branch as-is (I'll handle it later)
4. Discard this work

Which option?
```

**Detached HEAD Menu (3 options):**

```text
Implementation complete. You're on a detached HEAD (externally managed workspace).

1. Push as new branch and create a Pull Request
2. Keep as-is (I'll handle it later)
3. Discard this work

Which option?
```

### Step 5: Execute Selected Option

#### Option 1: Merge Locally

```bash
MAIN_ROOT=$(git -C "$(git rev-parse --git-common-dir)/.." rev-parse --show-top-level)
cd "$MAIN_ROOT"
git checkout <base-branch> && git pull && git merge <feature-branch>
<run test suite>
git branch -d <feature-branch>
```

#### Option 2: Push and Create PR

```bash
git push -u origin <feature-branch>
```

_Note: Do NOT remove worktree (user requires it for PR feedback)._

#### Option 3: Keep As-Is

Report branch state and worktree path. Do not perform cleanup.

#### Option 4: Discard Work

Require user to explicitly type `"discard"` before proceeding:

```text
This will permanently delete branch <name> and all unmerged commits. Type 'discard' to confirm.
```

Upon confirmation:

```bash
MAIN_ROOT=$(git -C "$(git rev-parse --git-common-dir)/.." rev-parse --show-top-level)
cd "$MAIN_ROOT"
git branch -D <feature-branch>
```

### Step 6: Hadron Worktree Safety Invariant

In Hadron, worktrees live under `.hadron/trees/<quark_id>`. Hadron Gluon harness owns and recycles all worktrees. **Quarks must NEVER execute `git worktree remove` or delete worktree directories directly.**

## Red Flags & Mandatory Rules

- **NEVER** present integration options while tests are failing.
- **NEVER** delete code/branches without typed `"discard"` confirmation.
- **NEVER** run `git worktree remove` inside `.hadron/trees/<quark_id>`.
- **ALWAYS** re-verify tests after merging locally into base branch.
