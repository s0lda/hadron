---
name: reviewing-work
description: Use when performing code review or plan verification on work completed by another agent or self
---

# Reviewing Work

## Core Principle
Verify author claims against ground truth in the repository. Where claims and codebase reality disagree, the codebase wins. Never accept a summary as evidence.

## Verification Checklist

### 1. Verify Ground Truth Claims
- **"Tests pass":** Execute full gate suite (`cargo test`, `npm test`, etc.) — never a narrow filter subset. Compare test output against baseline.
- **"It's committed":** Inspect `git log -n 5` and `git show <hash>` to verify commits exist and contain all modified files. Confirm struct/type changes include all invocation sites.
- **"It works / wired up":** Search by symbol, trait, and module path. Verify callers exist. If uncalled, label as `"implemented, unwired"` (a finding, not a silent pass).

### 2. Adversarial Evaluation (Try to Break It)
- Test edge/boundary cases: empty inputs, missing fields, swallowed error paths, concurrent calls.
- Security/Attack surface: evaluate auth, file access, command execution, or untrusted inputs touched by the change.

### 3. Verdict Output Contract
Output specific findings with `file:line` locations. Distinguish actual failures from stylistic preferences.

Provide one explicit verdict:
- **Approved**: Include exact command run and summary output. (Do NOT say "looks good").
- **Changes Needed**: Itemize findings, locations, and failure scenarios.

Hand back to the author by name (`@<quark-id>`). If you fixed an issue yourself, state the exact change and rationale.
