---
name: systematic-debugging
description: Use when encountering any bug, test failure, or unexpected behavior, before proposing fixes
---

# Systematic Debugging

ALWAYS find the root cause before attempting fixes. Symptom fixes are failure.

## The Iron Law

```
NO FIXES WITHOUT ROOT CAUSE INVESTIGATION FIRST
```

If Phase 1 is incomplete, you are forbidden from proposing or implementing fixes.

## When to Use
Use for ANY bug, test failure, build failure, performance issue, or unexpected behavior.
Do NOT skip when under time pressure, when the fix seems "obvious", or when previous fixes failed.

## The Four Phases

### Phase 1: Root Cause Investigation
1. **Read Error Messages & Traces Completely:** Inspect full log output, line numbers, file paths, and stack traces. Never guess from partial snippets.
2. **Reproduce Consistently:** Identify exact trigger steps. If not reproducible, gather more diagnostic evidence first.
3. **Check Recent Changes:** Review git diff, recent commits, dependency changes, and environmental flags.
4. **Instrument Multi-Component Boundaries:**
   For multi-layer/multi-component systems, insert logging at entry/exit boundaries to pinpoint failure layer before editing logic.
5. **Trace Data Flow to Trigger Source:**
   - Observe symptom location.
   - Trace back through callers to original trigger (e.g. empty string resolving to default dir, uninitialized state).
   - Fix at the root source, not at the symptom site.

### Phase 2: Pattern Analysis
1. **Find Working Examples:** Identify similar working code paths in the codebase.
2. **Compare References:** Read working implementations line-by-line; list all differences without assuming any detail is irrelevant.
3. **Identify Dependencies:** Check config, settings, state, and implicit assumptions.

### Phase 3: Hypothesis and Testing
1. **Form Single Hypothesis:** State explicitly: "I think X is the root cause because Y."
2. **Test Minimally:** Make the smallest possible change to test one variable at a time.
3. **Verify:** If hypothesis fails, revert change completely before testing next hypothesis.

### Phase 4: Implementation & Layered Defense
1. **Create Failing Test Case:** Use `test-driven-development` skill to write a reproducing failing test BEFORE changing production code.
2. **Implement Root Cause Fix:** One logical change only.
3. **Add Defense-in-Depth:** For invalid data bugs, validate at multiple layers:
   - *Entry boundary:* Reject invalid inputs at API layer.
   - *Business logic:* Assert state invariants.
   - *Environment guard:* Restrict dangerous ops (e.g., test directory boundaries).
   - *Instrumentation:* Log context for forensic tracing.
4. **Verify Fix:** Confirm failing test passes and rest of test suite remains green.
5. **3-Fix Limit & Architectural Escalation:**
   - If 3 fix attempts fail, **STOP immediately**.
   - Do NOT try fix attempt #4.
   - Question the architecture: recurring failures indicate flawed design/coupling. Discuss architectural changes with partner.

## Debugging Flaky Timing (Condition-Based Waiting)

Never use fixed `sleep` / `setTimeout` delays. Implement predicate polling with explicit timeouts:

```
waitFor(predicate, description, timeoutMs = 5000):
  loop:
    if predicate() is truthy: return result
    if elapsed > timeoutMs: throw "Timeout waiting for {description}"
    sleep 10ms
```

## Red Flags — STOP and Return to Phase 1

- Proposing fixes without log evidence or traceback.
- Patching where error surfaces instead of root source.
- Applying multiple changes at once.
- Attempting fix #4 without architectural review.
- Skipping failing test creation.
- Thinking "quick patch now, investigate later".

## Human Partner Redirection Signals
If human partner says: *"Is that not happening?"*, *"Will it show us...?"*, *"Stop guessing"*, or *"Ultra-think this"* -> **STOP immediately**. Revert changes and return to Phase 1.

## Quick Reference Workflow

| Phase | Core Goal | Output |
|-------|-----------|--------|
| **1. Root Cause** | Trace errors, logs, multi-layer boundaries | Verified root cause |
| **2. Pattern** | Compare against working examples | Difference list |
| **3. Hypothesis** | Single minimal hypothesis test | Confirmed cause |
| **4. Fix & Verify** | Failing test + single fix + layered defense | Verified fix & passing suite |
