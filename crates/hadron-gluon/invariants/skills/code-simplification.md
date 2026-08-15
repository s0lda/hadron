---
name: code-simplification
description: Ruthless code reduction (Rule 10), pruning dead abstractions, unused imports, and over-engineered logic
---

# Code Simplification

Ruthlessly reduce unnecessary complexity, prune speculative abstractions, and simplify implementation footprints while preserving exact behavior.

## Core Principle (Standard Model Rule 10)
Minimum code that solves the problem, nothing speculative. If you wrote 200 lines and it could be 50, rewrite it. Touch only what you must, match existing style, and remove dead code.

## Simplification Rules

### 1. Strip Speculative Abstractions
- Remove single-implementation traits, premature generalization layers, and unused builder patterns.
- Inline one-line wrapper functions that obscure rather than clarify data flow.
- Replace complex multi-nested closures or match chains with flat, readable control flow.

### 2. Dead Code & Drift Elimination
- Prune unused imports, obsolete struct fields, deprecated command branches, and unreachable error branches.
- Remove redundant state variables that can be directly derived from ground truth.

### 3. Net-Negative Diff Standard
- Measure lines added vs lines removed: prioritize solutions with net-negative line counts.
- Keep signatures minimal: take `&str` instead of `&String`, borrow rather than take ownership when unneeded.

### 4. Verification
- Run full workspace tests (`cargo test --workspace`) to prove refactored code preserves identical behavior and passes all invariants.
