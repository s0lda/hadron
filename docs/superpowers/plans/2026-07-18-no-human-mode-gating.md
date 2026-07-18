# No-Human-Mode Gating Implementation Plan (permissions §2)

> **For agentic workers:** REQUIRED SUB-SKILL: subagent-driven-development. Checkbox steps. **SECURITY-SENSITIVE — this edits the live permission gate. Every task is additive + inert behind a default-OFF toggle; adversarial review required.**

**Goal:** Under an explicit `No-Human-Mode` toggle (global `Bypass`), a worker command that would ask a human instead escalates to the orchestrator, which adjudicates against allow/deny lists; workers clamp to `Auto`. With the toggle OFF, behavior is byte-for-byte identical to today.

**Policy (user-confirmed 2026-07-18):** clamp + consult orchestrator (NOT bypass-for-all). Orchestrator stays `Bypass`; workers clamp to `Auto` and escalate; human deny-list is absolute; per-quark explicit `Bypass` override still allowed. Allow/deny = exact-match AND prefix/glob.

**Architecture:** Pure `decide()` gains a double-table + `Decision::AskOrchestrator` (matrix.rs). A default-OFF toggle gates the whole No-Human path. The engine implements suspend→adjudicate→resume, drivable headless with fake quarks.

**Tech Stack:** Rust (hadron-gatekeeper, hadron-gluon). cargo test.

## Global Constraints
- Baseline gate before/after: `cargo test --workspace --features gui` (full).
- INERT session: cargo test/check only, never run binaries; tempdirs; don't touch live ~/.hadron.
- **Rule 4 / additive:** with the toggle OFF, `decide()` and `resolve_mode` produce byte-for-byte today's results, INCLUDING global `Bypass` → `AutoApprove`. Prove it with a test that re-runs the entire pre-change matrix with the toggle off.
- **Deny is absolute:** a human deny-list match escalates (never auto-approves), even in No-Human-Mode, even if also allow-listed (deny wins).
- **Inert by construction:** `AskOrchestrator` is only ever returned when `no_human_mode` is ON. The suspend/resume loop only runs when the toggle is ON. Until then, the engine treats any `AskOrchestrator` as the existing `AskHuman` pause path.
- Adversarial review: reviewers hunt for ways the gate is bypassed or the human path regressed.
- One focused commit per task.

---

### Task 1: `Decision::AskOrchestrator` + double-table `decide()` (pure, matrix.rs)

**Files:** `crates/hadron-gatekeeper/src/matrix.rs`, tests inline. **Consumer to keep compiling:** `crates/hadron-gluon/src/engine.rs:1073,1085,1195` (add an arm treating `AskOrchestrator` as the existing `AskHuman` pause — inert until Task 3).

**Interfaces (Produces):**
- `Decision::AskOrchestrator` (schedule an orchestrator adjudication turn).
- `pub type DenyRules = HashSet<(QuarkId, String)>` (or a struct supporting glob) — per-quark denied ops.
- Glob/prefix matching helper `fn op_matches(pattern: &str, op: &str) -> bool` (exact OR prefix `foo*` / glob) — pure, tested.
- `decide(mode: Mode, global: Mode, no_human: bool, risk: Risk, op: &str, quark: &QuarkId, allow: &AllowRules, deny: &DenyRules) -> Decision`.

Resolution (spec §2, additive):
- If `!no_human`: behave EXACTLY as today (ignore `global`/`deny`; the current match on `(mode, risk)` + `allow`). AskOrchestrator never returned.
- If `no_human` (and only then):
  - resolved worker `mode == Bypass` → `AutoApprove`.
  - `deny` match (exact or glob) → `escalate` (deny wins, even if allow-listed).
  - `allow` match → `AutoApprove`.
  - else → `escalate`.
  - where `escalate = if global == Bypass { AskOrchestrator } else { AskHuman }`.

- [ ] **Step 1: Failing tests.** `additive_off_matches_todays_matrix` (run the full Ask/Write/Auto/Bypass × WorkspaceEdit/BashExec grid with `no_human=false`; assert byte-identical to the current `decide` behavior — snapshot the current results first). `deny_wins_over_allow_when_no_human` (op in both → escalate). `glob_deny_matches` (`git push*` denies `git push origin`). `no_human_escalates_to_orchestrator_only_under_global_bypass` (global Bypass → AskOrchestrator; global Auto → AskHuman). `worker_bypass_override_auto_approves`.
- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement** the variant, `DenyRules`, `op_matches` (exact + `*` glob — a tiny matcher or the `glob`/`globset` crate if already a dep; else hand-roll prefix+`*`), and the `no_human`-gated `decide`. Add the inert `AskOrchestrator => AskHuman-pause` arm at the engine's 3 match sites so the workspace compiles (behavior unchanged — AskOrchestrator can't occur yet since `no_human` is wired OFF in Task 3).
- [ ] **Step 4: Run** tests + full gate. Expect PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(gatekeeper): AskOrchestrator + double-table decide (no_human-gated, deny-wins, glob) — inert"`

---

### Task 2: Worker clamping under No-Human-Mode (matrix.rs / resolve wrapper)

**Files:** `crates/hadron-gatekeeper/src/matrix.rs` (or a small wrapper the engine calls), tests inline.

**Interface:** `fn effective_mode(events: &[Event], quark: &QuarkId, no_human: bool, is_orchestrator: bool) -> Mode` — wraps `resolve_mode`: when `no_human && global==Bypass && !is_orchestrator && no explicit per-quark override`, clamp to `Auto`; otherwise return `resolve_mode(...)` unchanged.

- [ ] **Step 1: Failing tests.** `clamp_off_is_resolve_mode` (no_human=false → identical to `resolve_mode` for all cases). `worker_clamps_to_auto_under_global_bypass` (no_human, global Bypass, worker, no override → Auto). `explicit_override_survives_clamp` (worker with a per-quark ModeSet Bypass → stays Bypass). `orchestrator_not_clamped` (is_orchestrator → stays Bypass).
- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement** `effective_mode`. Reuse `resolve_mode`/`has_override`/`global_mode`; do not duplicate their logic.
- [ ] **Step 4: Run** + full gate.
- [ ] **Step 5: Commit** — `git commit -m "feat(gatekeeper): worker clamp to Auto under No-Human-Mode (override survives)"`

---

### Task 3: Suspend → adjudicate → resume loop + the DO-NOT-ACTIVATE toggle (engine.rs / bin)

**Files:** `crates/hadron-gluon/src/engine.rs`, `crates/hadron-gluon/src/bin/hadron-gluon.rs`, tests inline (fake quarks).

**The toggle:** `no_human_mode` — read once (env var `HADRON_NO_HUMAN_MODE=1` or a `team.json`/config field), default **OFF**. Thread it into `decide`/`effective_mode` at the engine's call sites. With it OFF, the engine passes `no_human=false` → today's behavior exactly (AskOrchestrator never occurs; clamping off).

**The loop (only reached when toggle ON and `decide` returns `AskOrchestrator`):**
1. Worker turn suspends; engine appends a `PermissionReq` (reuse the kind) and marks the seat waiting-for-orchestrator.
2. On quiesce, if any seat waits, schedule an orchestrator turn (the seat holding the orchestrator role), injecting the pending request text.
3. Orchestrator appends a `PermissionGrant`/denial (honor deny-list absolutely — a denied op is never grantable).
4. Worker resumes: grant → proceed; denial → the op is refused (the turn continues without it, or aborts per the existing AskHuman-denied semantics).

- [ ] **Step 1: Failing tests (fake quarks — the headless-verifiable core).** `toggle_off_never_asks_orchestrator` (no_human off → a bash-exec under global Bypass auto-approves exactly as today). `worker_suspends_on_askorchestrator` (toggle on, worker hits a non-allow-listed bash under global Bypass → suspends + emits the request). `orchestrator_grant_resumes_worker` (a fake orchestrator appends PermissionGrant → the worker resumes and proceeds). `orchestrator_denial_refuses_op` (denial → op not run). `human_deny_is_absolute` (a deny-listed op → even the orchestrator can't grant it; stays refused). Use the engine's existing fake-quark harness.
- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement** the toggle read + threading, the suspend/mark/schedule/resume loop, and the deny-absolute guard. Keep the change at the existing permission seam (engine.rs:1073 area + the daemon quiesce loop). Update the inert `AskOrchestrator => AskHuman` arm from Task 1 to the real loop, still behind the toggle.
- [ ] **Step 4: Run** tests + full gate.
- [ ] **Step 5: Commit** — `git commit -m "feat(gluon): No-Human-Mode suspend/adjudicate/resume loop behind DO-NOT-ACTIVATE toggle (default off)"`

---

## Self-Review
- Spec §2 A (AskOrchestrator) → T1. B (double-table) → T1. C (worker clamp) → T2. D (loop) → T3. Toggle/inert → T3 (+ T1 inert arm). Deny absolute → T1 (deny-wins) + T3 (grant guard). Exact+glob → T1 (`op_matches`). Additive proof → T1/T2/T3 "toggle/no_human off = today". ✓
- Placeholder scan: `op_matches` glob impl is described (exact + `*`, or an existing crate — the implementer checks deps); the fake-quark harness is the existing one (the implementer locates it). No TBD. ✓
- Security: every task is additive + toggle-gated; the loop honors deny absolutely; adversarial review mandated. The residual risk (orchestrator LLM influenced by worker request text) is documented in the spec, not a code defect. ✓
