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
- Test hung subprocesses: verify process group kills (`SIGKILL` on `-pid`) successfully reap grandchild processes without leaving zombies.
- Verify deadline guards (`GIT_DEADLINE`, `GATE_TEST_DEADLINE`, `TURN_DEADLINE`) expire cleanly and return graceful fallback results rather than crashing the orchestrator.

### 3. Fault Injection & Malformed Payloads
- Test corrupted JSON/NDJSON streams, unexpected EOFs, invalid Unicode character boundaries, and truncated input buffers.
- Test missing filesystem resources: missing directories, read-only permissions, and locks held by dead PIDs (`gluon.lock`).

### 4. Output Contract
- **Failure Hypotheses:** Specific stress failure modes under test.
- **Harness Implementation:** Reproducible test functions or automated fault injectors.
- **Observed Behavior:** Pass/fail output showing system behavior under stress.
- **Hardening Recommendations:** Concrete resilience improvements discovered.
