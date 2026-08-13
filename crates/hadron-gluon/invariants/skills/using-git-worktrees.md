---
name: using-git-worktrees
description: Use when starting feature work that needs isolation from current workspace or before executing implementation plans - ensures an isolated workspace exists via native tools or git worktree fallback
---

# Using Git Worktrees

## Core Principle

Detect existing isolation first → Prefer native harness tools → Fall back to manual git worktrees.

## Execution Steps

### Step 0: Detect Existing Workspace Isolation

Inspect git directory structure:

```bash
GIT_DIR=$(cd "$(git rev-parse --git-dir)" 2>/dev/null && pwd -P)
GIT_COMMON=$(cd "$(git rev-parse --git-common-dir)" 2>/dev/null && pwd -P)
```

**Submodule Guard:** Verify directory is not a git submodule:

```bash
git rev-parse --show-superproject-working-tree 2>/dev/null
```

- **If `GIT_DIR != GIT_COMMON` (and not a submodule):** Already in an isolated worktree. Skip to Step 2 (Project Setup). Do NOT create another worktree.
- **If `GIT_DIR == GIT_COMMON`:** In normal repository checkout. Request user consent before creating worktree (unless preference already declared in instructions). If consent declined, work in place and proceed to Step 2.

### Step 1: Create Isolated Workspace

#### 1a. Native Worktree Tools (Preferred)

If platform provides native worktree tools (`EnterWorktree`, `WorktreeCreate`, `/worktree`), invoke the native tool directly and proceed to Step 2.

#### 1b. Manual Git Worktree Fallback

Only use if no native tool is available.

1. **Location Priority:**
    - Explicit directory in user instructions.
    - Existing `.hadron/trees/` or `.hadron/worktrees/` directory (or `worktrees/`).
    - Default: `.hadron/trees/` (or `.hadron/worktrees/`) at repository root.

2. **Gitignore Safety Verification:**

    ```bash
    git check-ignore -q .hadron 2>/dev/null || git check-ignore -q .hadron/trees 2>/dev/null || git check-ignore -q .worktrees 2>/dev/null
    ```

    _If NOT ignored:_ Add location to `.gitignore` and commit before creation.

3. **Creation:**
    ```bash
    path="$LOCATION/$BRANCH_NAME"
    git worktree add "$path" -b "$BRANCH_NAME"
    cd "$path"
    ```
    _Sandbox Fallback:_ If blocked by sandbox permissions, report error and work in current directory.

### Step 2: Project Setup

Detect stack and run setup:

```bash
if [ -f package.json ]; then npm install; fi
if [ -f Cargo.toml ]; then cargo build; fi
if [ -f requirements.txt ]; then pip install -r requirements.txt; fi
if [ -f go.mod ]; then go mod download; fi
```

### Step 3: Verify Clean Baseline

Execute test suite to confirm clean baseline before starting code changes:

```bash
npm test / cargo test / pytest / go test ./...
```

- **If tests fail:** Report baseline failures and ask whether to proceed or investigate.
- **If tests pass:** Report workspace ready format:
    ```text
    Worktree ready at <path>
    Tests passing (<N> tests, 0 failures)
    Ready to implement <feature-name>
    ```

## Mandatory Rules & Red Flags

- **NEVER** create nested worktrees if Step 0 confirms existing worktree isolation.
- **NEVER** bypass native worktree tools (e.g. `EnterWorktree`) when available.
- **NEVER** create project-local worktree without verifying it is listed in `.gitignore`.
- **ALWAYS** run project setup and baseline test verification before modifying code.
