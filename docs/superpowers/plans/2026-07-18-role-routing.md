# Role Routing Implementation Plan (WS4 §4, routing core)

> **For agentic workers:** REQUIRED SUB-SKILL: subagent-driven-development / executing-plans. Checkbox steps.

**Goal:** Seats declare `roles` + `exclusive`; `@role` mentions soft-prefer a matching seat (Phase 1); `exclusive` seats are filtered from non-matching tasks (Phase 2), with routing gaps reported not stalled.

**Architecture:** Config fields on `Seat`/`SeatOverride`/`QuarkCard` (lattice) → role resolution in `router.rs` → exclusivity filter at the engine's dispatch (gluon).

**Tech Stack:** Rust (hadron-lattice, hadron-gluon). cargo test.

## Global Constraints
- Baseline gate before/after: `cargo test --workspace --features gui` (full).
- INERT: cargo test/check only, never run binaries; tempdirs; don't touch live ~/.hadron.
- Back-compat by construction: an existing `team.json`/`QuarkCard` with no roles resolves identically (empty roles, `exclusive: false`).
- Reuse: role matching MUST ride the existing `match_longest_mention` (char-boundary safe — guards the '’'-at-byte-12 panic class); do not hand-roll a second mention matcher. Reserved aliases (`@team`/`@orchestrator`) and real ids keep precedence over role tokens.
- Phase 1 is SOFT: an unmatched `@role` falls back to existing no-addressee behaviour, never a hard error. Phase 2 routing gaps are REPORTED (field event/log), never a stall.
- `same_agent` compares the new fields (role/exclusivity change → rebuild). One focused commit per task.

---

### Task 1: `roles` + `exclusive` config fields (lattice)

**Files:** `crates/hadron-lattice/src/team.rs` (Seat, SeatOverride, resolve_team, same_agent, seat_override_delta, Seat::cli), `crates/hadron-lattice/src/quark.rs` (QuarkCard), tests inline.

**Interfaces (Produces):** `Seat.roles: Vec<String>`, `Seat.exclusive: bool`; `SeatOverride.roles: Option<Vec<String>>`, `SeatOverride.exclusive: Option<bool>`; `QuarkCard.roles: Vec<String>`, `QuarkCard.exclusive: bool`. All `#[serde(default)]` (+ `skip_serializing_if` for the empty/false/None cases so existing files don't grow keys).

- [ ] **Step 1: Failing tests** (team.rs + quark.rs): `seat_roles_and_exclusive_serde_round_trip`; `legacy_seat_has_no_roles_and_is_not_exclusive` (a `team.json` seat with neither key → `roles==[]`, `exclusive==false`); `resolve_team_applies_role_and_exclusive_overrides` (a SeatOverride setting roles + exclusive lands on the resolved seat, absent = inherit); `same_agent_rebuilds_on_role_or_exclusive_change`; `quark_card_round_trips_roles`.
- [ ] **Step 2: Run — expect FAIL** (fields undefined).
- [ ] **Step 3: Implement.** Add the fields with `#[serde(default, skip_serializing_if=...)]`. Extend `same_agent`'s destructure + comparison (roles + exclusive). Add to `Seat::cli()` ctor (`roles: vec![], exclusive: false`). In `resolve_team`, apply `ov.roles`/`ov.exclusive` when `Some` (mirror the existing model/effort override layering). In `seat_override_delta`, carry roles/exclusive when they differ from the def. Fix every full `Seat {..}` literal across `crates/` (grep) to add the two fields (spread literals inherit).
- [ ] **Step 4: Run** focused tests + full gate. Expect PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(lattice): seat roles + exclusive fields for role routing"`

---

### Task 2: Phase 1 — `@role` soft-preference routing (router.rs)

**Files:** `crates/hadron-gluon/src/router.rs` (extend `match_longest_mention`/`ResolvedMention`, `parse_addressee`, `human_mentions`), tests inline. **Consumes:** `QuarkCard.roles` (Task 1).

**Interfaces:** `ResolvedMention` gains a role resolution; `human_mentions`/`parse_addressee` resolve a `@role` token (not an id/alias) to the preferred matching card's id (roster-order tie-break).

- [ ] **Step 1: Failing tests.** Using a roster where one card carries `roles: ["architect"]`: `human_mentions("@architect do X", roster)` → `[that_id]`; `role_falls_back_softly_when_no_seat_has_it` (`@nobody` role with no match → `[]`, no panic/error); `id_precedence_over_role` (a card id `architect` + another card with role `architect` → `@architect` resolves the ID, not the role); `team_and_orchestrator_alias_beat_a_same_named_role`; `role_match_is_case_insensitive`; `role_tiebreak_is_roster_order` (two cards with the same role → the first in roster order). Reuse the existing `roster()` test helper; add `roles` to cards.
- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement.** In `match_longest_mention` (or a helper it calls), after the id + reserved-alias attempts fail for a candidate token, try it as a role: scan `roster` for enabled cards whose `roles` contains the token (case-insensitive), take the first in roster order, resolve to `ResolvedMention::Quark(card)`. Keep the id/alias precedence order (try id + `@team` + `@orchestrator` BEFORE role). Ride the existing longest-match/char-boundary machinery — no new matcher. Ensure `parse_addressee` (line-start, quark→quark) and `human_mentions` (anywhere, human) both benefit.
- [ ] **Step 4: Run** tests + full gate. Expect PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(gluon): Phase 1 soft @role-mention routing"`

---

### Task 3: Plumb `roles`/`exclusive` onto the Quark → card (the make-or-break wiring)

**Discovered during Task 1 review:** `Engine::new`/`seat` (engine.rs:395-467) build each `QuarkCard` from the live `Box<dyn Quark>` — reading `id`/`display_name`/`flavor`/`energy` off the **Quark trait**. `display_name` is carried on the quark, plumbed from the Seat at build time (`with_display_name`). `provider`/`model`/`roles` are left EMPTY (a daemon-side population step that does not exist). Routing needs card roles populated, so plumb them the SAME proven way `display_name` flows — not via the missing daemon step.

**Files:** `crates/hadron-gluon/src/quark.rs` (Quark trait), `crates/hadron-gluon/src/adapter/cli.rs` + `acp.rs` (carry + setter), `crates/hadron-gluon/src/adapter/registry.rs` (`build`/`build_seat` plumb seat roles), `crates/hadron-gluon/src/engine.rs` (read into the two card-build sites), tests inline.

- [ ] **Step 1: Failing test.** In engine.rs tests: build an engine from a quark carrying roles `["security"]` + `exclusive: true` and assert `engine.roster`'s card for that id has `roles == ["security"]` and `exclusive == true` (today it's empty). Use a test Quark (there's likely a fake in engine tests) — add role support to it. Name it `roster_card_carries_the_quarks_roles`.
- [ ] **Step 2: Run — expect FAIL** (card roles empty).
- [ ] **Step 3: Implement.** Add to the `Quark` trait: `fn roles(&self) -> Vec<String> { Vec::new() }` and `fn exclusive(&self) -> bool { false }` (default impls so nothing else breaks). In `CliQuark`/`AcpQuark`, add `roles: Vec<String>` + `exclusive: bool` fields + a `with_roles(roles, exclusive)` builder, and return them from the trait methods. In `registry::build` (the `QuarkSpec`→quark path) pass `spec.roles`/`spec.exclusive`; add `roles`/`exclusive` to `QuarkSpec` and set them in `build_seat` from `seat.roles`/`seat.exclusive`. In `Engine::new` and `Engine::seat`, replace `roles: Vec::new(), exclusive: false` with `q.roles()`/`q.exclusive()` (and `quark.roles()`/`.exclusive()`). Update the now-stale comments.
- [ ] **Step 4: Run** test + full gate. Expect PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(gluon): carry seat roles/exclusive onto the quark and its roster card"`

---

### Task 4: Phase 2 — exclusivity filter + routing-gap report (engine.rs)

**Files:** `crates/hadron-gluon/src/engine.rs` (the dispatch/roster-eligibility filter), tests inline. **Consumes:** `QuarkCard.{roles,exclusive}` (Task 1), role resolution (Task 2).

- [ ] **Step 1: Study** the dispatch eligibility path — grep in `engine.rs` for where a quark is chosen/permitted for a task (the `is_enabled`/`EnergyState::Depleted` filtering used when building `peers`/picking a target, e.g. near the `skills::select`/`next_pending` dispatch and the `peers` collection ~engine.rs:842-849). Identify the single place a candidate set is formed.
- [ ] **Step 2: Failing tests.** `exclusive_seat_excluded_from_non_matching_task` (a card `roles:["security"], exclusive:true` is NOT eligible for a task that doesn't name `@security`/its id); `exclusive_seat_eligible_for_matching_role_task` (same card IS eligible when the task names `@security` or `@its-id`); `non_exclusive_role_seat_always_eligible` (roles but `exclusive:false` → eligible regardless); `routing_gap_is_reported_not_stalled` (a task requiring a role whose only seats are exclusive+disabled → an observable diagnostic/event, not a hang or silent drop).
- [ ] **Step 3: Run — expect FAIL.**
- [ ] **Step 4: Implement.** In the eligibility filter, exclude any `exclusive` card unless the task addresses one of its roles (a `@role` resolving to it — reuse Task 2) or its `@id` explicitly. When a task requires a role but no eligible seat exists, emit the existing "no eligible quark" diagnostic (find it) or add an explicit field-event/log naming the missing role — never stall. Keep the change minimal and at the single eligibility seam.
- [ ] **Step 5: Run** tests + full gate. Expect PASS.
- [ ] **Step 6: Commit** — `git commit -m "feat(gluon): Phase 2 exclusive-seat filtering with routing-gap reporting"`

---

## Self-Review
- Spec §2.1 config → Task 1. §2.2 router Phase 1 → Task 2. §2.3 Phase 2 → Task 3. §3 testing → each task's tests. §4 security (restriction only, no new surface) → holds. ✓
- Placeholder scan: Task 3 Step 1 requires the implementer to LOCATE the eligibility seam by grep (a read, grounded by the ~842-849 pointer) — not a placeholder. No TBD. ✓
- Type consistency: `roles: Vec<String>`, `exclusive: bool`, `SeatOverride` `Option` variants, `QuarkCard.roles/exclusive` used consistently across tasks. ✓
