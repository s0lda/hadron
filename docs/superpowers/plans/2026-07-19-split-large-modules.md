# Split Large Modules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split 13 oversized `.rs` files so each module has one responsibility, without changing any behavior.

**Architecture:** Pure verbatim moves. Large `impl` blocks are cut into region files that re-open the block as `impl super::Type { … }` (descendant modules see the parent type's private fields). Test modules move to sibling `tests.rs`. Files with `include_str!` keep their `.rs` root and gain a sibling `foo/` dir; other files convert `foo.rs` → `foo/mod.rs`.

**Tech Stack:** Rust (edition 2021), cargo workspace, gpui.

## Global Constraints

- **Verification gate:** `cargo test --workspace --features gui` must report **577 passed; 0 failed; 8 ignored** after every task. This exact count is the invariant. A per-crate run (`cargo test -p <crate> --features gui`) is fine for a fast inner loop, but the full gate must pass before each commit.
- **No logic changes.** Move code verbatim. The only edits allowed beyond relocation are: (a) `mod` declarations, (b) `use` imports each new file needs, (c) visibility bumps (`pub(super)` / `pub(crate)`) the compiler demands, (d) removing imports left unused after a move.
- **No "while I'm here" edits.** No renames, no reformatting, no refactoring of moved bodies.
- **`include_str!` landmines:** `hadron-gluon/src/engine.rs` and `hadron-gluon/src/skills.rs` contain `include_str!("../invariants/…")`. Those macro-hosting functions/consts MUST stay in the original `.rs` file (paths resolve relative to the file). Verified: these are the only two files with `include_str!`/`include_bytes!`/`#[path]`.
- **Branch:** all work on `refactor/split-large-modules`. One commit per task.
- **No daemon:** confirm `pgrep -af hadron-gluon` is empty before starting (a running daemon commits your tree).

---

## The Split Procedure (applies to every task)

Each task moves items out of one big file. The mechanical loop is identical; per-task sections below only specify **which items go to which new file** and **the exact `mod` declarations**. Follow this loop for each task:

1. **Create the new files** listed in the task. For a `mod.rs`-style split (chamber `app/`), `git mv foo.rs foo/mod.rs` first, then create siblings. For a `.rs`-root split (gluon/lattice/gatekeeper flat files), leave `foo.rs` in place and create the `foo/` dir + sibling files.
2. **Add `mod` declarations** (exact lines given per task) to the root file (`mod.rs` or the kept `foo.rs`).
3. **Cut each item verbatim** from the root file and paste it into its target file. Wrap moved methods in `impl super::Type { … }` (one `impl` block per file is enough). Move `struct`/`enum`/free-`fn` items as-is.
4. **Add imports** to each new file: copy the `use` block from the original file header as a starting point (`use super::*;` where the parent re-exports, plus `use super::super::…` / crate paths as needed). Do not hand-optimize yet.
5. **Build:** `cargo build -p <crate> --features gui`. Fix errors mechanically:
   - `E0603`/private: bump the item's visibility to `pub(super)` (same-parent) or `pub(crate)` (cross-module).
   - unresolved import: add the missing `use`.
6. **Remove unused imports** the compiler now warns about (`cargo build` warnings), in the moved files only (rule 10).
7. **Verify:** run the per-crate tests, then the full gate. Confirm the pass count is unchanged.
8. **Commit** with the message given in the task.

**Verbatim-move check:** `git show <commit> --stat` should show lines roughly conserved (moved out of the root ≈ moved into siblings). A net logic change is a bug.

---

## Phase A — chamber (`app/` uses `mod.rs` + siblings)

### Task 1: Split `app/render.rs` (2891 → `render/`)

**Files:**
- `git mv crates/hadron-chamber/src/app/render.rs crates/hadron-chamber/src/app/render/mod.rs`
- Create: `render/{titlebar,roster,chat,terminal,stats,overlays}.rs`

**`mod.rs` keeps:** `impl Render for Chamber { fn render … }` (orig 8–137), `fn body` (orig 606–680), and the declarations:
```rust
mod titlebar;
mod roster;
mod chat;
mod terminal;
mod stats;
mod overlays;
```

**Item → file map** (all are `pub(super) fn` methods on `Chamber`; wrap each file's set in `impl super::Chamber { … }`):
- `titlebar.rs`: `titlebar` (552), `rail_strip` (681), `settings_button` (774)
- `roster.rs`: `roster_pane` (792)
- `chat.rs`: `chat_pane` (1012), `chat_view` (1199), `log_view` (1267), `markdown_body` (2698), `chat_message_row` (2728), `message_row` (2796)
- `terminal.rs`: `terminal_pane` (1583) — includes its nested `fn render_node` (1781); move the whole method intact
- `stats.rs`: `info_panel_overlay` (139), `timeline_view` (1348), `stats_window_tabs` (1407), `stats_view` (1437)
- `overlays.rs`: `completion_card_overlay` (497), `permission_toast` (2348), `about_overlay` (2394), `mode_select` (2504), `app_menu_overlay` (2600) — includes its nested `fn item` (2601)

- [ ] **Step 1:** Run the Split Procedure steps 1–6.
- [ ] **Step 2:** Verify — `cargo test -p hadron-chamber --features gui` (93 passed), then `cargo test --workspace --features gui` (577 passed).
- [ ] **Step 3:** Commit — `git commit -am "refactor(chamber): split render.rs into per-region render/ modules"`

> `render.rs` has no test module — its gate is compile + the 93 chamber tests staying green.

### Task 2: Split `app/settings.rs` (1883 → `settings/`)

**Files:**
- `git mv crates/hadron-chamber/src/app/settings.rs crates/hadron-chamber/src/app/settings/mod.rs`
- Create: `settings/{secrets,identity,acp_probe,overlay,providers,tests}.rs`

**`mod.rs` keeps:** the open/close/load/commit state methods — `open_settings` (10), `close_settings` (18), `settings_identity_mut` (27), `settings_color` (36), `load_settings_inputs` (47), `commit_settings_inputs` (128), `select_settings_target` (487), `reset_settings_target` (669) — plus declarations:
```rust
mod secrets;
mod identity;
mod acp_probe;
mod overlay;
mod providers;
#[cfg(test)]
mod tests;
```

**Item → file map:**
- `secrets.rs`: `set_settings_secret` (265), `clear_settings_secret` (306), `secret_field` (341); free items `declare_secret_var` (1743), `undeclare_secret_var` (1754), `enum SecretStatus` (1770), `secret_status` (1776)
- `identity.rs`: `set_settings_color` (648), `clear_settings_image` (658), `pick_avatar_image` (445), `session_select` (391)
- `acp_probe.rs`: `is_acp_quark` (507), `start_acp_model_probe` (519), `acp_model_select` (568)
- `overlay.rs`: `settings_overlay` (682), `settings_nav_row` (961)
- `providers.rs`: `providers_view` (1017) — the ~700-line panel, isolated on its own
- `tests.rs`: the `mod tests` block (orig 1785–end)

- [ ] **Step 1:** Run the Split Procedure steps 1–6. Note the free `fn`s at 1743–end move to `secrets.rs` and are already `pub(super)`.
- [ ] **Step 2:** Verify — `cargo test -p hadron-chamber --features gui`, then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(chamber): split settings.rs into settings/ modules + sibling tests"`

### Task 3: Split `app/mod.rs` (1356 → extract 3 modules)

**Files:**
- Modify: `crates/hadron-chamber/src/app/mod.rs`
- Create: `app/{input,terminal,reload}.rs`

**`mod.rs` keeps:** the `Chamber` struct (142), `enum ContextMenuAction` (298), `CompletionCard` struct (133), all `const`s, `fn term_dims` (122), `fn new` (315), `fn chat_at_bottom` (857), `fn resolve_identity` (1043), `pub fn run` (1121), the existing `mod tests` block (small, 49 lines — leave inline), and the existing + new module declarations. Add:
```rust
mod input;
mod terminal;
mod reload;
```

**Item → file map** (methods on `Chamber` → `impl super::Chamber`):
- `terminal.rs`: `pump_terminal` (571), `on_terminal_key` (624)
- `reload.rs`: `reproject` (562), `reload_if_changed` (725)
- `input.rs`: `on_input_submit` (867), `recompute_completion` (973), `move_completion_selection` (999), `accept_completion` (1015); free `fn split_leading_commands` (1086)

- [ ] **Step 1:** Run the Split Procedure steps 1–6. `split_leading_commands` is a private free fn → bump to `pub(super)` since `input.rs` and any caller in `mod.rs`/`on_input_submit` now cross the boundary.
- [ ] **Step 2:** Verify — `cargo test -p hadron-chamber --features gui`, then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(chamber): extract input/terminal/reload from app/mod.rs"`

### Task 4: Extract tests from `model.rs` + split stats (1269 → `model/`)

**Files:**
- `git mv crates/hadron-chamber/src/model.rs crates/hadron-chamber/src/model/mod.rs`
- Create: `model/{stats,tests}.rs`

**`mod.rs` keeps:** all type defs and free fns — `format_clock` (15), `date_divider_label` (24), `MessageRow` (37), `resolve_fresh` (47), `message_fresh` (69), `RosterRow` (84), `post_clear_reboots` (121), `TurnSpend` (133), `QuarkStats` (140), `StatsWindow` + impl (156), `SessionStats` (204), `SpendPoint` (217), `SpendTimeline` (231), `ChamberView` struct (239), `actor_str` (427), `note` (435), `render_row` (441), `project` (495), `load_archived_messages` (508), `project_with_team` (535). Add:
```rust
mod stats;
#[cfg(test)]
mod tests;
```

**Item → file map:**
- `stats.rs`: the `impl ChamberView` stats methods — `session_stats` (256), `stats_for` (267), `windowed_messages` (281), `spend_timeline` (307), `fold_stats` (355) → `impl super::ChamberView`
- `tests.rs`: the `mod tests` block (orig ~665–end)

- [ ] **Step 1:** Run Split Procedure 1–6. `windowed_messages`/`fold_stats` are private methods used only within the stats set — they move together, no visibility change needed.
- [ ] **Step 2:** Verify — `cargo test -p hadron-chamber --features gui`, then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(chamber): split model.rs stats + sibling tests"`

---

## Phase B — gluon (flat files → keep `foo.rs` + `foo/` dir)

### Task 5: Split `engine.rs` (5674 → keep `engine.rs` + `engine/`)

**Files:**
- Keep: `crates/hadron-gluon/src/engine.rs` (hosts `include_str!` — must not move)
- Create dir + files: `engine/{memory,routing,turn,merge,reboot,run,tests}.rs`

**`engine.rs` keeps:** `const FIELD_POLL` (29), `TURN_DEADLINE` (49), `struct Driver` (56), `struct TurnTree` (68), `workspace_root_of` (83), `const STANDARD_MODEL = include_str!` (104 — **must stay**), `struct Engine` (334), `env_no_human_mode` (422), and the `impl Engine` builder/accessor methods (new, with_*, seat, unseat, set_*, is_*, rename, seated_*, append, field_path, loaded_personas — orig 429–734). Add declarations:
```rust
mod memory;
mod routing;
mod turn;
mod merge;
mod reboot;
mod run;
#[cfg(test)]
mod tests;
```

**Item → file map** (methods → `impl super::Engine`; free fns get `pub(super)` when called cross-module):
- `memory.rs`: `memory_index_path` (110), `memory_notes_dir` (118), `memory_dir` (122), `const MEMORY_INDEX_BUDGET` (132), `read_memory_index` (146), `global_invariants_dir` (193), `read_invariant_dir` (206), `build_invariants` (243), `const FIELD_WINDOW_BUDGET_BYTES` (296), `event_cost` (300), `bounded_window` (315)
- `routing.rs`: `is_orchestrator` (515), `orchestrator_id` (521), `commands_for` (530), `human_addressees` (746), `human_message_targets` (779), `has_answered` (827), `pending_targets` (842), `exclusive_task_names_target` (870), `driver_for` (896), `projection_for` (972)
- `turn.rs`: `reroute_blocked` (1124), `finish_turn` (1136), `const NO_HUMAN_ADJUDICATION_MARKER` (1384), `orchestrator_adjudication_message` (1400)
- `merge.rs`: `merge_gate` (1481)
- `reboot.rs`: `service_reboots` (1638)
- `run.rs`: `run_until_quiesce` (1721)
- `tests.rs`: the entire `mod tests` block (orig 2107–5674) — the 3,569-line win

- [ ] **Step 1:** Run Split Procedure 1–6. Expect several `pub(super)` bumps on the free fns in `memory.rs` (`workspace_root_of` is already `pub(crate)`; `build_invariants`, `bounded_window`, `read_memory_index`, etc. called from `routing`/`run` need `pub(super)`). `tests.rs` uses `use super::*;` and `use crate::…` — copy the test module's existing `use` lines.
- [ ] **Step 2:** Verify — `cargo test -p hadron-gluon --features gui` (matches baseline for that crate: 308 passed / 8 ignored + 6 in the bin), then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(gluon): split engine.rs into engine/ submodules + sibling tests"`

### Task 6: Split `adapter/acp.rs` (1697 → `adapter/acp/`)

**Files:**
- `git mv crates/hadron-gluon/src/adapter/acp.rs crates/hadron-gluon/src/adapter/acp/mod.rs` (no `include_str!` in this file — safe to convert)
- Create: `acp/{session,spend,model,tests}.rs`

**`mod.rs` keeps:** `struct AcpQuark` (429), `impl AcpQuark` builders (463–541: new, watching, with_*, running_model), `impl Quark for AcpQuark` (854), and declarations:
```rust
mod session;
mod spend;
mod model;
#[cfg(test)]
mod tests;
```

**Item → file map:**
- `spend.rs`: `struct SpendWatermark` (123), `turn_spend` (149)
- `model.rs`: `permission_choice` (194), `struct AcpModel` (204), `struct ModelSelector` (215), `model_selector` (230), `effort_selector` (234), `mode_selector` (238), `config_selector` (242), `resolve_model` (289), `probe` (326), `probe_selector` (340), `probe_session` (349)
- `session.rs`: `acp_stdio_descriptor` (70), `struct TurnRequest` (88), `struct TurnReply` (94), `struct AcpSession` (107), `struct LiveFeed` + impl (399–429), `AcpQuark::boot` (542) and `AcpQuark::run_turn` (905) → `impl super::AcpQuark`
- `tests.rs`: the `mod tests` block (orig ~990–end)

- [ ] **Step 1:** Run Split Procedure 1–6. `boot`/`run_turn` are private methods used by the `Quark::excite` impl in `mod.rs` → `pub(super)`.
- [ ] **Step 2:** Verify — `cargo test -p hadron-gluon --features gui`, then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(gluon): split adapter/acp.rs into acp/ modules + sibling tests"`

### Task 7: Split `skills.rs` (1141 → keep `skills.rs` + `skills/`)

**Files:**
- Keep: `crates/hadron-gluon/src/skills.rs` (hosts `include_str!` in `builtins()` — must not move)
- Create: `skills/{parse,select,tests}.rs`

**`skills.rs` keeps:** `struct Skill` (41), `struct ResolvedSkill` (192), `builtins` (214 — **contains include_str!, must stay**), `is_tool_allowed` (245), `load_skills` (262). Add declarations:
```rust
mod parse;
mod select;
#[cfg(test)]
mod tests;
```

**Item → file map:**
- `parse.rs`: `upsert` (277), `load_dir` (289), `parse_skill_file` (340), `split_front_matter` (373), `front_matter_value` (383), `parse_list_value` (398)
- `select.rs`: `struct Match` (421), `select` (435), `struct Handoff` (465), `plan_ref` (479), `plan_author` (490), `description` (497), `index` (506), `render` (530)
- `tests.rs`: the `mod tests` block (orig ~613–end). Note: `tests.rs` references `include_str!("../testdata/builtins_index_snapshot.txt")` at orig 935 — that path is relative to the file, and `skills/tests.rs` is one level deeper, so **change it to `../../testdata/builtins_index_snapshot.txt`**.

- [ ] **Step 1:** Run Split Procedure 1–6. `split_front_matter`/`front_matter_value` are already `pub(crate)`; `load_dir`/`upsert`/`parse_skill_file`/`parse_list_value` are private → bump to `pub(super)` (called from `load_skills`/`builtins` in the root). Fix the test's `include_str!` depth as noted.
- [ ] **Step 2:** Verify — `cargo test -p hadron-gluon --features gui` (watch the `builtins_index_snapshot` test specifically), then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(gluon): split skills.rs parse/select + sibling tests"`

### Task 8: Split `adapter/registry.rs` (1080 → `adapter/registry/`)

**Files:**
- `git mv crates/hadron-gluon/src/adapter/registry.rs crates/hadron-gluon/src/adapter/registry/mod.rs`
- Create: `registry/{presets,tests}.rs`

**`mod.rs` keeps:** `enum QuarkKind` (20), `struct AcpTarget` (33), `impl AcpTarget` (345), `impl QuarkKind` (386), `struct QuarkSpec` (480), `validate_quark_id` (512), `build` (568), `build_seat` (601), `build_seat_watched` (623). Add:
```rust
mod presets;
#[cfg(test)]
mod tests;
```

**Item → file map:**
- `presets.rs`: `struct AcpAgentSpec` (49) and its large preset/data block (orig 49–345, up to where `impl AcpTarget` begins). Move the struct + all associated preset constructors/data intact.
- `tests.rs`: the `mod tests` block (orig ~642–end)

- [ ] **Step 1:** Run Split Procedure 1–6. If `AcpTarget::for_vendor`/`for_seat` consume `AcpAgentSpec` presets, expose the needed preset accessor as `pub(super)`.
- [ ] **Step 2:** Verify — `cargo test -p hadron-gluon --features gui`, then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(gluon): split adapter/registry.rs presets + sibling tests"`

### Task 9: Extract tests from `router.rs` (966 → `router/`)

**Files:**
- `git mv crates/hadron-gluon/src/router.rs crates/hadron-gluon/src/router/mod.rs`
- Create: `router/tests.rs`

**`mod.rs` keeps** all production code (orig 11–363, code is only 363 lines — no production split needed). Add:
```rust
#[cfg(test)]
mod tests;
```
- `tests.rs`: the `mod tests` block (orig ~363–end)

- [ ] **Step 1:** Run Split Procedure 1–6 (only steps for the tests move + `use super::*;`).
- [ ] **Step 2:** Verify — `cargo test -p hadron-gluon --features gui`, then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(gluon): move router.rs tests to sibling module"`

### Task 10: Extract tests from `adapter/prompt.rs` (766 → `adapter/prompt/`)

**Files:**
- `git mv crates/hadron-gluon/src/adapter/prompt.rs crates/hadron-gluon/src/adapter/prompt/mod.rs`
- Create: `prompt/tests.rs`

**`mod.rs` keeps** all production code (orig ~1–366). Add `#[cfg(test)] mod tests;`. Move the `mod tests` block (orig ~366–end) to `tests.rs`.

- [ ] **Step 1:** Run Split Procedure 1–6 (tests move only).
- [ ] **Step 2:** Verify — `cargo test -p hadron-gluon --features gui`, then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(gluon): move adapter/prompt.rs tests to sibling module"`

---

## Phase C — lattice & gatekeeper

### Task 11: Split `team.rs` (2004 → `team/`)

**Files:**
- `git mv crates/hadron-lattice/src/team.rs crates/hadron-lattice/src/team/mod.rs` (no `include_str!` — safe)
- Create: `team/{transport,seat,io,migrate,tests}.rs`

**`mod.rs` keeps:** `struct Team` (569), `impl Team` (581), `resolve_team` (617), and declarations:
```rust
mod transport;
mod seat;
mod io;
mod migrate;
#[cfg(test)]
mod tests;
```
Re-export moved public types so external paths (`hadron_lattice::Transport`, `Seat`, etc.) stay stable:
```rust
pub use transport::{Transport, AcpCommand, PromptChannel, ResumeMode, TimeoutArg, PostureMap, CliSpec};
pub use seat::{Seat, SeatCommands, SeatOverride};
pub use io::{parse_team, load_team, save_team, team_config_path, team_for_field, user_hadron_dir};
pub use migrate::{migrate_to_catalogue, seat_override_delta, orphan_overrides, legacy_id_renames, rename_legacy_ids, id_follows_convention};
```

**Item → file map:**
- `transport.rs`: `enum Transport` + impl (33–66), `struct AcpCommand` (80), `enum PromptChannel` (98), `enum ResumeMode` (114), `struct TimeoutArg` (127), `struct PostureMap` + impl (136–171), `struct CliSpec` + impl (171–262: code, agy, preset, generic)
- `seat.rs`: `struct SeatCommands` + impl (262–284), `struct Seat` (284), `enabled_by_default` (362), `is_false` (370), `impl Seat` (374: same_agent, cli, normalize_vendor, resolve_env), `struct SeatOverride` (479), `present_option` (527), `impl SeatOverride` (535)
- `io.rs`: `home_dir` (781), `user_hadron_dir` (796), `team_config_path` (803), `team_for_field` (813), `parse_team` (842), `load_team` (855), `save_team` (872)
- `migrate.rs`: `migrate_to_catalogue` (684), `seat_override_delta` (715), `orphan_overrides` (739), `legacy_id_renames` (751), `rename_legacy_ids` (758), `id_follows_convention` (775)
- `tests.rs`: the `mod tests` block (orig ~881–end, 1123 lines)

- [ ] **Step 1:** Run Split Procedure 1–6. `is_false` (`pub(crate)`) and `enabled_by_default`/`home_dir`/`present_option` (private, serde/helper) need `pub(super)` where referenced across the split (e.g. `is_false` in `#[serde(skip_serializing_if)]` attributes on `Seat`/`Team` fields — both must reach it). Confirm the `pub use` re-exports resolve every external reference (`cargo build --workspace` surfaces any missed path).
- [ ] **Step 2:** Verify — `cargo test -p hadron-lattice --features gui` (40 passed) and full gate (577). External consumers (chamber/gluon) must still compile against the re-exports.
- [ ] **Step 3:** Commit — `git commit -am "refactor(lattice): split team.rs into team/ modules + sibling tests"`

### Task 12: Extract tests from `event.rs` (860 → `event/`)

**Files:**
- `git mv crates/hadron-lattice/src/event.rs crates/hadron-lattice/src/event/mod.rs`
- Create: `event/tests.rs`

**`mod.rs` keeps** all production code (orig ~1–441). Add `#[cfg(test)] mod tests;`. Move the `mod tests` block (orig ~441–end) to `tests.rs`.

- [ ] **Step 1:** Run Split Procedure 1–6 (tests move only). `event/mod.rs` is a module root — ensure any `pub use` currently in `lib.rs` (`pub mod event;`) is unaffected (module path is unchanged).
- [ ] **Step 2:** Verify — `cargo test -p hadron-lattice --features gui`, then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(lattice): move event.rs tests to sibling module"`

### Task 13: Extract tests from `matrix.rs` (728 → `matrix/`)

**Files:**
- `git mv crates/hadron-gatekeeper/src/matrix.rs crates/hadron-gatekeeper/src/matrix/mod.rs`
- Create: `matrix/tests.rs`

**`mod.rs` keeps** all production code (orig ~1–251). Add `#[cfg(test)] mod tests;`. Move the `mod tests` block (orig ~251–end) to `tests.rs`.

- [ ] **Step 1:** Run Split Procedure 1–6 (tests move only).
- [ ] **Step 2:** Verify — `cargo test -p hadron-gatekeeper --features gui` (121 passed), then full gate (577).
- [ ] **Step 3:** Commit — `git commit -am "refactor(gatekeeper): move matrix.rs tests to sibling module"`

---

## Final verification (after Task 13)

- [ ] Full gate green: `cargo test --workspace --features gui` → **577 passed; 0 failed; 8 ignored**.
- [ ] No file over ~900 lines remains among the 13 targets: `find crates/hadron-* -name '*.rs' -not -path '*/target/*' | xargs wc -l | sort -rn | head -20`.
- [ ] `cargo build --workspace --features gui` clean (no new warnings introduced by leftover unused imports).
- [ ] Branch `refactor/split-large-modules` has 13 refactor commits + the design doc commit.
- [ ] Report the WSL2 caveat: `render.rs`/`settings.rs` GUI paths are verified by compile + the 93 chamber unit tests, not by runtime rendering.

## Self-Review notes

- **Spec coverage:** all 13 files from the spec's two scope tables have a task (Tasks 1–13). ✓
- **`include_str!` guard:** engine.rs (Task 5) and skills.rs (Task 7) keep their `.rs` root; skills `tests.rs` fixes the snapshot path depth. ✓
- **Public API stability:** team.rs (Task 11) is the only file exporting widely-used public types across crates → explicit `pub use` re-exports added. Other `mod.rs` conversions keep module paths identical, so `pub` items stay reachable. ✓
- **Line numbers** are from the pre-refactor snapshot and are anchors, not literals — the executor cuts by item identity, not by hard offsets (earlier moves shift later line numbers within a file).
