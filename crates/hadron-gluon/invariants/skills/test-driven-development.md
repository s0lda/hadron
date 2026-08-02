---
name: test-driven-development
description: Use when implementing any feature or bugfix, before writing implementation code
---

# Test-Driven Development (TDD)

Write failing test first -> watch it fail -> write minimal code -> verify passing -> refactor.

**Core Principle:** If you didn't watch the test fail, you don't know if it tests the right thing. Violating the letter of the rules is violating the spirit of the rules.

## The Iron Law

```
NO PRODUCTION CODE WITHOUT A FAILING TEST FIRST
```

If code was written before the test: **Delete it completely. Start over.**
- Do NOT keep as reference.
- Do NOT adapt while writing tests.
- Delete means delete.

## When to Use

- **Always:** New features, bug fixes, refactoring, behavior changes.
- **Exceptions (requires human partner approval):** Throwaway prototypes, generated code, pure config files.

## Red-Green-Refactor Cycle

### Phase 1: RED (Write Failing Test)
- Write one minimal test for one specific behavior.
- Use clear descriptive names.
- Test against real code/APIs (avoid mocks unless external/unavoidable).

### Phase 2: Verify RED (MANDATORY)
Run the test runner command (e.g. `npm test path/to/test.test.ts` or `cargo test`).
- Confirm test FAILS (not compilation/syntax error).
- Verify failure message matches missing feature/behavior (not typos).
- If test passes: feature already exists or test is invalid. Fix test.

### Phase 3: GREEN (Minimal Implementation)
- Write the simplest possible code to make the test pass.
- Do NOT over-engineer, add unrequested options, or refactor surrounding code.

### Phase 4: Verify GREEN (MANDATORY)
Run test runner command.
- Confirm test PASSES.
- Confirm full test suite still passes with pristine output (no warnings, errors, or stray logs).
- If test fails or breaks existing tests: fix implementation code, NOT the test.

### Phase 5: REFACTOR
- Remove duplication, improve naming, extract helpers.
- Keep tests green at all times. Do not alter behavior.

## Testing Anti-Patterns

1. **Testing Mock Behavior:** Asserting on mocked objects proves the mock, not the code. Test real component behavior or assert on component actions, never mock existence.
2. **Test-Only Production Methods:** Do NOT add `destroy()` / `reset()` methods to production types solely for test cleanup. Put cleanup in test utilities.
3. **Mocking Without Understanding:** Over-mocking strips required side effects. Mock at the lowest external layer (network/DB), not high-level methods.
4. **Incomplete Mocks:** Partial mocks cause silent downstream failures. Mirror complete data structures.
5. **Tests as Afterthought:** Adding tests after code is complete violates TDD.

## Common Rationalizations & Red Flags

### Excuses vs Reality
| Excuse | Reality |
|--------|---------|
| "Too simple to test" | Simple code breaks. Tests take seconds. |
| "I'll test after" | Passing tests immediately prove nothing. |
| "Tests after achieve same goals" | Tests-after = "what does this do?". Tests-first = "what SHOULD this do?". |
| "Already manually tested" | Ad-hoc != systematic. Unrepeatable. |
| "Deleting X hours is wasteful" | Keeping unverified code is technical debt. |
| "Keep as reference" | Adapting pre-written code is testing-after. Delete it. |

### Red Flags — STOP AND DELETE CODE IMMEDIATELY
- Code written before test
- Test added after implementation
- Test passes on first run
- Cannot explain failure reason
- "Just this once" or "keeping as reference"
- "Already manually tested"
- Test setup requires complex partial mocking of internal layers

## Verification Checklist

Before declaring task complete, confirm:
- [ ] Every new function/method has a test.
- [ ] Watched each test fail BEFORE writing code.
- [ ] Failure was due to missing feature (not typo/compilation error).
- [ ] Wrote minimal code to pass test.
- [ ] All tests pass cleanly with pristine output.
- [ ] Real code tested (mocks used only for external dependencies).
- [ ] Edge cases and error paths covered.

## Final Rule

```
Production code -> test exists and failed first
Otherwise -> NOT TDD
```
