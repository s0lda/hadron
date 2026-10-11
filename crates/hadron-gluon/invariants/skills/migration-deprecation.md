---
name: migration-deprecation
description: Design and execute safe multi-crate API deprecations, backwards-compatible shims, and caller migrations
---

# Safe API Migration & Deprecation

Safely evolve shared interfaces, deprecate obsolete abstractions, and migrate call sites across crates without breaking backwards compatibility or causing runtime regressions.

## Core Principle (Standard Model Rules 1, 3, 4)
An API change across multiple crates must never be a flag-day cutover. Provide backwards-compatible adapters, migrate callers systematically, and verify every call site before retiring old implementations.

## Phased Migration Lifecycle

```text
PHASE 1: Add new API alongside old →
PHASE 2: Wrap old API as deprecated shim delegating to new →
PHASE 3: Migrate all workspace call sites to new API →
PHASE 4: Verify zero remaining references →
PHASE 5: Prune deprecated shim in subsequent release
```

### Phase 1: New Contract Introduction
- Author the new type, function, or trait following Rule 8 (make invalid states unrepresentable).
- Establish single source of truth (Rule 3) for the underlying logic.
- Add unit and integration tests proving the new implementation runs and satisfies all invariants.

### Phase 2: Backwards-Compatible Shims
- Mark legacy entrypoints with deprecation annotations (`#[deprecated(since = "...", note = "...")]`).
- Implement the deprecated function as a thin shim delegating to the new interface.
- If data formats or wire schemas are changing, introduce dual-read/single-write deserialization.

### Phase 3: Systematic Caller Migration
- Find every call site using symbol search, trait implementations, and macro invocations across the workspace.
- Migrate callers incrementally, crate by crate, ensuring tests pass at each step.
- Never silence deprecation warnings with `#[allow(deprecated)]` in production code.

### Phase 4: Verification Gate
- Run full workspace tests and compiler checks (`cargo check --workspace`).
- Grep for deprecated symbols to confirm 0 production call sites remain.
- Standard Model Rule 1: Verify the new API has real live callers executing in production paths.

### Phase 5: Cleanup & Pruning
- Remove the deprecated shim and dead adapter code only after all internal consumers are verified clean.
- Update documentation and `features.md` to reflect the retired API.
