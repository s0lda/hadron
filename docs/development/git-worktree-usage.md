# 🌿 Git Worktree Isolation & Merge Gate

Hadron isolates every agent task inside dedicated git worktrees, protecting working directory state and verifying code before merging.

---

## Worktree Isolation

When an agent turn begins:
1. `hadron-gluon` creates a temporary git worktree under `.hadron/trees/<turn_id>`.
2. A dedicated git branch (e.g. `quark/<agent_name>/<turn_id>`) is created off the current HEAD.
3. The agent executes all code modifications, file operations, and terminal commands strictly within its isolated worktree directory.
4. Changes made in one worktree do not affect other running agents or the primary workspace branch.

---

## The Merge Gate

When an agent completes its turn, it submits a merge intent back to `field.jsonl`. `hadron-gluon` executes the **Merge Gate**:

```text
 Agent Turn Complete
         │
         ▼
 Rebase branch onto main
         │
         ▼
 Run native project test suite (cargo test, npm test, etc.)
         │
 ┌───────┴───────┐
 │               │
 PASS           FAIL
 │               │
 ▼               ▼
Fast-Forward    Refuse Merge &
onto main       Log Failure Event
```

### Key Verification Rules:
- **Automatic Rebase**: The branch is rebased on the latest `main` commit.
- **Test Gate Enforcement**: The workspace test suite is executed under a strict execution deadline.
- **Fast-Forward Merge**: If all tests pass, `hadron-gluon` fast-forwards `main` to include the verified changes.
- **Refusal on Failure**: If compilation or unit tests fail, the merge is refused, preserving main branch stability.
