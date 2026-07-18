# No-Human-Mode gating (permissions spec §2) — spec + plan — **NOT IMPLEMENTED (design only)**

**Status:** implementation-ready spec + plan. **Deliberately NOT built in the autonomous session** — it changes the live security-gating path and the *policy* (an LLM auto-adjudicating what would otherwise ask a human) is the user's decision, not one to switch on unattended. Half-built security is worse than a reviewed plan.
**Date:** 2026-07-18
**Source:** §2 of `docs/superpowers/specs/2026-07-17-permissions-and-extensibility-design.md`
**Branch:** `feat/permissions-gating` (docs)

## Grounding: what exists today (verified)
`crates/hadron-gatekeeper/src/matrix.rs`:
- `enum Decision { AutoApprove, AskHuman }`.
- `decide(mode: Mode, risk: Risk, op: &str, quark: &QuarkId, rules: &AllowRules) -> Decision` — the mode ladder (Ask→AskHuman; Write→edit auto / bash ask; Auto→edit auto / bash allow-listed?auto:ask; Bypass→all auto).
- `global_mode(events)`, `resolve_mode(events, quark)`, `has_override(events, quark)`, `allow_rules(events) -> AllowRules` (`HashSet<(QuarkId, String)>` of remembered approvals).
- **Runtime consumer:** `crates/hadron-gluon/src/engine.rs:1073` matches `decide(...)` → `AutoApprove` proceeds, `AskHuman` pauses + emits a `PermissionReq` to the human. (Also `:1195`.) **This is why §2 is not inert — it edits the live gate.**

## The change (spec §2)
Add automated orchestrator adjudication under global `Bypass` ("No-Human Mode"): a command that would ask a human instead asks the **orchestrator** quark, which appends a grant/denial the worker resumes on.

### A. `Decision::AskOrchestrator` (matrix.rs)
Add the variant. **Additive invariant (Rule 4):** it is returned **only** when the global mode is `Bypass`. Every non-Bypass path returns `AskHuman` byte-for-byte as today. The existing human-approval path is untouched.

### B. Double-table `decide()` (matrix.rs)
`decide()` gains the **global** mode (distinct from the quark's resolved mode) to pick the escalation target, and two tables per spec §2:
- **Global allow-list:** command prefixes/patterns permitted without any intervention (extends today's per-quark `AllowRules` with a global tier).
- **Per-quark deny-list:** prefixes explicitly prohibited for a quark. **Deny wins** (matches both allow and deny → escalate).

New signature (illustrative): `decide(mode: Mode, global: Mode, risk: Risk, op: &str, quark: &QuarkId, allow: &AllowRules, deny: &DenyRules) -> Decision`.

Resolution (spec §2 flow), with `escalate(global) = if global == Bypass { AskOrchestrator } else { AskHuman }`:
1. resolved worker `mode == Bypass` → `AutoApprove` (standing authority).
2. op does NOT match the global allow-list → `escalate(global)`.
3. op matches the quark's deny-list → `escalate(global)` (deny wins over an allow).
4. else → `AutoApprove`.
Keep the existing risk/mode ladder as the base; the tables refine the BashExec escalation decision. **Every branch that is not `global == Bypass` yields exactly today's `AskHuman`** — prove this with a test that runs the full pre-change matrix with `global != Bypass` and asserts byte-identical results.

### C. Global-bypass worker clamping (spec §2)
When the **global** mode resolves to `Bypass`, a worker's resolved mode defaults to `Auto` (not `Bypass`) unless it carries an explicit per-quark override. Keeps the orchestrator at `Bypass` while workers run monitored under `Auto`. Implement in `resolve_mode` (or a wrapper the engine calls), guarded so it only applies to non-orchestrator seats and only absent an explicit override.

### D. Suspend → adjudicate → resume loop (engine.rs / bin/hadron-gluon.rs) — the involved part
1. `decide()` returns `AskOrchestrator` → the worker turn suspends; the engine appends a `PermissionReq` (reuse the existing kind) and marks the seat waiting-for-orchestrator.
2. In the daemon's main loop, **when the swarm quiesces**, if any worker is waiting-for-orchestrator, schedule a turn for the orchestrator (`@cli-agy` / whoever holds the role), injecting the pending request's details into its prompt.
3. The orchestrator appends a `PermissionGrant`/denial.
4. The worker resumes at the next engine step.
**Verifiability (this IS drivable headless):** use the engine's existing fake quarks — a fake orchestrator that appends `PermissionGrant`, and assert the suspended worker resumes and proceeds. No network needed. This is where the real test effort goes.

## Inert / activation
Gate the entire `AskOrchestrator` behavior behind a prominent **`DO-NOT-ACTIVATE-until-reviewed`** flag (a config/env toggle defaulting OFF). With it off, `decide()` never returns `AskOrchestrator` (global-Bypass escalation falls back to `AskHuman`), so the branch is dormant and the human path is byte-for-byte unchanged. The engine's new `AskOrchestrator` match arm, until the toggle is on, degrades to the `AskHuman` behavior — so even a partially-wired state is safe.

## Testing (all headless)
- `decide()` double-table: allow-hit→auto; allow-miss→escalate; deny-hit→escalate (deny-wins over allow); worker-Bypass→auto. For EACH: `global == Bypass` → `AskOrchestrator`; `global != Bypass` → `AskHuman`.
- **Additive proof:** re-run the entire pre-change matrix with `global != Bypass`; assert byte-identical to today.
- Worker clamping: global Bypass + no override → worker resolves `Auto`; with an explicit override → the override.
- Loop (fake quarks): a worker hitting `AskOrchestrator` suspends; a fake orchestrator grants; the worker resumes and proceeds; a denial keeps it paused/aborts the op.
- Full gate `cargo test --workspace --features gui`.

## Security note (Rule 7) — this is the crux
This **weakens the default human gate** the moment the toggle is on: under global Bypass, worker commands that would have asked a human are auto-adjudicated by an LLM. That is a deliberate trust transfer and MUST be the user's explicit choice. Mitigations baked into the design: (a) additive — off by default, human path unchanged; (b) worker clamping keeps workers at `Auto`, not `Bypass`; (c) deny-list is honored deny-wins; (d) the orchestrator's grant/denial is a field event (auditable). Residual risk: the orchestrator LLM can be prompt-influenced by the worker's request text — treat the injected request as untrusted, and the orchestrator's judgment is not a security boundary against a compromised worker.

## DESIGN QUESTIONS FOR THE USER (do not decide these unattended)
1. **Should `AskOrchestrator` ever auto-approve a command the human explicitly deny-listed, or is a human deny an absolute floor even in No-Human-Mode?** (Recommend: human deny is absolute — the orchestrator can't override it.)
2. **LLM-approves-LLM trust:** is the orchestrator adjudicating worker commands acceptable for your threat model, given the orchestrator is itself an LLM that the worker's request text can influence? What command classes (if any) must ALWAYS reach a human even under global Bypass (e.g. `rm -rf`, network exfil, credential access, `git push`)?
3. **Global allow-list source & syntax:** where do allow/deny prefixes live — `team.json`, a new `.hadron/permissions.json`, field `ModeSet`-style events? Exact-match (today's `AllowRules`) or prefix/glob?
4. **Worker clamping to `Auto` under global Bypass** — agreed, or should some workers be individually promotable to Bypass?
