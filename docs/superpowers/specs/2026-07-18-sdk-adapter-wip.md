# Native SDK adapter for agy (sub-project #3) — spec + plan — **DOCUMENTED WIP, NOT IMPLEMENTED**

**Status:** WIP handoff. **Deliberately NOT implemented in the autonomous session.** This is a design + plan the user executes and verifies with real credentials.
**Date:** 2026-07-18
**Branch:** `feat/sdk-adapter-wip` (docs only)

## Why this ships as a plan, not code (honest scope)
`Transport::Sdk` is reserved-and-hard-rejected today (`registry.rs` `from_seat` bails "sdk transport is reserved but not yet implemented (see sub-project #3)"). A *real* native adapter must speak to the Gemini/Antigravity API over the network with a `GEMINI_API_KEY`. **I have no key and no network in this sandbox, and self-authored fixtures would only prove the code matches my memory of the Gemini wire format — not the real API.** Shipping an unverified network client as "done" violates Rule 1 ("prove it runs"). The existing Python bridge (`crates/hadron-gluon/scripts/agy_acp.py`, an ACP-over-stdio wrapper around `google.antigravity.Agent`) already works over `Transport::Acp`, so a native Rust adapter shipped unverified would be *negative* value. Per the advisor's explicit call: hand over a thorough plan, keep the `Transport::Sdk` bail as-is.

## What "SDK adapter" means here
Today `acp-agy` reaches Gemini via `Transport::Acp` → a Python subprocess (`agy_acp.py`) that maps ACP JSON-RPC onto the Python Antigravity SDK. AUTH: the SDK takes an API key or Vertex project only — no OAuth — so it needs `GEMINI_API_KEY` in the daemon env (the `agy` CLI's OAuth creds don't work).

A native `sdk-agy` (`Transport::Sdk`) would replace the Python hop with a **resident Rust adapter** that calls the Gemini API (`generativelanguage.googleapis.com` `generateContent`/streaming, or Vertex) directly — no subprocess, no ACP framing. This is the `multi-provider-sdk-adapters` direction (memory note): resident per-provider SDK adapters that can later do real per-tool `canUseTool` propose-and-wait (which is the *deeper* layer WS4§1's gate rides toward — see the decoupling note below).

## Architecture (to implement + verify with a key)
- **`SdkQuark<C: GeminiClient>`** (`crates/hadron-gluon/src/adapter/sdk.rs`) implementing `Quark`. Holds a client, model, roles/display_name (carried like the CLI/ACP adapters), and a resident conversation history (`Vec<Content>`), so multi-turn context persists without re-sending (mirrors agy's `--continue`).
- **`GeminiClient` trait** (the network seam — the ONLY unverifiable part): `async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse>`. A `ReqwestGeminiClient` hits the real API (needs `GEMINI_API_KEY`); a `FakeGeminiClient` returns queued responses for tests. **Keep ALL network specifics behind this trait so everything else is unit-testable; the trait impl is what the user verifies live.**
- **Translation (pure, testable — but see the fixture caveat):** `projection → GenerateRequest` (system instruction = the invariants/prompt; history = resident `Content`s; the new turn = the task). `GenerateResponse → TurnOutcome` (text → message; `usageMetadata` → `TokenSpend` {promptTokenCount→input, candidatesTokenCount→output, cachedContentTokenCount→cache_read}; safety-block/finishReason → a clear error or empty turn). **CAVEAT: the request/response JSON shapes below are from memory of the Gemini `generateContent` API and MUST be validated against a live call — do not trust the fixtures until a real round-trip confirms the field names.**
- **Registry:** `Transport::Sdk => QuarkKind::Sdk(SdkSpec)`; `from_seat` resolves an `SdkSpec` (model, key-env-var name, base URL / Vertex toggle) from `seat` — mirror the CLI `CliSpec::preset` pattern with an `SdkSpec::gemini()` preset for vendor `agy`. Plumb roles/exclusive/display_name via the same `with_*` builders. Replace the hard-bail arm.
- **Auth:** read `GEMINI_API_KEY` (or the configured env var) at build/first-excite; a missing key returns a clear error naming the env var (like `agy_acp.py`'s `NO_KEY` message), never a silent stall.
- **Streaming (optional, later):** the live-watch stream (like ACP's) could publish mid-turn text via the same `live` mechanism; start with non-streaming `generateContent`.

## Decoupling from WS4§1 (important — from the advisor)
The SDK path is the layer that would enable **real per-tool `canUseTool` propose-and-wait**. But **WS4§1's gate does not depend on it**: WS4§1 operates at the posture/command granularity the existing permission-modes work already uses (transport-agnostic). Build WS4§1 against today's transports; per-tool granularity rides on THIS adapter later. Do not let the unbuildable SDK gate the buildable gatekeeper.

## Implementation plan (for the user, with a key)
1. Add `SdkSpec` (lattice) + `SdkSpec::gemini()` preset + `Seat`→spec resolution. TDD serde + preset.
2. `GeminiClient` trait + `FakeGeminiClient`. `SdkQuark` with translation as pure fns.
3. Registry: `QuarkKind::Sdk`, `from_seat` Sdk arm (replace bail), `build` → `SdkQuark` over `ReqwestGeminiClient`, plumb roles/display_name.
4. **VERIFY LIVE (the step I cannot do):** with `GEMINI_API_KEY` set, drive one real turn (`#[ignore]`d live test, like the ACP live tests). Confirm the request shape is accepted and the response/usage parse. **Fix the wire format against reality here** — the memory-derived shapes in step 2 are unverified until this passes.
5. Once verified, migrate the `acp-agy` seat → `sdk-agy` (`Transport::Sdk`, vendor `agy`, no python command) and retire `agy_acp.py` (or keep it as a fallback).
6. Reserved `Transport::Sdk` stops being a bail; `sdk_transport_is_reserved_and_not_seatable` (from taxonomy #2) is replaced by real seating tests.

## Security note (Rule 7)
A live SDK adapter adds a **network egress** surface (HTTPS to Google) and reads a secret (`GEMINI_API_KEY`) from the daemon env — the same trust surface `agy_acp.py` already has. No new *local* attack surface. The key must come from the environment, never `team.json` (never persist secrets in config). Validate TLS; treat the API response as untrusted input (parse defensively).

## Open questions for the user
- **Native Rust vs. keep the Python bridge?** The bridge works. Is a native adapter worth the maintenance (a Rust Gemini client to track against API changes), or is the real goal just to rename `acp-agy` honestly? If the latter, this is a naming change, not a new adapter.
- **Vertex vs. AI Studio** endpoint/auth (API key vs. Vertex project)?
- **Streaming** needed for the live-watch panel, or is non-streaming fine to start?
