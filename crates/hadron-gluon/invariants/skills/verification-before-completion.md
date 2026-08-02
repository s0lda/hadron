---
name: verification-before-completion
description: Use when about to claim work is complete, fixed, or passing, before committing or creating PRs - requires running verification commands and confirming output before making any success claims; evidence before assertions always
---

# Verification Before Completion

## Core Principle
Evidence before claims, always. Claiming work is complete without fresh empirical verification is an invariant violation.

## The Iron Law
```text
NO COMPLETION CLAIMS WITHOUT FRESH VERIFICATION EVIDENCE IN THE CURRENT TURN
```

## Mandatory Gate Function
Before making any completion statement, expressing satisfaction, or committing code:

1. **IDENTIFY:** Determine the exact command that proves the claim (`cargo test`, `npm test`, etc.).
2. **RUN:** Execute the complete verification command in the current session (fresh run).
3. **READ:** Inspect full output log, exit code, and failure counts.
4. **VERIFY:** Confirm output validates the assertion (0 errors, 0 failures).
5. **CLAIM:** State outcome WITH exact command run and summary output.

## Required Verification Matrix

| Claim | Mandatory Proof Requirement | Invalid Evidence (Prohibited) |
|---|---|---|
| Tests pass | Full test suite output: 0 failures | Previous turn output, "should pass" |
| Linter clean | Linter tool output: 0 errors | Partial file check |
| Build succeeds | Compiler exit code 0 | Linter passing, clean syntax |
| Bug fixed | Reproducer test execution: PASS | Code changed without re-test |
| Regression test | Verified Red-Green cycle | Test passing only once |
| Subagent done | Inspect `git diff` / VCS state | Subagent self-report summary |
| Task finished | Line-by-line plan checklist | Tests passing alone |

## Verification Patterns
- **TDD Red-Green Verification:** Write test → Run (FAIL) → Apply fix → Run (PASS) → Revert fix → Run (FAIL) → Restore fix → Run (PASS).
- **Subagent Hand-off:** Inspect `git status` and `git diff` directly. Do not rely on subagent completion text.

## Red Flags & Prohibition
- Expressing completion or satisfaction ("Great!", "Done!", "Fixed!") before running verification commands.
- Using speculative language ("should work", "appears fixed", "probably passes").
- Trusting subagent success reports without inspecting code diffs.
