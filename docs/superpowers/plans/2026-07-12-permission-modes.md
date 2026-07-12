# Permission Modes & Quark Legibility — Implementation Plan

> **For agentic workers:** implement task-by-task, TDD, commit green after each. Steps use `- [ ]`.

**Goal:** Ship the Ask/Write/Auto/Bypass mode ladder (field-as-SSOT, per-quark over global, TOFU allow-list) with status-bar + roster `Tag`s, quark provider/model legibility, and minimal `team.json` seating.

**Spec:** `docs/superpowers/specs/2026-07-12-permission-modes-design.md` (read it first).

## Global Constraints

- Zero API spend in tests (mock quarks/runners only).
- Chamber may depend on lattice + gatekeeper (pure), never gluon.
- Hand-written `Kind` serde: matching serialize **and** deserialize arms.
- `cargo test --workspace` green after every task; `cargo clippy --workspace` + `--features gui` clean at the end.
- Fixed vocabulary; "Mode" is a UI label for the gatekeeper level.

---

### Task 1 — lattice: Mode, ModeSet, PermissionGrant.remember, QuarkCard provider/model

**Files:** `crates/hadron-lattice/src/quark.rs`, `crates/hadron-lattice/src/event.rs`, `crates/hadron-lattice/src/lib.rs` (re-export `Mode`).

**Produces:** `Mode{Ask,Write,Auto,Bypass}` (Default Ask); `Kind::ModeSet{mode}` (tag `mode_set`, target on event.to); `Kind::PermissionGrant{approved,remember}` (remember serde-default false); `QuarkCard{..,provider,model}`.

- [ ] `Mode` enum in quark.rs (or a new `mode.rs`), `#[serde(rename_all="snake_case")]`, `#[derive(..., Default)]` with `#[default] Ask`. Re-export from lib.rs.
- [ ] `QuarkCard`: add `pub provider: String`, `pub model: String`. Update `quark_card_round_trips` to set/assert them.
- [ ] event.rs: add `Kind::ModeSet { mode: Mode }`. Serialize arm: `kind=mode_set`, `mode`. Deserialize arm: `"mode_set" => Kind::ModeSet { mode: take_field(&mut map,"mode")? }`.
- [ ] event.rs: `Kind::PermissionGrant { approved, remember }`. Serialize both fields; deserialize `remember` via a default (missing → false). Use `map.remove("remember").map(from_value).transpose()?.unwrap_or(false)` or a `take_field_or_default`.
- [ ] Tests: `mode_round_trips` (each variant JSON), `mode_set_round_trips` (with a `to`), `permission_grant_remember_round_trips` (remember true/false), `permission_grant_without_remember_defaults_false` (deserialize legacy `{"kind":"permission_grant","approved":true}` → remember false), updated `quark_card_round_trips`.
- [ ] Fix construction sites broken by the new `remember` field: grep `PermissionGrant {` across the workspace; add `remember: false` (or the right value) everywhere (gatekeeper gate.rs, engine.rs prod + tests, chamber model.rs test). **Commit only when the whole workspace compiles + tests green.**

Run: `cargo test -p hadron-lattice` then `cargo test --workspace`.

---

### Task 2 — gatekeeper: delete Policy, add Mode decision core

**Files:** `crates/hadron-gatekeeper/src/matrix.rs`, `gate.rs`, `lib.rs`.

**Consumes:** lattice `Mode`, `Risk`, `Event`, `Kind`, `Actor`, `QuarkId`.
**Produces:** `AllowRules`, `resolve_mode`, `global_mode`, `allow_rules`, `decide(mode,risk,op,quark,rules)`, `Decision`, `grant_remembering`.

- [ ] Delete `Policy`, `Policy::locked_down/default`, old `decide(risk,policy)` and their tests.
- [ ] `pub type AllowRules = std::collections::HashSet<(QuarkId,String)>;`
- [ ] `resolve_mode(events,quark)`: iterate; track `global: Option<Mode>` (ModeSet with `to==None`) and `per: Option<Mode>` (ModeSet with `to==Some(quark)`), last-wins. Return `per.or(global).unwrap_or_default()`.
- [ ] `global_mode(events)`: last `ModeSet` with `to==None`, else `Mode::default()`.
- [ ] `allow_rules(events)`: walk; on `PermissionGrant{approved:true,remember:true}` with `to==Some(q)`, find the nearest preceding `PermissionReq` from `q` with no grant between → insert `(q, description)`. (Reuse a helper mirroring `pending_permission`'s pairing but scanning all.)
- [ ] `decide` per the §3.2 truth table. Only Auto+BashExec consults `rules`.
- [ ] `grant_remembering(pending) -> Event` = `PermissionGrant{approved:true,remember:true}` addressed to `pending.quark`. `grant(pending,approved)` now sets `remember:false`.
- [ ] lib.rs re-exports: `Mode, Decision, decide, resolve_mode, global_mode, allow_rules, AllowRules, grant, grant_remembering, pending_permission, PendingPermission, Risk`.
- [ ] Tests (exhaustive): `decide` all 8 mode×risk cells; Auto bash on/off allow-list; `resolve_mode` (global only / per-quark overrides global / unset→Ask / latest-wins); `global_mode`; `allow_rules` (a remember-grant teaches a rule; a plain grant does not; rule carries the right quark+op); `grant_remembering` shape.

Run: `cargo test -p hadron-gatekeeper` then `cargo test --workspace`.

---

### Task 3 — engine: mode-based permission hook

**Files:** `crates/hadron-gluon/src/engine.rs`.

- [ ] Remove the `policy` field, its init in `new`, and `with_policy`.
- [ ] In the permission hook: capture `let op = ask.description.clone();` before moving it into the req event. After appending the `PermissionReq`, compute `let mode = hadron_gatekeeper::resolve_mode(&events,&target); let rules = hadron_gatekeeper::allow_rules(&events);` (the top-of-loop `events` binding already holds all prior ModeSet/remember grants — sufficient). `match hadron_gatekeeper::decide(mode, risk, &op, &target, &rules) { AutoApprove => {append PermissionGrant{approved:true,remember:false} from Gluon→target; exchanges+=1; continue;} AskHuman => {append Status Waiting; return Ok(());} }`.
- [ ] Replace the 3 policy tests with mode tests (mock `PermissionQuark`, seed ModeSet events into the field before serving where needed):
  - `ask_mode_pauses_for_human` (default, no ModeSet) — req + Waiting, no grant.
  - `write_mode_auto_approves_edit_but_pauses_on_bash` — seed global `ModeSet{Write}`; an edit-risk ask auto-grants, a bash ask pauses.
  - `auto_mode_learns_on_always_allow` — global `ModeSet{Auto}`; first bash pauses; after appending `grant_remembering` for that op, a second identical bash ask auto-approves.
  - `bypass_mode_auto_approves_bash` — global `ModeSet{Bypass}`; bash auto-grants from Gluon (audit).
  - `per_quark_override_beats_global` — global `Ask` + per-quark `ModeSet{Bypass}` to the asking quark → auto-approves.
  - Keep a task-preservation assertion (`recorded[1]=="hello"`) in one of them.
- [ ] Fix the bin (`bin/hadron-gluon.rs`) and any other caller that used `with_policy` (none expected besides tests).

Run: `cargo test -p hadron-gluon` then `cargo test --workspace`.

---

### Task 4 — chamber model: modes + legibility derivation (pure)

**Files:** `crates/hadron-chamber/src/model.rs`, new `crates/hadron-chamber/src/team.rs` (read-only team loader) or reuse a shared loader.

- [ ] `ChamberView`: add `global_mode: Mode`.
- [ ] `RosterRow`: add `mode: Mode`, `mode_is_override: bool`, `provider: String`, `model: String`.
- [ ] `project(events, &team)`: compute `global_mode = gatekeeper::global_mode(events)`; per row, `mode = resolve_mode(events,&id)`, `mode_is_override = (resolve differs by having a per-quark ModeSet)` — track via a set of ids that have a `to==Some(id)` ModeSet; `provider/model` from `team` map (empty if absent). (Add a `team: &TeamView` param, default empty in tests.)
- [ ] Team loader: `team::load(path) -> Team` (serde of `{quarks:[{id,provider,model,flavor}]}`); `Team::get(id) -> Option<&Seat>`. Malformed/missing → empty. Tests: round-trip, missing→empty, lookup.
- [ ] Tests: `global_mode_surfaced`; `roster_row_carries_effective_mode_and_override_flag`; `roster_row_carries_provider_model_from_team`; existing render-row tests updated for `Kind::ModeSet` (add a compact display arm) and `PermissionGrant.remember`.
- [ ] `render_row`: add a `Kind::ModeSet` arm (e.g. `("mode → {mode:?}", "mode_set")`); `PermissionGrant` arm unchanged (ignore remember in display or append " (remembered)").

Run: `cargo test -p hadron-chamber` then `cargo test --workspace`.

---

### Task 5 — chamber app (GPUI, blind, compiles under --features gui)

**Files:** `crates/hadron-chamber/src/app.rs`.

- [ ] Import `Tag` (and `TagVariant` if needed) from `gpui_component`.
- [ ] `status_bar`: replace the two text spans with `status_tag(state)` + `mode_tag(global_mode)` (both `.outline()`), variants per §5.2. Make the mode tag clickable → `cycle_global_mode` (append `ModeSet{mode}` `to:None`) or a `PopupMenu` of the four modes.
- [ ] Roster row: add `provider · model` muted text + a per-quark mode `Tag`; click → `set_quark_mode(id, mode)` appending `ModeSet{mode}` `to:Some(id)`.
- [ ] `permission_toast`: insert an **Always allow** `text_button` → `answer_permission_remember` appending `gatekeeper::grant_remembering(&pending)`.
- [ ] Add `answer_permission_remember`, `cycle_global_mode`, `set_quark_mode` (mirror `answer_permission`: append event, re-read field, `project`, `cx.notify()`). Load team once (store on `Chamber`) and pass to `project`.
- [ ] **Remove** `god_mode_section`, `toggle_policy`, `god_toggle_row`, and the `.child(self.god_mode_section(cx))` in `terminal_pane`. Remove the now-unused `ChamberPrefs.policy` field + its default + its test (`god_mode_policy_persists`).
- [ ] `cargo build -p hadron-chamber --features gui` must link. Add a manual-verify checklist to a record doc.

Run: `cargo build -p hadron-chamber --features gui`; `cargo test -p hadron-chamber`.

---

### Task 6 — team seating: adapter --model + daemon bin wiring

**Files:** `crates/hadron-gluon/src/adapter/{claude.rs,agy.rs,registry.rs}`, new `crates/hadron-gluon/src/team.rs` (or reuse chamber's shape via a shared location — simplest: a `team` module in gluon; chamber has its own read-only copy or shares via lattice — keep gluon's authoritative), `crates/hadron-gluon/src/bin/hadron-gluon.rs`.

- [ ] Adapters gain `model: String`; `new(id,flavor,model,runner)`; `ClaudeQuark` adds `--model <model>` args; `AgyQuark` the agy flag. Update adapter tests to assert the model arg (mock runner).
- [ ] `registry`: build a quark from a `Seat{id,provider,model,flavor}`.
- [ ] `team.json` loader in gluon (serde + tests). If chamber also needs the type, define `Seat`/`Team` once (e.g. in lattice or a tiny shared module) — **decide at impl time**; default: duplicate a small read-only struct in chamber to preserve chamber↛gluon decoupling.
- [ ] bin: load `team.json` from the config dir; if present, seat real adapters and `Engine::serve`; if absent, keep DemoQuark demo + print a hint. No API call in any test.

Run: `cargo test -p hadron-gluon` then `cargo test --workspace`.

---

### Task 7 — records

- [ ] Manual-verify checklist doc (`docs/superpowers/plans/2026-07-12-permission-modes-verify.md`): mode tags render + recolor per tier; clicking cycles/sets and appends the right `ModeSet` line; roster shows provider·model + per-quark tag; toast Always-allow appends `remember:true` and the op is silent next time; terminal rail is full again (no toggles).
- [ ] `STATUS.md` in the worktree root: what's done/green, what's staged (GUI Add-Quark, orchestrator-turn Bypass), how to merge (ff), how to add a quark (`team.json`), how to spin agy.
- [ ] Update memory (`phase6-gatekeeper.md`, MEMORY.md).
- [ ] Final `cargo test --workspace` (several runs) + `cargo clippy --workspace --all-targets` + `--features gui` clean.
