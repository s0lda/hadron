# Role preference dispatch + role locks — design

**Status:** approved-direction (Jake 2026-07-20: "combination of B and C … roles are preference … also we can do lock in roles")
**Date:** 2026-07-20
**Author:** acp-claude
**Builds on:** `2026-07-18-role-routing-design.md` (seat `roles` + `exclusive`, `@role` soft routing — SHIPPED: `seat.rs:89`, `router/mod.rs:128` `card_for_role`, personas loader `personas.rs`)

## 1. What Jake asked for

1. Roles like **Architect** exist as first-class labels a task can be designed for.
2. **Preference, not requirement**: a task designed for Architect goes to the Architect seat when one is enabled; if no seat holds the role, the task deploys to any other quark — never stalls.
3. **Role-shaped behavior**: a role-holder acts differently (design-first skills, review authority) — "pick best models for task".
4. **Hard locks**: some seats must NEVER receive some task kinds (e.g. an image-model seat can't be handed a write-plans task). Locks are the only hard filter; roles stay soft.

## 2. What already exists (reuse, don't rebuild)

| Piece | Where | State |
|---|---|---|
| `Seat.roles: Vec<String>` + `exclusive` | `hadron-lattice/team/seat.rs:89` | shipped, serde-back-compat |
| `@role` mention → first enabled role-holder | `hadron-gluon/router/mod.rs:128` | shipped |
| Personas (`.md` + `preferred_role`) | `hadron-gluon/personas.rs` | loader shipped; body injection unverified |
| Exclusive-seat dispatch filter | engine dispatch | shipped (`exclusive-seats-guard-the-direct-assign-gap`) |
| Skill selection per turn | engine (gluon invariants) | shipped (`skills-are-picked-by-the-engine`) |

Dead seam warning: do NOT hang this on `Kind::Assign` / `requested` invariants — that path never executes (`the-assign-invariants-seam-is-dead`).

## 3. Design

### 3.1 Settings UI — assign roles to seats (the missing entry point)
- Provider settings panel gets a **Roles** field (comma/chip input) writing `Seat.roles`; roster row context menu gets "Set roles…". The wizard stops hardcoding `roles: vec![]` (`settings/providers.rs:519`).
- Suggested-but-not-enforced vocabulary shown as placeholder: `architect`, `reviewer`, `executor`, `researcher`. Roles remain free strings — the router already matches case-insensitively; an enum here would break user-defined roles.

### 3.2 Task→role classification + soft preference (the B half)
- A task is "designed for a role" when (a) it opens with `@<role>` (already routes), or (b) the engine's existing skill classifier maps the turn's starting skill to a role via a small static table in the engine (SSOT, one table):
  - `writing-plans`/`brainstorming` → `architect`
  - `requesting-code-review`/`reviewing-work` → `reviewer`
  - `executing-plans`/`subagent-driven-development` → `executor`
- At dispatch, among eligible seats the role-holder is **preferred** (stable: first by roster order, matching `card_for_role`); if none is enabled/non-depleted, dispatch proceeds exactly as today. No new failure mode, no stall.

### 3.3 Role-shaped prompts (the C half)
- One `.md` per role in `.hadron/roles/` (global `~/.hadron/roles/` + repo, repo wins) — **reuse the personas loader machinery** (`personas.rs` pattern: front-matter `name:`, body). When a turn is dispatched to a seat holding role X *for a role-matched task*, the role body is appended to the turn prompt (same injection point as the active skill body).
- Verify-it-runs requirement: the persona **body** injection has no proven caller today — wiring it is part of this work, not assumed.

### 3.4 Hard locks (new, small)
- New field `Seat.deny_skills: Vec<String>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]` — old team.json decodes unchanged). Names entries of the known skill index (validated on Settings save against the engine's skill list; unknown names rejected in UI, tolerated in file).
- Dispatch filter: a seat whose `deny_skills` contains the turn's selected starting skill is **ineligible** for that task — same filter point as `exclusive`, same "report the gap, never stall" behavior when nobody remains.
- Why skill-keyed and not role-keyed: the skill IS the engine's task-kind classification (SSOT); a parallel task-kind enum would drift (Rule 3).
- Carrying `deny_skills` to `QuarkCard` mirrors how `exclusive` travels. NOTE `adding-a-field-to-Seat-breaks-the-gui-build`: fix the `app.rs` struct literal in the same commit and run BOTH gates.

## 4. Explicitly out of scope (YAGNI)
- Group hierarchy (group leader tiers) — separate design when Jake picks it up.
- Auto-detecting roles from model names.
- Load-balancing among multiple same-role seats (roster order stays the tie-break).

## 5. Testing
- Serde: `deny_skills` default/round-trip; `same_agent` treats a deny change as a different agent.
- Dispatch: role-matched task prefers role-holder; falls back when absent/disabled/depleted; `deny_skills` excludes a seat and the gap is reported when nobody remains.
- Prompt: a role-matched turn to a role-holder contains the role body; a non-matched turn does not.
- Both gates: `cargo test --workspace` AND `cargo test --workspace --features gui`.

## 6. Security note (Rule 7)
Roles/locks only narrow or reorder which local seat services a task; a lock is a restriction, never a capability. The role `.md` bodies are prompt text loaded from Jake's own `.hadron` dirs — same trust level as skills/personas today. No new attack surface.
