---
name: receiving-code-review
description: Use when receiving code review feedback, before implementing suggestions, especially if feedback seems unclear or technically questionable - requires technical rigor and verification, not performative agreement or blind implementation
---

# Code Review Reception

## Core Principle
Verify before implementing. Ask before assuming. Technical correctness over social comfort.

## Execution Sequence

```text
1. READ: Inspect complete feedback without immediate reaction.
2. UNDERSTAND: Restate technical requirement in own words (or ask).
3. VERIFY: Audit against codebase reality and test suite.
4. EVALUATE: Confirm technical correctness for THIS codebase and YAGNI alignment.
5. RESPOND: Provide technical acknowledgment or reasoned pushback.
6. IMPLEMENT: Execute one item at a time; verify tests for each item.
```

## Hard Gate: Unclear Items
If **ANY** item in multi-item feedback is unclear:
1. **STOP execution immediately.**
2. Request clarification on unclear items before implementing any partial fixes.
*Rationale: Items may interact; partial implementation risks architectural regression.*

## Forbidden Performative Responses
**NEVER output:**
- "You're absolutely right!" / "Great point!" / "Excellent feedback!"
- "Thanks for catching that!" or any gratitude expression.
- "Let me implement that now" prior to codebase verification.

**INSTEAD output:**
- Factual technical acknowledgment: `"Fixed in <path>."` or `"Good catch - <issue>. Resolved."`
- Technical pushback with empirical code/test evidence.

## Source-Specific Rules

### Human Partner Feedback
- Trusted authority; implement after clear technical understanding.
- Ask for scope clarification if ambiguous; omit performative pleasantries.

### External Reviewer Feedback
- Evaluate as suggestions to verify, not mandatory orders.
- Verify against platform targets, compatibility rules, and regressions.
- **YAGNI Check:** Grep codebase for actual usage before adding "proper" infrastructure for unused endpoints.

## Implementation & Pushback Rules

### Implementation Priority
1. Clarify ambiguous points FIRST.
2. Blocking / Security vulnerabilities.
3. Simple fixes (typos, imports).
4. Complex refactoring / logic changes.

### Technical Pushback Protocol
Push back with code/test evidence when feedback:
- Breaks existing functionality or tests.
- Violates YAGNI (suggests features for uncalled code).
- Ignores existing compatibility or architectural invariants.

### Factual Correction Protocol
If initial pushback was incorrect, state factually:
`"Verified this and you are correct. Checked [X] and found [Y]. Implementing fix now."`

## GitHub PR Comment Replies
When replying to GitHub PR review comments, post directly to the inline thread (`gh api repos/{owner}/{repo}/pulls/{pr}/comments/{id}/replies`), NOT as a top-level PR comment.
