# Custom CLI transport (sub-project #1) — design

**Status:** design (implemented autonomously per user's standing approval; review when awake)
**Date:** 2026-07-17
**Branch:** `feat/custom-cli-transport` (off `feat/transport-first-taxonomy`)
**Sub-project of:** the adapter-taxonomy rework. #2 (transport-first taxonomy) is done. This is **#1**.

## 1. Goal

Turn the `Cli` transport into a **generic, config-driven** path: a seat can point at *any* CLI binary (program + args + how the prompt is fed and the reply read), instead of Hadron shipping one bespoke Rust adapter per vendor. Fold the existing `agy` CLI onto this generic path as a built-in **preset**, and **delete the bespoke `claude.rs`** (Claude is reached over ACP only — the user's explicit "ACP-only for Claude" decision). One CLI adapter to maintain; new CLIs are a config change, not a code change — the same principle the ACP transport already stands on.

## 2. Approach (and why)

**One `CliQuark<R>` adapter driven by a `CliSpec`.** `agy`'s hard-won, incident-hardened behaviours (the E2BIG `fit_prompt` argv guard, `--print-timeout`, `--continue` resident resume, its posture vocabulary, prompt-as-`--print`-arg) are **preserved as Rust behaviours gated by `CliSpec` fields** — selected, not deleted — and shipped as the built-in `CliSpec::agy()` preset. A full `CliProfile` trait system is **YAGNI**: with `claude.rs` gone, `agy` is the only non-trivial CLI left, and its output side is already trivial (`reply_to_outcome` = trim stdout, no token telemetry). When a second algorithmically-weird CLI appears, add a profile hook then.

Rejected alternatives: (a) keep two bespoke adapters — the thing this sub-project exists to remove; (b) a `CliProfile` trait now — premature abstraction for one preset.

## 3. Non-goals
- No change to the ACP transport, routing, prompt building, or permission-mode *semantics* (only which CLI flags a posture maps to, per-spec).
- No `Transport::Sdk` work (that is sub-project #3).
- No JSON/token-telemetry reply parsing — it left with `claude.rs`; raw-stdout only. (A `reply` format enum is a future extension point, not built now.)

## 4. Design

### 4.1 `CliSpec` — the CLI invocation shape (`hadron-lattice/src/team.rs`, serializable)

```rust
pub struct CliSpec {
    pub program: String,
    pub args: Vec<String>,                 // static leading args
    pub prompt: PromptChannel,             // where the prompt text goes
    pub model_flag: Option<String>,        // e.g. "--model"; None = never pass model
    pub resume: ResumeMode,                // None | Continue { flag }
    pub timeout: Option<TimeoutArg>,       // e.g. { flag: "--print-timeout", value: "29m" }
    pub posture: PostureMap,               // Mode -> Vec<String>; default empty (no gating flags)
    pub argv_guard: bool,                  // apply the E2BIG fit_prompt guard (agy needs it)
}

pub enum PromptChannel {
    Stdin,                                 // prompt piped to stdin (was claude's channel)
    Arg { flag: Option<String> },          // prompt is the value of `flag` (e.g. "--print"); None = positional
}
pub enum ResumeMode { None, Continue { flag: String } }   // Continue = agy's `--continue` (resume most-recent-in-cwd)
pub struct TimeoutArg { pub flag: String, pub value: String }
pub struct PostureMap { pub ask: Vec<String>, pub write: Vec<String>, pub auto: Vec<String>, pub bypass: Vec<String> }
```

All fields `#[serde(default)]` where sensible so a minimal custom-CLI seat need only give `program` + `prompt`.

### 4.2 Built-in preset: `CliSpec::agy()`

Encodes today's `agy.rs` exactly, so the existing `cli-agy` seat behaves byte-for-byte:
- `program: "agy"`, `prompt: Arg { flag: Some("--print") }`, `model_flag: Some("--model")`
- `resume: Continue { flag: "--continue" }`, `timeout: { "--print-timeout", "29m" }`, `argv_guard: true`
- `posture: { ask: ["--mode","plan"], write/auto: ["--mode","accept-edits"], bypass: ["--dangerously-skip-permissions"] }`

`CliSpec::preset(vendor) -> Option<CliSpec>`: `"agy" => Some(agy())`, else `None`.

### 4.3 Seat carries an optional `cli` spec

Add `#[serde(default)] pub cli: Option<CliSpec>` to `Seat`. Resolution for a `Cli`-transport seat (mirrors ACP's command-or-catalogue pattern):
- explicit `seat.cli` wins;
- else `CliSpec::preset(&seat.vendor)` (so `cli-agy` needs no config);
- else, if the seat has a bare `command` (program+args) — build a **generic** spec: `prompt: Stdin`, raw stdout, no model_flag/resume/timeout/posture/guard (the "pipe prompt in, read reply out" default that works for most CLIs);
- else error: "cli seat '{id}' on vendor {v:?} has no built-in preset — give it a `cli` spec or a `command`" (parallel to the ACP "give it a command" message).

`same_agent` destructures the new `cli` field (compiler forces the identity decision; a changed spec rebuilds the quark).

### 4.4 `CliQuark<R>` adapter (`hadron-gluon/src/adapter/cli.rs`, replaces `claude.rs` + `agy.rs`)

Interprets a `CliSpec` to build the `CliInvocation` (reusing `runner.rs`'s `CliInvocation`/`CliRunner`/`reply_to_outcome`, unchanged):
- **prompt**: `Stdin` → prompt on stdin, no prompt arg; `Arg{flag}` → prompt rides as the flag's value (or positional), stdin empty.
- **argv_guard**: when set, run the prompt through the ported `fit_prompt` (E2BIG guard) before placing it in argv. Preserved verbatim from `agy.rs`, with its tests.
- **resume**: `Continue{flag}` → track in-memory `resident: bool`; append `flag` once resident; on a resumed turn strip the field window (`without_field_window`, ported) so history isn't re-sent.
- **model_flag / timeout / posture**: append when set, from the turn's `Mode`.
- **reply**: raw stdout → `reply_to_outcome`.

### 4.5 Registry (`hadron-gluon/src/adapter/registry.rs`)

`QuarkKind::{Claude, Agy}` → **`QuarkKind::Cli(CliSpec)`**; `QuarkKind::Acp(AcpTarget)` unchanged. `from_seat` for `Transport::Cli` resolves the `CliSpec` per §4.3 and returns `Cli(spec)`. `build` constructs a `CliQuark` over `ProcessRunner`. `from_vendor` (the old `"claude"=>Claude, "agy"=>Agy` map) is replaced by the preset resolution.

### 4.6 Settings UI (`hadron-chamber`)

A minimal "custom CLI" path in the add-quark wizard: fields for `program`, `args`, and prompt channel (stdin vs a flag), persisted as a `Cli`-transport seat with a `cli` spec; id defaults to `cli-<vendor>` via `Transport::conventional_id`. **GUI cannot be driven headless on WSL2 — value-level logic is unit-tested; visual flow marked needs-your-eyes.**

## 5. Migration / back-compat
- The existing `cli-agy` seat (vendor `agy`, no `cli` spec) resolves to `CliSpec::agy()` → identical behaviour. Proven by porting `agy.rs`'s tests onto `CliQuark` + the agy preset.
- The disabled `cli-claude` seat (vendor `claude`, CLI transport): after `claude.rs` deletion there is no `claude` CLI preset, so a `Cli`+`claude` seat with no `cli`/`command` now errors on build. It is **disabled** in the live config (`enabled:false`), so it is never built — no runtime break. Documented; if the user ever wants a CLI Claude, they add an explicit `cli` spec/`command`. (Claude's supported path is ACP.)

## 6. Testing
- Port every `agy.rs` test onto `CliQuark` + `CliSpec::agy()` — argv guard (E2BIG), `--print-timeout`, `--continue` resident + field-window stripping, posture, prompt-as-`--print`-arg, cwd. Byte-for-byte behaviour parity is the acceptance bar.
- New: a **generic** CLI seat (stdin prompt, raw stdout) round-trips prompt→reply through `FakeRunner`.
- New: `CliSpec` serde round-trip; preset resolution (`agy`→preset, unknown+command→generic, unknown+neither→error).
- `same_agent` rebuilds on a changed `cli` spec.
- Full gate `cargo test --workspace --features gui` green.

## 7. Security note (Rule 7)
A `CliSpec` names a program Hadron will `spawn` in the quark's worktree — this is the same trust surface `agy.rs`/`claude.rs`/ACP `command` already have (the human's own `team.json` chooses what runs). No new *external* input: the spec comes from local config, not the network or the model. A custom CLI runs with the daemon's privileges in the shared tree (same as every existing adapter). No sandbox change. The generic default passes the prompt on stdin (no argv-injection surface); `Arg` placement is the human's explicit choice. No new attack surface versus the current CLI adapters.

## 8. Judgment calls (autonomous — flag for review)
- **Deleting `claude.rs` outright** (vs keeping a CLI-Claude preset): per your "ACP-only for Claude". Its JSON-envelope/token-telemetry/`--resume` smarts are dropped, not ported. Reversible from git if you want a `CliSpec::claude()` later.
- **`agy` folded to a preset, not left bespoke**: preserves behaviour via ported tests; the alternative (keep agy.rs) would leave two adapters, defeating the sub-project.
- **Raw-stdout reply only** (no JSON reply format): YAGNI now that claude's envelope is gone; `reply` enum is a noted extension point.
- **Generic default = stdin + raw stdout**: the safest, most-portable "pipe in / read out" shape; advanced knobs are opt-in via the `cli` spec.
