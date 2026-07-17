# Custom CLI Transport Implementation Plan (sub-project #1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make `Transport::Cli` generic and config-driven via a `CliSpec`; fold `agy` onto it as a built-in preset; delete the bespoke `claude.rs` and `agy.rs`.

**Architecture:** One `CliQuark<R>` adapter interprets a serializable `CliSpec` (program, args, prompt channel, model flag, resume, timeout, posture map, argv-guard). `agy`'s hardened behaviours are preserved as `CliSpec`-gated Rust logic shipped as `CliSpec::agy()`. Claude is ACP-only.

**Tech Stack:** Rust workspace (hadron-lattice, hadron-gluon, hadron-chamber), serde, async-trait. Tests via cargo test.

## Global Constraints
- Baseline gate before/after each task: `cargo test --workspace --features gui` (full). CLAUDE.md Rule 5.
- INERT session: do NOT run the chamber/gluon binaries (only cargo test/check); do not push/merge; do not touch live `~/.hadron` or repo `.hadron/team.json`. Tests use tempdirs only.
- SSOT: the CLI invocation shape lives once in `CliSpec`; `agy`'s behaviour lives once in `CliSpec::agy()`. Reuse `runner.rs` (`CliInvocation`/`CliRunner`/`reply_to_outcome`) unchanged.
- Behaviour parity: after folding, `cli-agy` must behave byte-for-byte as `agy.rs` did — proven by porting agy.rs's tests. Preserve the E2BIG `fit_prompt` guard, `--print-timeout`, `--continue` resident resume + field-window stripping, posture vocabulary. These have documented incident history — do not weaken.
- Do NOT change ACP transport, routing, prompt building, or permission-mode semantics.
- Match existing style; remove unused imports. Frequent commits.

---

### Task 1: `CliSpec` types + `Seat.cli` field (lattice)

**Files:**
- Modify: `crates/hadron-lattice/src/team.rs` (new `CliSpec` + sub-types, presets; `Seat.cli` field; `same_agent`; `Seat::cli()` ctor; Seat literals)
- Modify: `crates/hadron-lattice/src/lib.rs` (wildcard `pub use team::*` already re-exports — confirm)
- Test: inline `#[cfg(test)]` in `team.rs`

**Interfaces (Produces):**
- `CliSpec { program: String, args: Vec<String>, prompt: PromptChannel, model_flag: Option<String>, resume: ResumeMode, timeout: Option<TimeoutArg>, posture: PostureMap, argv_guard: bool }` (all serde, sensible `#[serde(default)]`).
- `enum PromptChannel { Stdin, Arg { flag: Option<String> } }` (default `Stdin`).
- `enum ResumeMode { None, Continue { flag: String } }` (default `None`).
- `struct TimeoutArg { flag: String, value: String }`.
- `struct PostureMap { ask: Vec<String>, write: Vec<String>, auto: Vec<String>, bypass: Vec<String> }` (default all-empty) + `fn for_mode(&self, Mode) -> &[String]`.
- `impl CliSpec { fn agy() -> Self; fn preset(vendor: &str) -> Option<Self>; fn generic(program: String, args: Vec<String>) -> Self }`.
- `Seat.cli: Option<CliSpec>` (`#[serde(default, skip_serializing_if=Option::is_none)]`).

- [ ] **Step 1: Write failing tests** — `cli_spec_serde_round_trips`, `agy_preset_matches_todays_agy_flags` (assert program `"agy"`, prompt `Arg{flag:Some("--print")}`, model_flag `Some("--model")`, resume `Continue{flag:"--continue"}`, timeout `{"--print-timeout","29m"}`, argv_guard true, posture ask=`["--mode","plan"]`, bypass=`["--dangerously-skip-permissions"]`), `preset_resolves_agy_and_none_for_unknown`, `generic_spec_is_stdin_raw` (prompt `Stdin`, no model_flag/resume/timeout, empty posture, argv_guard false).

- [ ] **Step 2: Run — expect FAIL to compile** (`cargo test -p hadron-lattice cli_spec`). Types undefined.

- [ ] **Step 3: Implement** the types + impls above. `agy()` mirrors `crates/hadron-gluon/src/adapter/agy.rs` (posture_args, PRINT_TIMEOUT="29m", `--continue`, `--print`, argv guard). `generic(program,args)` = `{ program, args, prompt: Stdin, model_flag: None, resume: None, timeout: None, posture: PostureMap::default(), argv_guard: false }`.

- [ ] **Step 4: Add `Seat.cli` field** — add to struct, to `Seat::cli()` ctor (`cli: None`), and to `same_agent`'s destructure + comparison (a changed `cli` spec is a different agent → rebuild). Fix every `Seat { .. }` literal across the workspace (grep `Seat {` in crates/, add `cli: None`); the `..Seat::cli(...)`/`..def.clone()` spread literals need no change.

- [ ] **Step 5: Run tests + full gate** (`cargo test -p hadron-lattice cli_spec`, then `cargo test --workspace --features gui`). Expect PASS.

- [ ] **Step 6: Commit** — `git add crates/hadron-lattice/src/team.rs crates/hadron-lattice/src/lib.rs && git commit -m "feat(lattice): CliSpec + Seat.cli field for the generic CLI transport"`

---

### Task 2: `CliQuark<R>` adapter (gluon) — port agy's logic onto `CliSpec`

**Files:**
- Create: `crates/hadron-gluon/src/adapter/cli.rs`
- Modify: `crates/hadron-gluon/src/adapter/mod.rs` (`pub mod cli;`)
- Test: inline `#[cfg(test)]` in `cli.rs` (port from `agy.rs` + new generic tests)

**Interfaces (Consumes):** `CliSpec`, `PromptChannel`, `ResumeMode`, `TimeoutArg`, `PostureMap` (Task 1); `runner::{CliInvocation, CliRunner, reply_to_outcome}`; `prompt::build`. **(Produces):** `CliQuark<R: CliRunner>` implementing `Quark`, `CliQuark::new(id, flavor, model, spec, runner)`, `.with_display_name(...)`.

This task ADDS `cli.rs` alongside `claude.rs`/`agy.rs` (which still exist and compile); the registry rewire + deletion is Task 3.

- [ ] **Step 1: Port the guard helpers** into `cli.rs`, gated by spec fields:
  - `fit_prompt(projection, self_id) -> String` — copy verbatim from `agy.rs` (the E2BIG argv guard, `MAX_ARG_STRLEN`, `SAFE_ARG_BYTES`, `TRUNCATION_MARKER`, oldest-first drop). Applied only when `spec.argv_guard`.
  - `without_field_window(projection) -> Projection` — copy verbatim from `agy.rs`. Applied only when resuming.

- [ ] **Step 2: Write the invocation builder + `excite`** — `CliQuark::invocation(&self, prompt, mode, cwd) -> CliInvocation`:
  - args = `spec.args.clone()`; if resident && `ResumeMode::Continue{flag}` → push `flag`.
  - prompt placement: `Stdin` → `stdin = prompt`, no prompt arg; `Arg{flag: Some(f)}` → push `f`, push prompt, stdin empty; `Arg{flag: None}` → push prompt positionally, stdin empty.
  - `timeout` → push flag+value; `model_flag` + non-empty model → push flag+model; posture → extend `spec.posture.for_mode(mode).to_vec()`.
  - `excite`: `prompt = if spec.argv_guard { fit_prompt(&turn2,...) } else { prompt::build(&turn2,...) }` where `turn2 = if resident && resume!=None { without_field_window(turn) } else { turn }`; run; set `resident=true`; `reply_to_outcome(&result)`.

- [ ] **Step 3: Port agy.rs's ENTIRE test module** into `cli.rs`, constructing `CliQuark::new(id, flavor, model, CliSpec::agy(), runner)` instead of `AgyQuark::new(...)`. Every assertion (argv-guard E2BIG, truncation-drops-oldest, normal-prompt-untouched, cwd, posture-maps-each-mode, print-mode+reply, print-timeout override, resident `--continue` stops resending field) must pass unchanged. This is the byte-for-byte parity proof.

- [ ] **Step 4: Add new generic tests** — `generic_cli_pipes_prompt_on_stdin_and_reads_raw_stdout` (spec `generic("cat", vec![])`, FakeRunner returns "reply", assert stdin carried the prompt, message == reply), `generic_cli_passes_no_model_or_posture_flags`.

- [ ] **Step 5: Run** `cargo test -p hadron-gluon cli::` then full gate. Expect PASS.

- [ ] **Step 6: Commit** — `git add crates/hadron-gluon/src/adapter/cli.rs crates/hadron-gluon/src/adapter/mod.rs && git commit -m "feat(gluon): CliQuark generic adapter; agy behaviour ported to CliSpec::agy()"`

---

### Task 3: Registry rewire + delete `claude.rs`/`agy.rs` (gluon)

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/registry.rs` (`QuarkKind`, `from_seat`, `from_vendor`→preset resolution, `build`, `build_seat`, `build_seat_watched`, tests)
- Delete: `crates/hadron-gluon/src/adapter/claude.rs`, `crates/hadron-gluon/src/adapter/agy.rs`
- Modify: `crates/hadron-gluon/src/adapter/mod.rs` (remove `pub mod claude; pub mod agy;`)

**Interfaces:** `QuarkKind::Cli(CliSpec)` replaces `QuarkKind::{Claude, Agy}`; `QuarkKind::Acp(AcpTarget)` unchanged.

- [ ] **Step 1: Write failing tests** in `registry.rs`:
  - `cli_agy_seat_resolves_to_the_agy_preset` — `acp_seat`-style helper but Cli transport, vendor "agy", no cli/command → `from_seat` gives `QuarkKind::Cli(spec)` with `spec == CliSpec::agy()`.
  - `cli_seat_with_explicit_cli_spec_wins` — a Cli seat carrying `cli: Some(custom)` resolves to `Cli(custom)`.
  - `cli_seat_unknown_vendor_no_spec_errors` — Cli seat vendor "mystery", no cli/command → `from_seat` errs with a message naming the missing spec/command.
  - `cli_seat_bare_command_builds_generic` — Cli seat with `command: Some({program, args})`, no `cli`, unknown vendor → `Cli(CliSpec::generic(program,args))`.

- [ ] **Step 2: Run — expect FAIL to compile** (`QuarkKind::Cli` doesn't exist yet).

- [ ] **Step 3: Rewire** — replace `QuarkKind::{Claude,Agy}` with `Cli(CliSpec)`. `from_seat` `Transport::Cli` arm resolves per spec §4.3: `seat.cli.clone()` else `CliSpec::preset(&seat.vendor)` else `seat.command`→`CliSpec::generic(program,args)` else `bail!`. `build` `QuarkKind::Cli(spec) => Box::new(CliQuark::new(id, flavor, model, spec, ProcessRunner).with_display_name(name))`. Remove `from_vendor`'s claude/agy arms (now handled by preset). `build_seat_watched` unchanged for Acp; Cli falls through to `build_seat`.

- [ ] **Step 4: Delete** `claude.rs` + `agy.rs` and their `pub mod` lines. Remove any now-dead imports.

- [ ] **Step 5: Run** focused registry tests + full gate. Expect PASS (agy behaviour is now covered by `cli.rs`'s ported tests; the deleted files' tests are gone with them).

- [ ] **Step 6: Commit** — `git add -A crates/hadron-gluon/src/adapter/ && git commit -m "refactor(gluon): route Cli transport through CliQuark; delete bespoke claude.rs/agy.rs"`

---

### Task 4: Settings — add a custom CLI quark (chamber)

**Files:**
- Modify: `crates/hadron-chamber/src/app/settings.rs` and/or `providers.rs` (custom-CLI add path)
- Test: inline value-level tests where possible

**Note:** GUI cannot be driven headless on WSL2 — unit-test the value-level derivation; mark the visual flow needs-your-eyes in the report.

- [ ] **Step 1:** Find the add-quark wizard (`WizardState::PickPreset`/`Connecting` in `settings.rs`, ~line 925-1270) and the preset list (`available_presets`). Add a "Custom CLI" option that collects `program`, `args` (space-split), `vendor` (label), and a prompt-channel toggle (stdin vs a flag).

- [ ] **Step 2:** On save, build a `Cli`-transport `Seat`: `transport: Cli`, `vendor`, `cli: Some(CliSpec::generic(program, args))` (set `prompt` from the toggle), `id: Transport::Cli.conventional_id(&vendor)`, `command: None`. Route through the existing `add_configured_quark`. Reuse `conventional_id` (SSOT) and `id_follows_convention` (advisory) exactly as the ACP path does.

- [ ] **Step 3:** Add a value-level test proving a custom-CLI descriptor produces a `Seat` with `transport==Cli`, `cli.is_some()`, `id=="cli-<vendor>"`. If the derivation is inline in an `on_click` closure, extract it to a small testable helper `fn cli_seat_from(vendor, program, args, channel) -> Seat` and test that.

- [ ] **Step 4:** Run full gate. Expect PASS.

- [ ] **Step 5: Commit** — `git add crates/hadron-chamber/ && git commit -m "feat(chamber): add a custom CLI quark from Settings"`

---

## Self-Review
- Spec §4.1 CliSpec → Task 1. §4.2 agy preset → Task 1 (+ parity tests Task 2/3). §4.3 resolution → Task 1 (types) + Task 3 (from_seat). §4.4 CliQuark → Task 2. §4.5 registry/delete → Task 3. §4.6 Settings → Task 4. §6 testing → each task's tests + agy port (Task 2/3). §7 security → no new surface (documented). ✓
- Placeholder scan: the agy port references `agy.rs` as the verbatim source rather than re-pasting 460 lines — this is a *move*, and the source file is in-repo; acceptable. No TBD/TODO. ✓
- Type consistency: `CliSpec`/`PromptChannel`/`ResumeMode`/`TimeoutArg`/`PostureMap`, `QuarkKind::Cli(CliSpec)`, `CliQuark::new`, `conventional_id` used consistently. ✓
