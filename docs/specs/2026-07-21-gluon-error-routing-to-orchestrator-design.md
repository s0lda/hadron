# Design: Route Gluon Execution & Turn Errors to Orchestrator

## Context & Purpose
When a Quark turn fails (due to process exit, timeout, tool failure, or panic) or when an excite error occurs in `hadron-gluon`, the error is currently logged to stderr or recorded only as a terminal `Status { state: Error }` event. Because no error message is posted to the field log addressing the orchestrator, the orchestrator is not excited to handle or report the failure to the user.

This design ensures that whenever a turn or daemon excite error occurs, `hadron-gluon` automatically appends a `Kind::Message` event to the field log that addresses `@orchestrator` (when seated) so the orchestrator receives the error report and can take appropriate action.

## Operational & Architectural Changes

### 1. Error Message Event Generation in `Engine::run_until_quiesce` (`src/engine/run.rs`)
When a turn ends with `Err(err)` or a panic occurs:
- Determine if an orchestrator seat exists on the roster (`Flavor::Orchestrator`).
- If an orchestrator exists and the failed quark is NOT the orchestrator:
  - Body: `@orchestrator ⚠️ Quark '<id>' turn errored: <error_text>`
- If the failed quark IS the orchestrator or no orchestrator exists:
  - Body: `⚠️ Quark '<id>' turn errored: <error_text>`
- Append an `Event::new(Actor::Gluon, None, Kind::Message { body })` event to the field log.

### 2. Daemon Excite Error Field Logging (`src/bin/hadron-gluon.rs`)
When `engine.run_until_quiesce().await` in the daemon main loop returns `Err(e)`:
- If an orchestrator exists:
  - Body: `@orchestrator ⚠️ Gluon excite error: <e>`
- Else:
  - Body: `⚠️ Gluon excite error: <e>`
- Append this event to the field log so excite errors are preserved in the field log and reported to the orchestrator.

## Verification & Testing Strategy
1. Add unit test `failing_quark_turn_sends_error_message_to_orchestrator` in `src/engine/tests.rs` verifying that a failing worker turn appends an error message mentioning `@orchestrator`.
2. Add unit test verifying that when the orchestrator itself errors, it appends an error message without `@orchestrator` prefix to avoid self-excitation loops.
3. Run `cargo test -p hadron-gluon` and `cargo test --workspace --features gui` to confirm all tests pass.
