---
name: dispatching-parallel-agents
description: Use when facing 2+ independent tasks that can be worked on without shared state or sequential dependencies
---

# Dispatching Parallel Agents

## Core Principle
The Orchestrator dispatches one task per independent domain across worker Quarks in the Hadron swarm using `@<quark-id> <task>` lines. Workers run concurrently in isolated worktrees and report back to `@orchestrator`.

## Applicability

### Use When
- 2+ independent test failures or broken subsystems across separate files.
- Problem domains require no shared state or sequential ordering.
- Tasks can proceed in parallel without file editing conflicts.

### Do NOT Use When
- Failures are interdependent (fixing one may fix others).
- System requires holistic state analysis.
- Tasks require editing the exact same lines of code simultaneously.

## Execution Pattern

1. **Identify Independent Domains**: Group failures by subsystem/file boundaries.
2. **Construct Focused Prompts**:
   - **Scope**: Single test file or isolated component.
   - **Context**: Exact failure trace and line references.
   - **Constraints**: Do not modify outside scope.
   - **Output Contract**: Return root cause summary and exact code diff/changes.
3. **Dispatch Concurrently**: Emit delegation lines (`@<quark-id> <task>`) at the start of your message to fan them out across the Hadron swarm.
4. **Review & Integrate**:
   - Verify worker reports for correctness and lack of file conflict.
   - Re-run full test suite across the workspace.

## Agent Prompt Checklist
- [ ] **Targeted Scope**: Explicit target file/subsystem.
- [ ] **Empirical Context**: Includes error log/traceback snippet.
- [ ] **Strict Boundaries**: Explicitly states files/behavior off-limits.
- [ ] **Contract Return**: Requires root cause + diff summary return format.

## Post-Execution Verification Checklist
- [ ] Review returned agent summaries and root causes.
- [ ] Inspect git status/diff to confirm non-overlapping file modifications.
- [ ] Execute full workspace verification suite (`cargo test`, `npm test`, or equivalent).
