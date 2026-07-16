# Tabbed info panel · time-windowed Stats · provider search — Design

**Date:** 2026-07-16
**Status:** Implemented (P1 b9e8023, stats core b9343fe, P2/P3 UI d19fc99; engine
id-set reboot fix 4d39808). Full gate green (392 tests).

## Problem

Five related UI refinements, driven by Jake:

1. **Restart placement.** The force-restart control is a roster `⟳` glyph today; move
   it into the right-click **context menu**, and keep a Restart action in the info
   panel's **Identity** tab. Drop the standalone glyph.
2. **Quark info panel** is a fixed 420px single column that grows very tall. Make it
   **wider** and split into tabs: **Identity · Config · Stats**, where Stats holds
   **Session / Week / Month / All time** sub-tabs.
3. **Chat "Session" tab → "Stats"**, itself holding **Session / Week / Month / All
   time** sub-tabs (team-wide, the panel analogue of the per-quark info Stats tab).
4. **Add-quark (Settings › Providers › add):** a **search bar** to filter the ~37
   presets, and **compact** rows — today each is a tall `p_4` card showing only a name
   and "Configure →", so the list is enormous.
5. **Charts:** use `AreaChart` (present in the fork, exact API Jake pasted) for the
   spend-over-time series, with a gradient fill.

## Decisions (from Jake)

- **Time windows:** *treat the field as the session.* `/clear` archives the field and
  starts fresh — **and must now also restart every quark** (fresh sessions). So:
  - **Session** = the current `field.jsonl` (post-/clear live session).
  - **Week / Month** = rolling `now − 7d` / `now − 30d`, over the current field **plus
    archived sessions** (`.hadron/sessions/*/field.jsonl`).
  - **All time** = every event across the current field + all archived sessions.
- **Info panel tabs:** `Identity · Config · Stats` (Stats has the 4 window sub-tabs).
  Not six flat tabs.
- **Restart:** context menu **and** the info Identity tab; roster glyph removed.

## Architecture

### Time-windowed stats (the shared core)
- A `StatsWindow` enum `{ Session, Week, Month, AllTime }` with `label()` and a
  `cutoff(now) -> Option<DateTime<Utc>>` (`None` = no lower bound).
- `ChamberView::session_stats` already folds per-quark/total stats from `messages`.
  Generalize to `stats_for(events, window)` — filter events to the window by `ts`
  before folding. `Session` uses the live `messages`; `Week/Month/AllTime` fold over a
  **merged** event stream (live field + archives).
- **Archive loader:** `load_archived_events(hadron_dir)` globs
  `sessions/*/field.jsonl`, reads each, returns the concatenation. Cached on the
  `Chamber` (`archived_events: Vec<Event>` + the sessions-dir mtime it was read at);
  refreshed lazily when a Stats view opens and the dir changed. `Session`/live never
  needs it, so the common path stays archive-free.
- One renderer `stats_body(stats, q_color, window)` builds the KV rows + charts, shared
  by the info Stats tab (per-quark) and the chat Stats tab (team total). It replaces
  the current inline stats block.

### Charts
- Spend-over-time (`spend_history`): `AreaChart` with a gradient fill
  (`linear_gradient(0., stop(chart_1.opacity(0.4),1.), stop(background.opacity(0.3),0.))`),
  `.natural()`. Replaces the `LineChart`.
- Context occupancy (used/remaining snapshot) stays a `BarChart` — it is a proportion,
  not a time series, so an area chart does not fit.

### Info panel (per-quark)
- Widen `420px → ~560px`, keep `max_h` + scroll.
- `info_tab: InfoTab { Identity, Config, Stats }` on `Chamber`, plus reuse the Stats
  window selector. A segmented `TabBar` at the top (same component the chat panel uses).
  - **Identity:** header (avatar, name, presence) · Role · State · Adoption · the
    **Restart** action (ACP only).
  - **Config:** Provider · Agent · Model · Transport · Effort · Permission chip.
  - **Stats:** the 4 window sub-tabs → `stats_body` for this quark.

### Chat Stats tab (team-wide)
- Rename `ChatTab::Session → ChatTab::Stats` (label "Stats").
- Its body: a window sub-tab bar (Session/Week/Month/All time) over `stats_body` for
  the team totals (and the existing per-quark breakdown).

### Restart → context menu
- Add a `Restart` item to the roster row's `context_menu` closure, gated to
  `Transport::Acp`, routed through a new `ContextMenuAction::RestartQuark(id)` →
  `reboot_quark`. Remove the roster `⟳` glyph; keep the info-panel Identity action.

### Add-quark search + compaction
- A `preset_filter` input (`Entity<InputState>`) above the preset list; case-insensitive
  substring match on preset name + command. Compact each row `p_4 → p_2`, drop or shrink
  the command subtitle so a row is ~2 lines tall.

### /clear also restarts quarks
- In the `"clear"` command, after archiving + truncating, append a `Kind::Reboot` for
  **each** seated quark (per-quark events; `service_reboots` already ignores unseated
  ids) so every resident agent re-boots into the fresh session.

## Phasing (committed incrementally)

1. **P1 — independent, low-risk:** restart→context-menu (+ remove glyph); add-quark
   search + row compaction; `/clear` restarts all quarks.
2. **P2 — info panel:** widen + `Identity/Config/Stats` tabs; Restart in Identity;
   `stats_body` extracted; AreaChart. (Windows over the *live* field first.)
3. **P3 — chat Stats tab + archives:** rename Session→Stats with window sub-tabs; the
   archive loader so Week/Month/All time span history; shared `stats_body`.

## Testing
- `StatsWindow::cutoff` boundaries (Session=None-bound-live, Week/Month rolling, AllTime
  no bound).
- `stats_for(events, window)` filters by ts (an event outside the window is excluded;
  one inside is counted) — a model-level unit test with hand-built timestamped events.
- Archive loader: a temp `.hadron/sessions/A/field.jsonl` + `B/…` merge into one stream;
  missing dir → empty, no error.
- `/clear` appends one Reboot per seated quark (assert count + `to` ids).
- Existing projection/color tests stay green (RosterRow unchanged).

## Out of scope (YAGNI)
- Calendar-aligned weeks/months (rolling only).
- Cross-repo / global stats. Archive compaction or retention limits.
- Restarting CLI quarks (nothing resident).
