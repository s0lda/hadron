---
name: chaos-testing
description: Design stress harnesses, concurrency races, error-recovery paths, and boundary edge cases
---

# Chaos Testing & Test Engineering

Design failure-injection tests, stress harnesses, and concurrency race validations to prove resilience under pressure.

## Core Principle (Standard Model Rule 1)
"The code is correct" and "the code runs" are different claims. Prove systems recover gracefully from process crashes, deadlocks, channel overflow, timeout expiration, and malformed inputs.

## Testing Vectors

### 1. Concurrency & Race Conditions
- Test multi-threaded access and async lock acquisition under contention.
- Validate that aborting a task (`JoinHandle::abort`) does not leak file descriptors, orphan background subprocesses, or leave locks poisoned.
- Check channel backpressure: ensure unbounded event emission does not starve workers or exhaust memory.

### 2. Timeout, Deadlock & Process Teardown
- Test hung subprocesses: verify process group kills (`SIGKILL` on `-pid` via `kill_process_group`) successfully reap grandchild processes without leaving orphaned zombies.
- Verify deadline guards (`GIT_DEADLINE`, `GATE_TEST_DEADLINE`, `TURN_DEADLINE`) expire cleanly and return graceful fallback results rather than wedging or crashing the orchestrator.
- Test the turn watchdog silence invariant: verify that `TURN_DEADLINE` (30m) measures silence via `<field-dir>/live/<quark>.json`, not total elapsed time (actively reporting turns must never be killed).

### 3. Fault Injection & Malformed Payloads
- Test corrupted NDJSON streams: newline-delimited framing with missing newlines, mid-character chunk splitting across socket buffers, partial JSON lines, and unexpected EOFs.
- Test missing filesystem resources: missing directories, read-only permissions, and corrupt event ledgers.
- Test daemon process lock recovery: ensure `gluon.lock` safely recovers when the holding PID no longer names a live gluon process (`main.rs::pid_names_a_live_gluon`), avoiding permanent lockouts after ungraceful crashes.

### 4. Output Contract
- **Failure Hypotheses:** Specific stress failure modes under test.
- **Harness Implementation:** Reproducible test functions or automated fault injectors.
- **Observed Behavior:** Pass/fail output showing system behavior under stress.
- **Hardening Recommendations:** Concrete resilience improvements discovered.
