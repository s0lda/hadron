# Autonomous Bypass Orchestration & Active Plan State Updates Design

**Date**: 2026-07-22  
**Author**: @Agy  
**Status**: Approved

## 1. Context & Problem Statement
In **Bypass Mode** (full autonomous execution), Hadron is designed to run complex multi-task goals end-to-end without requiring human intervention at every sub-step. However, two key gaps exist:
1. **Interactive Prompt Habit**: The Orchestrator prompt (`crates/hadron-gluon/src/adapter/prompt/mod.rs`) does not differentiate execution loops between Ask/Write Mode and Bypass Mode. As a result, the Orchestrator stops to ask the human for options (e.g. *"Option 1 or Option 2?"*) or pauses after a worker finishes.
2. **Stale Plan Files**: Active plan files in `.hadron/docs/plans/*.md` are never updated on disk as tasks complete. Tasks remain unchecked (`- [ ]`), causing `hadron-chamber`'s Plan tab to show `0% completed` and leaving the Orchestrator unable to track progress across turns.

---

## 2. Core Architecture & Requirements

### 2.1 Active Plan File as State SSOT
- **Format**: All implementation plans use GitHub Flavored Markdown checkboxes (`- [ ] Task N: ...`).
- **On-Disk Updates**: Upon verifying a completed task (via git log / test gate output), the Orchestrator updates the active plan file on disk, changing `- [ ] Task N` to `- [x] Task N (completed in commit <hash>)` and commits the plan file edit.
- **GUI Sync**: `hadron-chamber`'s Right Rail Plan tab parses progress directly from disk (`- [x]` vs `- [ ]`), enabling live progress bar and task list updates in real time.

### 2.2 Bypass Mode Autonomous Directives
In `crates/hadron-gluon/src/adapter/prompt/mod.rs`, prompt generation is updated with explicit mode guidance for the Orchestrator:
- **In Bypass Mode**:
  1. **Auto-Select Strategy**: Automatically select the optimal execution path (e.g., subagent or parallel worker fan-out) without prompting the human.
  2. **Active Task Execution Loop**: Scan the active plan for the first incomplete task (`- [ ]`). Dispatch worker(s) or execute inline.
  3. **Verification & State Update**: On worker completion, verify the worker's commit against `git log` and test gates. Update `- [x]` in the active plan file on disk and commit.
  4. **Continuous Handoff**: Immediately dispatch the next unchecked task (`- [ ]`) without pausing or asking the human.
  5. **Completion Gate**: Hand control back to the human (message without `@mention`) *only* when 100% of plan tasks are marked `- [x]`, or on unrecoverable errors.

### 2.3 Worker Handoff Contract
- Workers in Bypass Mode continue to report progress/completion starting with `@orchestrator`.
- If a worker turn encounters an error, the Orchestrator applies systematic debugging or re-dispatches, escalating to the human only when progress is completely blocked.

---

## 3. Implementation Targets
1. `crates/hadron-gluon/src/adapter/prompt/mod.rs`: Update `build()` prompt generator with `Mode::Bypass` Orchestrator directives.
2. `crates/hadron-gluon/src/adapter/prompt/tests.rs`: Add unit tests ensuring Bypass Mode Orchestrator prompts contain autonomous execution and plan-updating rules.
