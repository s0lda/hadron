---
author: acp-claude
status: draft
---

# Role Preference Dispatch + Role Locks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Roles are a soft preference at dispatch (Architect-shaped tasks prefer the Architect seat, fall back to anyone), role-holders get a role prompt body, and `deny_skills` is a hard lock (a seat never receives a task whose starting skill it denies).

**Architecture:** Extend the shipped role-routing core (seat `roles`, `@role` mentions, `exclusive` filter) rather than adding a parallel system. The task→role mapping keys off `skills::select` — the engine's existing task classifier — so there is exactly one notion of "what kind of task is this". Locks are enforced at the same dispatch choke point as `exclusive` (`engine/run.rs`), preference at the same resolver that already does persona→role (`engine/routing.rs`).

**Tech Stack:** Rust; hadron-lattice (types), hadron-gluon (engine/prompt), hadron-chamber (Settings UI).

**Spec:** `docs/superpowers/specs/2026-07-20-role-preference-and-locks-design.md` (approved by Jake 2026-07-20).

## Global Constraints

- Serde back-compat: every new field defaults so an old `team.json` decodes byte-identically (`#[serde(default, skip_serializing_if = ...)]`).
- BOTH gates green per task: `cargo test --workspace` AND `cargo test --workspace --features gui` (memory: `workspace-gate-skips-gui`).
- A new `Seat`/`QuarkCard` field breaks the GUI build via struct literals in `app.rs` — fix construction sites in the SAME commit (memory: `adding-a-field-to-Seat-breaks-the-gui-build`, `stage-every-site`).
- Never stall dispatch: every filter that empties the candidate set must report via the existing `reroute_blocked` path, exactly like the `exclusive` filter.
- Do NOT touch `Kind::Assign`/`requested` invariants — dead seam (memory: `the-assign-invariants-seam-is-dead`).

---

### Task 1: `deny_skills` on Seat, SeatOverride, QuarkCard

**Files:**
- Modify: `crates/hadron-lattice/src/team/seat.rs` (Seat ~line 89 block, `same_agent` ~142, ctor ~174, SeatOverride ~264, override resolve/delta)
- Modify: `crates/hadron-lattice/src/quark.rs` (QuarkCard, next to `roles`/`exclusive`)
- Modify: every `Seat`/`QuarkCard` struct-literal construction site (search `roles: vec![]` and `exclusive:` across `crates/` including `hadron-chamber/src/app` and test fixtures)
- Test: `crates/hadron-lattice/src/team/` existing serde test module

**Interfaces:**
- Produces: `Seat.deny_skills: Vec<String>`, `SeatOverride.deny_skills: Option<Vec<String>>`, `QuarkCard.deny_skills: Vec<String>` — consumed by Task 3 (dispatch) and Task 5 (Settings UI).

- [ ] **Step 1: Failing serde test** — in the seat test module:
```rust
#[test]
fn deny_skills_defaults_empty_and_round_trips() {
    let json = r#"{"id":"x","vendor":"v","model":"m","flavor":"Worker","transport":"Cli"}"#;
    let seat: Seat = serde_json::from_str(json).expect("legacy seat decodes");
    assert!(seat.deny_skills.is_empty());
    let mut seat2 = seat.clone();
    seat2.deny_skills = vec!["writing-plans".into()];
    let back: Seat = serde_json::from_str(&serde_json::to_string(&seat2).unwrap()).unwrap();
    assert_eq!(back.deny_skills, vec!["writing-plans"]);
    assert!(!seat.same_agent(&seat2), "a lock change is a different agent");
}
```
(Adjust the minimal JSON to whatever the existing legacy-decode test in that module uses — copy its fixture.)
- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p hadron-lattice deny_skills` → compile error (field missing).
- [ ] **Step 3: Add the fields** — mirror `roles` exactly:
```rust
/// Skill names this seat must NEVER be handed (hard lock, e.g. an image model
/// never gets `writing-plans`). Matched against `skills::select`'s chosen name.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub deny_skills: Vec<String>,
```
Add to `same_agent` destructure+compare, `Seat::cli()` ctor default, `SeatOverride` (`Option<Vec<String>>`, absent = inherit) + `resolve_team` application + `seat_override_delta`, and `QuarkCard` (`#[serde(default)]`). Fix every construction site the compiler and `grep -rn "exclusive:" crates/` reveal.
- [ ] **Step 4: Both gates** — `cargo test --workspace` then `cargo test --workspace --features gui` → all green.
- [ ] **Step 5: Commit** — `git add` each touched file by name; `git commit -m "feat(lattice): Seat/QuarkCard deny_skills field (role hard locks)"`.

### Task 2: Task→role table beside the skill selector

**Files:**
- Modify: `crates/hadron-gluon/src/skills.rs` (re-export) and `crates/hadron-gluon/src/skills/select.rs`
- Test: same module's tests

**Interfaces:**
- Produces: `pub fn preferred_role(skill_name: &str) -> Option<&'static str>` — consumed by Tasks 3–4.

- [ ] **Step 1: Failing test**
```rust
#[test]
fn skills_map_to_their_preferred_role() {
    assert_eq!(preferred_role("writing-plans"), Some("architect"));
    assert_eq!(preferred_role("brainstorming"), Some("architect"));
    assert_eq!(preferred_role("requesting-code-review"), Some("reviewer"));
    assert_eq!(preferred_role("reviewing-work"), Some("reviewer"));
    assert_eq!(preferred_role("executing-plans"), Some("executor"));
    assert_eq!(preferred_role("subagent-driven-development"), Some("executor"));
    assert_eq!(preferred_role("systematic-debugging"), None);
}
```
- [ ] **Step 2: Run, expect FAIL** — `cargo test -p hadron-gluon preferred_role` → not found.
- [ ] **Step 3: Implement** — a single `match` in `select.rs` (this file already owns "engine picks the skill", so the role mapping lives with it — SSOT):
```rust
/// The role a task of this kind prefers (spec 2026-07-20 §3.2). None = no preference.
pub fn preferred_role(skill_name: &str) -> Option<&'static str> {
    match skill_name {
        "writing-plans" | "brainstorming" => Some("architect"),
        "requesting-code-review" | "reviewing-work" => Some("reviewer"),
        "executing-plans" | "subagent-driven-development" => Some("executor"),
        _ => None,
    }
}
```
Re-export from `skills.rs` alongside `select`.
- [ ] **Step 4: Test green, commit** — `git commit -m "feat(skills): preferred_role table keyed on the engine's skill names"`.

### Task 3: Hard lock filter at dispatch

**Files:**
- Modify: `crates/hadron-gluon/src/engine/run.rs` (directly AFTER the `exclusive` filter block at ~line 121–133 — same shape, same `reroute_blocked` reporting)
- Test: `crates/hadron-gluon/src/engine/tests.rs` (copy the existing exclusive-filter test's harness)

**Interfaces:**
- Consumes: `QuarkCard.deny_skills` (Task 1), `skills::select` (existing) — the same task text already available as `fallback_task`/events at that point.

- [ ] **Step 1: Failing engine test** — mirror the existing exclusive-skip test: seat a card with `deny_skills: vec!["writing-plans".into()]`, send it a task whose text selects `writing-plans` (e.g. contains "implementation plan"), assert the turn is skipped and the field contains the ⚠️ lock message; then a plain task reaches it normally.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** — inside the same `if let Some(card) = self.roster.iter().find(...)` block that hosts the exclusive check:
```rust
// Hard lock (spec 2026-07-20 §3.4): a seat that denies this task's starting
// skill never receives the turn. Same reporting discipline as `exclusive`:
// say so in the field, never drop silently.
let task_text = fallback_task.as_deref().unwrap_or_default();
let selected = crate::skills::select(task_text); // reuse the engine's real selector call shape at this site
if let Some(skill) = selected {
    if card.deny_skills.iter().any(|d| d.eq_ignore_ascii_case(skill.name())) {
        let msg = format!(
            "⚠️ @{} locks out '{}' tasks (deny_skills); skipping.",
            target.as_str(), skill.name()
        );
        self.reroute_blocked(&target, &msg).await?;
        continue;
    }
}
```
NOTE for implementer: read how `select` is actually invoked for the turn (signature may take more context than bare text — match the real call used when building the projection, and reuse the SAME inputs so the lock and the injected skill can never disagree).
- [ ] **Step 4: Both gates green. Commit** — `git commit -m "feat(engine): deny_skills hard lock at dispatch, reported not stalled"`.

### Task 4: Soft role preference at target resolution

**Files:**
- Modify: `crates/hadron-gluon/src/engine/routing.rs` (the resolver that already runs the persona pass at ~line 45/180 and `card_for_role` in `router/mod.rs:128`)
- Test: `crates/hadron-gluon/src/engine/tests.rs`

**Interfaces:**
- Consumes: `skills::preferred_role` (Task 2), `card_for_role` (existing router helper).
- Produces: broadcast/general tasks with a role-shaped skill prefer the role-holder.

- [ ] **Step 1: Failing test** — roster of two enabled workers, second carries role `architect`; a `@team`-addressed (or unaddressed-fallback, whichever the existing tests exercise) task whose text selects `writing-plans` must excite the architect seat first; with no architect seated, the same task must dispatch exactly as today (assert same target as a control run without roles).
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** — in the resolver, after id/alias/role/persona passes produce a candidate SET (broadcast/general case only — an explicit `@id`/`@role` is never overridden):
```rust
// Soft preference (spec 2026-07-20 §3.2): among the otherwise-eligible cards,
// a task whose starting skill maps to a role bubbles the role-holder first.
// Preference ONLY — if no enabled card holds the role, the order is unchanged.
if let Some(role) = crate::skills::preferred_role(selected_skill_name) {
    if let Some(preferred) = card_for_role(roster, role) {
        targets.sort_by_key(|id| if *id == preferred.id { 0 } else { 1 }); // stable sort keeps roster order for the rest
    }
}
```
Implementer: anchor this where the resolved broadcast target list is materialized (follow `human_mentions`' output through routing.rs — the persona pass at :45 is the model to copy). If dispatch takes only the FIRST of the list, this reorder IS the preference; if it excites all of them, apply preference only where a single fallback worker is chosen, and say so in the report.
- [ ] **Step 4: Both gates green. Commit** — `git commit -m "feat(engine): role-holders preferred for role-shaped tasks, soft fallback"`.

### Task 5: Role prompt bodies (`.hadron/roles/*.md`)

**Files:**
- Modify: `crates/hadron-gluon/src/personas.rs` — generalize `load_dir` reuse; add `pub fn load_roles(global: Option<&Path>, repo: Option<&Path>) -> Vec<Persona>` reading `roles/` dirs with the SAME front-matter/merge machinery (a role file needs only `name:` + body; `preferred_role` ignored)
- Modify: `crates/hadron-gluon/src/engine.rs` (beside `loaded_personas` ~line 494: `loaded_roles()`), thread the matched role body into the projection the same way the skill body travels
- Modify: `crates/hadron-gluon/src/adapter/prompt/mod.rs` — render the role body under a `# Your role` heading, directly after the skill-for-this-turn section
- Test: `personas` tests (loader) + `adapter/prompt/tests.rs` (injection)

**Interfaces:**
- Consumes: `preferred_role` (Task 2), seat `roles` (existing).
- Produces: a turn dispatched to a seat holding role R, for a task whose skill maps to R, carries the `roles/R.md` body; any other turn carries none.

- [ ] **Step 1: Failing loader test** — write `roles/architect.md` with `name: architect` front-matter into a tempdir, `load_roles` returns it; repo dir overrides global by name (copy the personas merge test).
- [ ] **Step 2: Failing prompt test** — build a projection whose quark holds `architect` and whose skill is `writing-plans`: prompt contains `# Your role` + the body; same projection with skill `systematic-debugging`: it does not.
- [ ] **Step 3: Implement loader + engine getter + projection field + prompt section.** Projection grows an `Option<String>` field (envelope-style additive — nothing breaks when None). Verify the caller chain end-to-end: engine sets it, prompt renders it — this closes the spec's "persona body injection unproven" flag for roles.
- [ ] **Step 4: Both gates green. Commit** — `git commit -m "feat(prompt): role .md bodies injected for role-matched turns"`.

### Task 6: Settings UI — roles + deny_skills on a seat

**Files:**
- Modify: `crates/hadron-chamber/src/app/settings/providers.rs` (the wizard that writes `roles: vec![]` at ~line 519, and the seat edit panel)
- Test: `app::settings::tests`

**Interfaces:**
- Consumes: `Seat.roles`, `Seat.deny_skills` (Task 1); the engine skill-name list for validation (`hadron_gluon::skills::builtins()` names — chamber already links the crates it needs; if gluon isn't a chamber dep, validate against a re-exported const list in lattice instead and note it).

- [ ] **Step 1: Failing test** — saving a provider with roles text `"Architect, reviewer"` writes `roles == ["architect","reviewer"]` (trimmed, lowercased); deny text `"writing-plans, nope-skill"` keeps `writing-plans`, drops `nope-skill`, and surfaces a validation notice string.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** — two text inputs on the provider form (comma-separated; same input pattern as existing fields in that file), parse helper `fn parse_roles(&str) -> Vec<String>` (trim, lowercase, drop empties, dedupe). Unknown deny names: reject in UI with the notice, tolerate if already in the file (engine treats unknown names as never-matching — harmless).
- [ ] **Step 4: GUI gate green** (`cargo test --workspace --features gui`). Commit — `git commit -m "feat(settings): edit seat roles and deny_skills from the provider panel"`.

---

## Verification at the end

- Full: `cargo test --workspace && cargo test --workspace --features gui` → all crates green.
- Live smoke (needs daemon rebuild + restart, memory `daemon-recompile-during-swarm-changes`): seat a quark with role `architect`, send a "write an implementation plan for X" task with no addressee → the architect takes it; give another seat `deny_skills: ["writing-plans"]` and address it directly with the same task → field shows the ⚠️ lock skip.

## Self-review notes (done at authoring)

- Coverage vs spec: §3.1→Task 6, §3.2→Tasks 2+4, §3.3→Task 5, §3.4→Tasks 1+3. Groups/hierarchy stay out (spec §4).
- Two flagged uncertainties are called out INSIDE their tasks (the exact `select` call signature in Task 3; where the broadcast target list materializes in Task 4) with the instruction to match the real call sites — the implementer must resolve them by reading, not guessing.
