# Custom Skills Loading Implementation Plan (permissions §3 Part A — skills)

> **For agentic workers:** REQUIRED SUB-SKILL: subagent-driven-development. Checkbox steps.

**Goal:** Load custom `.md` skills from `~/.hadron/skills/` (global) + `<workspace>/.hadron/skills/` (repo) at runtime, merge with the compile-time built-ins (repo > global > built-in, by id/name), and use the merged set in `select`/`index`/`render`. Fully testable headless (tempdirs). **Personas (`.hadron/agents/`) reuse this loader in a follow-up — NOT in this plan.**

**Architecture:** Introduce an owned `ResolvedSkill` (String-backed) so runtime-loaded and compile-time skills coexist; a pure `load_skills(dirs)`; refactor `select`/`index`/`render` to operate on `&[ResolvedSkill]`.

**Tech Stack:** Rust (hadron-gluon). cargo test.

## Global Constraints
- Baseline gate: `cargo test --workspace --features gui` (full).
- INERT session: cargo test/check only, never binaries; tempdirs; never touch real ~/.hadron.
- Back-compat: with NO custom dirs, the merged set == today's built-ins, and `select`/`index`/`render` behave byte-for-byte as now (WS4§5's trim intact: index + active body).
- Reuse the existing front-matter parsers (`description:`, and the `---`-block split) — don't hand-roll a second YAML splitter.
- Override by id/name: a repo skill named `writing-plans` REPLACES the built-in of that name.
- One focused commit per task.

---

### Task 1: `ResolvedSkill` + `load_skills` + refactor select/index/render (skills.rs)

**Files:** `crates/hadron-gluon/src/skills.rs`, tests inline (tempdir fixtures).

**Interfaces (Produces):**
- `pub struct ResolvedSkill { pub id: String, pub triggers: Vec<String>, pub body: String, pub description: Option<String>, pub tools: Vec<String> }` (owned).
- `pub fn builtins() -> Vec<ResolvedSkill>` — the compile-time `SKILLS` mapped into owned `ResolvedSkill`s (parse each body's front-matter `description`; `tools` empty for built-ins).
- `pub fn load_skills(global_dir: Option<&Path>, repo_dir: Option<&Path>) -> Vec<ResolvedSkill>` — start from `builtins()`, then for each `*.md` in global then repo: parse front-matter (`name` REQUIRED → id; `description`; `triggers:` as a comma or YAML-list; `tools:` list) + the body; upsert by id (a later source with the same id REPLACES the earlier — so repo > global > built-in). A file with no `name` is skipped with a logged warning (don't guess an id from the filename silently — or DO derive id from filename; pick one and document). 
- Refactor `select(task, skills: &[ResolvedSkill]) -> Option<Match>`, `index(skills: &[ResolvedSkill]) -> String`, `render(m, self_id, handoff, include_body, skills: &[ResolvedSkill]) -> String` to take the skill slice instead of the global `SKILLS`. (`Match` must carry enough to render — likely the resolved skill's id/body; keep `Match` owned or index-based into the slice.)

- [ ] **Step 1: Failing tests** (tempdir fixtures): `load_skills_with_no_dirs_equals_builtins` (None,None → ids == today's SKILLS ids, same count); `repo_skill_overrides_builtin_by_name` (a repo `writing-plans.md` with a distinctive body replaces the built-in — `select("write a plan", loaded)` renders the repo body); `global_then_repo_precedence` (same id in global + repo → repo wins); `custom_skill_is_selectable_by_its_triggers` (a new skill with `triggers: [foo]` → `select("do foo", loaded)` finds it); `front_matter_tools_parsed`; `index_lists_loaded_skills`. And an ADDITIVE proof: `select`/`index`/`render` over `builtins()` produce the same output the old static-SKILLS versions did (snapshot a couple).
- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement** `ResolvedSkill`, `builtins()`, `load_skills`, the front-matter `triggers`/`tools` parsers (reuse the `---` split helper), and the `select`/`index`/`render` refactor. Keep the built-in `SKILLS` const as the source `builtins()` maps from.
- [ ] **Step 4: Fix the callers** of `select`/`index`/`render` (engine.rs — grep). For THIS task, pass `&builtins()` at the call sites so behavior is unchanged (the real dir-loading wiring is Task 2). Full gate green.
- [ ] **Step 5: Commit** — `git commit -m "feat(gluon): ResolvedSkill + load_skills (merge built-in<global<repo); select/index/render take a slice"`

---

### Task 2: Engine loads the merged set from the real dirs (engine.rs)

**Files:** `crates/hadron-gluon/src/engine.rs` (load once + thread into the projection's skill calls), tests inline.

- [ ] **Step 1: Study** where the engine calls `skills::select`/`index`/`render` (the projection builder ~engine.rs:828-866 after WS4§5) and what workspace root it has (`workspace_root`). The skill dirs: `<workspace_root>/.hadron/skills/` (repo) and `~/.hadron/skills/` (global, via `hadron_lattice::user_hadron_dir()`).
- [ ] **Step 2: Failing test.** `engine_loads_repo_skills` — build an engine with a workspace containing `.hadron/skills/custom.md` (a skill with a distinctive body + trigger), excite on a task matching that trigger, and assert the projection's `invariants` contains the custom skill's body. (Use the engine test harness + a tempdir workspace.)
- [ ] **Step 3: Implement.** Load `load_skills(user_hadron_dir()/skills, workspace_root/.hadron/skills)` — either once at construction (cache on the engine) or per-projection (simpler, re-reads each turn — acceptable, mirrors how team.json is re-read; pick + document). Pass the merged set to `select`/`index`/`render`. Keep WS4§5's `include_body=true` behavior.
- [ ] **Step 4: Full gate.** Expect PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(gluon): engine loads custom skills from ~/.hadron and repo .hadron/skills"`

---

## Self-Review
- Spec Part A (dir loading, merge by name, wire into select/index/render) → T1 (loader+merge+refactor) + T2 (engine wiring). Personas explicitly deferred. Tool-gating (§3.2) deferred (ties to the §2 gatekeeper). ✓
- Placeholder scan: the `name`-missing behavior (skip-with-warning vs filename-derive) is a decision the implementer makes + documents — not a TBD. Front-matter `triggers`/`tools` parsing reuses the existing `---` split. ✓
- Back-compat: `builtins()` + no-dirs = today; additive proof pins it. ✓
