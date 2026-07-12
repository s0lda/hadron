# Real-Quark Gating — Design

**Goal:** Make the permission modes actually shape what a *real* quark (the `claude`/`agy` CLIs) can do, closing the "gate is dormant for real quarks" gap that shipped with the permission-modes work.

**Status of the gap:** The mode ladder (Ask/Write/Auto/Bypass) exists and is field-driven, but real adapters return `TurnOutcome { permission: None }` and the vendor CLI runs its own tools inside its own subprocess. So the mode never influenced a real turn. This design makes the **resolved mode select the CLI's permission posture at invocation time.**

---

## What the installed CLIs actually allow (probed, not assumed)

Probed against `claude 2.1.207` and `agy 1.1.1` in headless (`-p`/`--print`) mode. **These findings are load-bearing — the design is built on the observed behavior, not the help text.**

**claude 2.1.207:**
- **No `--permission-prompt-tool`** flag exists. The clean MCP-mediated "route each permission request to a live approver" hook is not available in this version. Live, mid-turn, field-mediated human approval is therefore **not reachable** headless.
- Tools are **binary** per posture: either present-and-auto-allowed, or absent.
  - `--permission-mode plan` → **read-only**: the model proposes and executes nothing (verified: asked to create a file, it drafted a plan and created nothing).
  - `--permission-mode acceptEdits` / `default` / `manual` / `dontAsk` → in headless print mode these **all auto-run bash** (verified: `cargo --version` ran under every one) and auto-apply edits (verified: a `Write` was applied to disk). There is **no posture that records a mid-turn denial** — `permission_denials` stayed `[]` in every case, so the "attempt → deny → capture the command" path does not exist here.
  - `--allowedTools "Bash(echo:*)"` is **permissive-additive**, not a restrictive whitelist — a non-listed `ls` still ran. It cannot express "only these commands."
  - `--disallowedTools "Bash"` **removes** Bash from the toolset entirely — the model can't attempt it and says so in prose; no denial is recorded.
  - `--permission-mode bypassPermissions` → everything, including operations even `acceptEdits` would hold back.
- Result envelope (`--output-format json`): `result` (reply text), `session_id`, `permission_denials` (array), and `usage` with **`input_tokens`/`output_tokens`** — there is **no `total_tokens`** (the current adapter reads `usage.total_tokens`, so it always records 0 — fixed here).

**agy 1.1.1:**
- Has `--mode {plan|accept-edits}`, `--sandbox`, `--dangerously-skip-permissions`, `--print`. No `--output-format json` — print output is prose.
- Model ids are **display names** (`agy models`): e.g. `Gemini 3.5 Flash (Low)`, `Gemini 3.1 Pro (High)`, `Claude Opus 4.6 (Thinking)`. The old `team.example.json` value `gemini-3-pro` is wrong.
- Arg parsing is finicky (a naive `--mode plan "<prompt>"` confused it). agy's live invocation shape is **not yet verified** — its mapping is implemented by the same pure function and unit-tested, but marked needs-live-validation.

## Consequence: only a *turn-granular* gate is reachable

Because the CLI does all its work inside one subprocess turn and exposes no mid-turn approval hook, Hadron cannot approve individual commands live. What it **can** do is choose the posture the whole turn runs under. That yields **turn-granular propose-and-wait**:

- In **Ask**, the quark runs read-only (`plan`): it proposes, changes nothing, and posts the proposal to the field. The human reviews, **escalates the quark's mode** (roster mode tag → Write/Bypass), and re-addresses it to execute. The proposal is already in the field, so a fresh higher-mode turn acts on it. This is genuine propose-and-wait, matching the user's "Ask is just conversation; the user must permit."

## Mode → posture mapping

| Mode | claude posture | agy posture | Effect |
|------|----------------|-------------|--------|
| **Ask** | `--permission-mode plan` | `--mode plan` | Read-only. Proposes; executes nothing. |
| **Write** | `--permission-mode acceptEdits --disallowedTools Bash` | `--mode accept-edits` | Auto file edits; **no ungated bash**. |
| **Auto** | *same as Write* | *same as Write* | Degrades to Write (see below). |
| **Bypass** | `--permission-mode bypassPermissions` | `--dangerously-skip-permissions` | Everything runs. |

**Why Auto degrades to Write, not to all-bash.** Auto's real semantics are "auto-run *allow-listed* commands, ask me about the rest" — a per-command TOFU list. That is **not expressible** against this CLI headless (no deny signal, `--allowedTools` is not restrictive). Faced with a choice, Auto must degrade **toward safety**: it takes Write's posture (edits auto, bash blocked), never `acceptEdits`-with-all-bash. Running every command silently under a mode the user picked expecting to be asked would recreate the exact safety-expectation trap this work exists to close. So **Auto = Write until real TOFU lands.**

## Deferred: real per-command TOFU (the honest next milestone)

True Auto (and live Write "ask me about this bash") needs a mid-turn approval callback the CLI doesn't offer. The reachable path is the **Claude Agent SDK's `canUseTool` callback** (or a future `claude --permission-prompt-tool`), where each tool call is intercepted, turned into a Hadron `permission_req`, and blocked on a grant. That is a separate, larger integration (a resident SDK-driven adapter rather than one-shot `claude -p`) and is **out of scope here**. This design gets real behavioral gating shipped against the CLIs that exist today; the SDK path upgrades Auto/Write fidelity later without changing the mode vocabulary.

## Plumbing

- **`Projection` gains `mode: Mode`** (lattice; defaults to `Ask`). The per-turn context the adapter already receives now carries the resolved mode.
- **The engine resolves the mode before `excite`** (`gatekeeper::resolve_mode(events, quark)`) and sets `projection.mode`. The existing *post*-turn self-declared-permission path (a quark returning `TurnOutcome.permission`) is unchanged — that governs the mock/self-declaring flow and the chamber toast.
- **Each adapter maps the mode to argv** via a pure `posture_args(mode) -> Vec<String>` function, appended to the invocation. Unit-tested against the existing `FakeRunner` (zero API spend), mirroring the `--model` assertion pattern.
- **Token fix:** read `usage.input_tokens + usage.output_tokens`.
- **`permission_denials`:** captured defensively (a code path + comment); always `[]` in headless print today, so no behavior hangs off it — it's the hook the SDK path will use.

## Testing & spend

- All mapping logic is a pure function unit-tested against `FakeRunner` — **zero API spend in the test suite** (the standing rule).
- Live validation of the claude mapping was done once, manually, with cheap `haiku` turns (Ask=plan proposes nothing; Write applies an edit and blocks bash; Bypass runs bash). agy live-validation is deferred (finicky parse + display-name models).

## Out of scope (explicit)

- SDK `canUseTool` per-command gating (the real-TOFU upgrade).
- agy live-flag verification and its prose-output parsing.
- Any change to the mode vocabulary, the field-as-SSOT model, or the chamber UI.
