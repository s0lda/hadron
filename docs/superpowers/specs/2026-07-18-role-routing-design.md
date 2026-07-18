# Role routing (WS4 §4, routing core) — design

**Status:** design (autonomous; review when awake)
**Date:** 2026-07-18
**Source:** §4 of `docs/superpowers/specs/2026-07-17-permissions-and-extensibility-design.md`
**Branch:** `feat/role-routing` (stacked on `feat/ui-fixes`)

## 1. Scope
Implement the **routing core** of spec §4: seats declare `roles` + `exclusive`; a `@role` mention (e.g. `@architect`, `@security`) soft-prefers a seat carrying that role (Phase 1); an `exclusive` seat is filtered out of any task that doesn't match its role (Phase 2). **Deferred:** persona files (`.hadron/agents/*.md` with `preferred_role`) — they share the "load .md from global+local dirs" machinery with skills §2 and are cleaner to build alongside it. Routing works from `team.json` seat roles alone; personas are an additive layer later.

## 2. Design

### 2.1 Config — `hadron-lattice`
- **`Seat`** (`team.rs`): add `#[serde(default)] pub roles: Vec<String>` and `#[serde(default)] pub exclusive: bool`. Defaults (empty / false) keep every existing `team.json` byte-identical. `same_agent` destructures + compares both (a role/exclusivity change is a different agent → rebuild). `Seat::cli()` ctor sets them to defaults.
- **`SeatOverride`** (`team.rs`): a per-repo override MAY set `roles`/`exclusive` (both `Option<...>`: absent = inherit). `resolve_team` applies them like the other definition deltas. `seat_override_delta` carries them when they differ from the catalogue default.
- **`QuarkCard`** (`quark.rs`): add `#[serde(default)] pub roles: Vec<String>` (the router reads roles off the card; the card is the router's view of a seat). `exclusive` is a *dispatch-filter* property — carry it on the card too as `#[serde(default)] pub exclusive: bool` so the engine's dispatch filter can see it without re-reading team.json.

### 2.2 Router — `hadron-gluon/src/router.rs`
- Extend `ResolvedMention` with a `Role(&'a str)` case, OR resolve a role mention to the set of matching cards. Because a role can match *several* seats, the cleanest is a resolver that, given a role token, returns the preferred card:
  - `role_addressees(body, roster) -> Vec<QuarkId>` / integrate into `human_mentions` + `parse_addressee`: when a mention token isn't a quark id or a reserved alias, try it as a **role** — match against every enabled card whose `roles` contains it (case-insensitive), and prefer one (deterministic: first by roster order, or the least-busy — start with roster order for determinism; note as a tuning point).
  - Reserved-alias precedence unchanged: `@team`/`@orchestrator` still resolve first (a role named `team`/`orchestrator` is disallowed the same way ids are).
  - **Phase 1 is soft:** if a `@role` matches no enabled seat, fall back to the existing behaviour (no addressee → hand back to orchestrator / general worker), never a hard error mid-routing.
- **Longest-match safety:** role matching rides the same `match_longest_mention` char-boundary discipline (the '’'-at-byte-12 panic class the existing tests guard) — reuse it, don't hand-roll a second matcher.

### 2.3 Phase 2 — exclusivity filter (`hadron-gluon/src/engine.rs` dispatch)
- Where the engine picks/permits a quark for a task (the roster filter used at dispatch — the same place `is_enabled`/energy filtering happens), add: a card with `exclusive == true` is **eligible only** for a task that names one of its roles (via a `@role` mention resolving to it, or an explicit `@id`). For any other task it is filtered out — it never gets a turn it isn't scoped for.
- **Routing failure is reported, not stalled:** if a task requires a role and the only seats for it are `exclusive` and none match/are enabled, the engine surfaces the routing gap (a field event / log back to the orchestrator or human) rather than silently dropping the task or hanging. Reuse the existing "no eligible quark" path if one exists; otherwise add an explicit diagnostic.

## 3. Testing
- **Config:** serde round-trip of `roles`/`exclusive`; `resolve_team` applies role/exclusive overrides; back-compat (a `team.json` with no roles → empty roles, false exclusive, resolves identically); `same_agent` rebuilds on role/exclusive change.
- **Router Phase 1:** `@architect` with one seat carrying role `architect` → routes to it; with none → soft fallback (no addressee); case-insensitive; a role token that collides with a real id resolves as the id first (id precedence); `@team`/`@orchestrator` still win over a same-named role.
- **Phase 2:** an `exclusive` seat is excluded from a non-matching task and included for a matching-role task; a task needing a role with only-disabled exclusive seats reports the gap (doesn't stall).
- Full gate `cargo test --workspace --features gui` green.

## 4. Security note (Rule 7)
Routing decides which *local* quark services a task — no auth, no execution, no external input beyond the human's own message (already the routing input). An `exclusive` seat is a *restriction* (fewer tasks reach it), never a new capability. No new attack surface. (The dangerous parts of the permissions spec — No-Human-Mode gating §2 and script-exec §3 — are NOT in this sub-project.)

## 5. Autonomous judgment calls (flag for review)
- **Persona files deferred** to the skills/agents `.md`-loading sub-project (shared machinery). Routing works from seat `roles` now.
- **Preferred-seat tie-break = roster order** (deterministic) when a `@role` matches multiple seats. Alternative (least-busy / round-robin) is a tuning point noted for later — starting deterministic so tests are stable.
- **Role token namespace:** a role is matched only when the mention isn't already a quark id or reserved alias (id/alias precedence), mirroring how the reserved aliases already work.
