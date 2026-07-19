# Design: Split large source files into functional modules

**Date:** 2026-07-19
**Goal:** Break up oversized `.rs` files so each file/module does one thing, is small
enough to hold in context, and is easy to navigate. Behavior-preserving — zero logic
changes.

## Baseline (the invariant)

`cargo test --workspace --features gui` → **577 passed, 0 failed, 8 ignored**.
The chamber test binary (`hadron_chamber-*`, 93 tests) is confirmed to run on WSL2.

**This exact pass count must be identical after every commit.** "Still compiles" does
not prove a test stopped being compiled in; the green count does. No gluon daemon is
running (checked `pgrep`), so editing the main checkout is safe from
`live-swarm-shares-the-checkout`.

## Convention (match each crate's existing style)

- **chamber `app/`** already uses `mod.rs` + siblings → `foo.rs` becomes `foo/mod.rs`
  plus one file per region.
- **gluon / lattice** use flat files → keep `foo.rs` as the module root and add a
  `foo/` sibling directory for submodules (`mod part;` resolves to `foo/part.rs`,
  Rust 2018+/edition 2021). This is **mandatory** for `engine.rs` and `skills.rs`:
  their `include_str!("../invariants/…")` paths resolve relative to the file, so the
  file must not move. These are the only two `include_str!` landmines (grep-confirmed).
- **Splitting `impl` blocks is free**: a descendant module sees the parent type's
  private fields, so each region file is just `impl super::Chamber { … }` /
  `impl super::Engine { … }`. Private free `fn`s that become cross-module get bumped to
  `pub(super)` / `pub(crate)`. No logic changes — pure moves (rules 2, 4, 10).
- **Tests → sibling** `#[cfg(test)] mod tests` moves to `foo/tests.rs` via `use super::*`.

## Scope — 13 files

### Production split (code > 600 lines) — 9 files

| File | code | Target modules |
|---|---|---|
| `hadron-chamber/src/app/render.rs` | 2891 | `render/{mod (Render impl + body layout), titlebar, roster, chat, terminal, stats, overlays}` |
| `hadron-gluon/src/engine.rs` | 2105 | keep `engine.rs` (struct + builders + `STANDARD_MODEL`) + `engine/{memory, routing, turn, merge, reboot, run, tests}` |
| `hadron-chamber/src/app/settings.rs` | 1783 | `settings/{mod (open/close/load/commit state), secrets, identity, acp_probe, overlay, providers, tests}` |
| `hadron-chamber/src/app/mod.rs` | 1307 | keep `Chamber` struct + consts + `run()` + submodule decls; extract `{input, terminal, reload}` |
| `hadron-gluon/src/adapter/acp.rs` | 990 | `acp/{mod (Quark impl), session, spend, model, tests}` |
| `hadron-lattice/src/team.rs` | 881 | keep core types + `resolve_team`; extract `{transport, seat, io, migrate, tests}` |
| `hadron-chamber/src/model.rs` | 665 | keep types + `project`; extract `{stats, tests}` |
| `hadron-gluon/src/adapter/registry.rs` | 642 | keep builders; extract `{presets, tests}` |
| `hadron-gluon/src/skills.rs` | 613 | keep `builtins()` (include_str) + extract `{parse, select, tests}` |

### Test-extraction only (code ≤ 600, file bloated by tests) — 4 files

| File | total → code | Action |
|---|---|---|
| `hadron-gluon/src/router.rs` | 966 → 363 | tests → `router/tests.rs` |
| `hadron-lattice/src/event.rs` | 860 → 441 | tests → `event/tests.rs` |
| `hadron-gluon/src/adapter/prompt.rs` | 766 → 366 | tests → `prompt/tests.rs` |
| `hadron-gatekeeper/src/matrix.rs` | 728 → 251 | tests → `matrix/tests.rs` |

## Delivery & verification

- **One branch** `refactor/split-large-modules`; **one commit per file**; the full test
  gate (`cargo test --workspace --features gui`, 577 passed) must be green before each
  commit. Single review at the end.
- `render.rs` has **no tests** (pure GPUI element trees). Its gate is: compiles + the 93
  chamber tests stay green. Runtime GUI is not verifiable on WSL2 (known limitation) —
  called out in the final report, not silently skipped.
- The tests-to-siblings moves are the cheap line-count win but do **not** by themselves
  satisfy "one module, one thing" — the production-code splits do. Both are in scope.

## Non-goals

- No behavior/logic changes, no API changes beyond visibility bumps required by the move.
- No "while I'm here" edits. No renaming of functions or types.
- Files with code ≤ 600 and no test bloat are left alone (they are already fine).
