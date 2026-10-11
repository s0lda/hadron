---
name: doc-drift-audit
description: Audit features.md, CLI tables, and docs against live symbol call sites and invariants to prevent drift
---

# Documentation & Feature Drift Audit

Systematically cross-reference project documentation, feature maps, and command reference tables against the real codebase to eliminate stale claims and documentation drift.

## Core Principle (Standard Model Rules 1, 3, 9)
Documentation is part of the system's contract. A document describing an API that was renamed, a command that was removed, or a feature marked active that has zero live callers is a bug.

## Drift Audit Checklist

### 1. Feature Map Audit (`features.md`)
- Open `.hadron/nucleus/features.md`.
- For every listed feature:
  - Verify entrypoint files exist on disk.
  - Verify key functions and structs named in the description exist and are referenced by live production callers (Rule 1).
  - If a feature is removed, mark as deprecated or remove from the map.
  - If status has changed (e.g. from refining to active), update the row.

### 2. Chat Command Table Audit (`COMMANDS`)
- Cross-reference `hadron_chamber::text::COMMANDS` against `crates/hadron-chamber/src/app/actions.rs`.
- Verify every listed command has an active handler.
- Verify command descriptions match current argument syntax and behavior.
- Ensure all public commands appear in user-facing help and README documentation.

### 3. Nucleus Index & Invariants Audit
- Check `.hadron/nucleus/index.md` size against the budget limit (Rule 9).
- Ensure every index line is a pointer (`- [slug](notes/slug.md) — hook`) and contains no inline content body.
- Verify invariant claims in `invariants/` still reflect actual codebase enforcement rather than obsolete workarounds.

### 4. Output Contract
- **Audited Sites:** Files and registries inspected.
- **Drift Detected:** Concrete mismatches between documentation and implementation.
- **Remediation:** Exact edits made to reconcile docs with ground truth.
