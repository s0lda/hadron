# Encrypted per-seat secrets (API keys) — design + plan

**Status:** design → implement (Jake: "proceed with encryption, the proper way, not a cheap one"). Branch `feat/encrypted-secrets`.
**Date:** 2026-07-18

## Goal
Let a seat carry secret environment variables (e.g. `GEMINI_API_KEY` for Antigravity) that are injected into its spawned subprocess — with the **values stored only in the OS credential store, never in `team.json` or any plaintext file**, and never in argv, logs, the field, or a quark prompt.

## Non-negotiables (Jake)
- **Never plaintext at rest.** `team.json` holds only the env-var NAMES (`secret_env: ["GEMINI_API_KEY"]`), never values. Values live in the OS keychain.
- **Proper store, not a cheap one.** OS credential store via the `keyring` crate (macOS Keychain / Windows Credential Manager / Linux Secret Service). NO home-grown "encrypt with a key next to the data" fallback. If the platform has no credential store, the operation errors clearly — we do not silently degrade to plaintext.

## Verified technical facts
- **ACP subprocess env is natively supported.** `agent-client-protocol` v1.2's `AcpAgent::from_str` accepts a JSON stdio descriptor with an `env: [{name, value}]` array, and applies it via `cmd.env(name, value)` before spawn (acp_agent.rs:185-187). So the ACP path injects env through the descriptor — no argv-leak hack.
- **CLI subprocess** spawns via `ProcessRunner` (`runner.rs`), which builds a `CliInvocation` and does NOT set env today — add an `env` field + `.envs(...)`.
- `keyring = "4"` needs an explicit backend feature (default is a no-op mock). Enable `apple-native`, `windows-native`, and Linux `sync-secret-service` (+ `crypto-rust`). Linux/WSL2 requires a running Secret Service (gnome-keyring/KWallet + D-Bus) — documented; this is the cost of the proper store.

## Architecture
- **`SecretStore` trait** (`hadron-lattice/src/secrets.rs`): `get(seat: &QuarkId, var: &str) -> Result<Option<String>>`, `set(seat, var, value)`, `delete(seat, var)`. Account key = `format!("{seat}/{var}")`, service `"hadron"`.
  - `KeyringStore` — the `keyring`-backed impl (daemon + chamber).
  - `MemoryStore` — an in-memory `HashMap` impl for tests, so **no test ever touches the real keychain**.
- **`Seat.secret_env: Vec<String>`** — the var NAMES to resolve at spawn. `#[serde(default, skip_serializing_if = "Vec::is_empty")]`. Added to `same_agent` (a change re-seats, like `commands`).
- **Resolution at spawn** (freshness / rotation): the engine holds a `Box<dyn SecretStore>` (injected — `MemoryStore` in tests, `KeyringStore` in the daemon bin). At build/boot it resolves `seat.secret_env` → `Vec<(name, value)>` and hands the adapter a resolved env list. Adapters never see the store; they inject the resolved list (ACP via the JSON descriptor `env`, CLI via `CliInvocation.env`). A missing key resolves to absent (the agent then reports its own "missing credential" error, as today).
- **UI** (`settings.rs`): a masked per-seat "API key" input in the quark editor. On save: `store.set(seat, VAR, value)` AND ensure `VAR ∈ seat.secret_env` (persisted to `team.json`). A "Clear" action calls `store.delete`. The field never renders the stored value (write-only; shows "•••• set" vs "not set").

## Leak surface (Rule 7 — the review must check each)
- The value is NEVER serialized to `team.json`, the field/bus, a `Projection`, or a quark prompt. Only the NAME appears in `team.json`.
- Not placed in argv (ACP uses the descriptor `env`, not the command string; CLI uses `.envs`, not args).
- Not logged: no `eprintln!`/tracing of the resolved value; error messages name the VAR, never the value.
- `Debug`/`Display` on any type holding a resolved value must redact it (or not derive `Debug`).
- Tests use `MemoryStore` exclusively; a CI/headless run must not require or hit a real keychain.

## Tasks (subagent-driven, adversarial review — security-critical)
1. **`SecretStore` + `KeyringStore` + `MemoryStore`** (lattice, `keyring` dep). Tests: `MemoryStore` round-trip (set/get/delete, per-seat/per-var isolation); the account-key format; a "get absent → None". `KeyringStore` is NOT unit-tested against a live keychain (integration-only, `#[ignore]`).
2. **`Seat.secret_env`** (lattice): field + serde default + `same_agent` + a `resolve_env(store) -> Vec<(String,String)>` helper. Tests: serde round-trip, absent = empty & omitted, `same_agent` rebuilds on change, resolve pulls from a `MemoryStore` and skips absent.
3. **Spawn injection** (gluon): `CliInvocation.env` + `ProcessRunner.envs`; ACP boot builds the JSON descriptor with `env`. Thread the resolved env from `Seat.secret_env` + injected store through registry/engine to both adapters. Tests: CLI invocation carries env through `FakeRunner`; ACP descriptor JSON contains the env (unit-test the descriptor builder, not a live spawn); a redaction test (Debug of the carrier does not print the value).
4. **UI** (chamber): masked API-key field + save-to-keychain + Clear + `secret_env` persistence. Value-logic unit-tested; visual flow marked needs-Jake's-eyes (WSL2).

## Global constraints
- Gate `cargo test --workspace --features gui` green before/after each task.
- One focused commit per task. Adversarial whole-branch review before merge, pinned to the leak surface above.
