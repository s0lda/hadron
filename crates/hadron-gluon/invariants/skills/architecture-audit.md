---
name: architecture-audit
description: Audit component coupling, SSOT integrity (Rule 3), invariant schemas, and unrepresentable invalid states (Rule 8)
---

# Architecture Audit

Evaluate system design, structural cohesion, module coupling, and invariant integrity.

## Core Principles
1. **Single Source of Truth (Rule 3):** A value, rule, or type has exactly one canonical home. Copying creates drift.
2. **Make Invalid States Unrepresentable (Rule 8):** Push runtime invariants into the type system (enums, newtypes, non-empty structures) rather than scattering ad-hoc runtime checks.
3. **Layering & Defense-in-Depth (Rule 4):** Never remove defensive guards because they appear redundant.

## Audit Workflow

### 1. Structural Boundaries & Module Coupling
- Inspect module dependencies and directional flow (`lattice` -> `gluon` -> `chamber`).
- Detect circular dependencies, shared mutable state, or leaky abstractions where callers reach into internal private state.
- Verify separation of pure text/model logic from UI/rendering frameworks (e.g. keeping pure logic testable without GUI features).

### 2. SSOT & Drift Detection
- Search the codebase for duplicate constant definitions, mirrored enum variants, or parallel parsing tables.
- Ensure UI completion tables, CLI parsers, and backend routers derive from a single unified definition.

### 3. Type-Level Hardening
- Identify loose types (e.g. bare `String` used for paths, ids, or tokens where a `ResolvedAcpTarget`, `QuarkId`, or `SessionId` newtype prevents invalid dispatch).
- Replace stringly-typed configuration with typed enums and exhaustive pattern matches.
- Ensure error variants propagate meaningful context instead of discarding errors with `.ok()` or bare `unwrap()`.

### 4. Output Contract
- **Coupling & Cohesion Assessment:** Overview of structural boundaries.
- **SSOT Violations & Duplication:** Locations where logic or configuration is duplicated.
- **Type Hardening Proposals:** Concrete suggestions to move runtime checks into compile-time types.
- **Architecture Verdict:** `Sound` or `Refactoring Recommended` with prioritized action items.
