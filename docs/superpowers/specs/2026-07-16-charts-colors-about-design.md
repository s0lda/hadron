# Spend area chart · per-quark color identity · progress bar · About opacity — Design

**Date:** 2026-07-16
**Status:** Implemented (P1 About+progress `e16c18c`, P2 colours `ead8111`, P3 spend
chart `509768b`). Full gate green (393 tests).

**Implementation note — colour storage (SSOT correction):** the design proposed a new
`SeatOverride.color` in `team.json`. During P2 a pre-existing per-quark colour system was
found — `ChamberPrefs.quarks[id].color` (`~/.hadron/chamber.json`), resolved by
`resolve_identity`, set by `set_settings_color` + swatches. Adding a second colour store
would have violated SSOT, so the lattice/model plumbing was reverted and the feature was
built on the existing system: `color_for` delegates to `resolve_identity().color`, and
the ColorPicker writes `ChamberPrefs` via `set_settings_color`. Colour is therefore
per-machine (chamber.json), not per-repo — the correct home, since that is where
`display_name`/avatar/colour already live together.

## Problem

Four visual refinements to the chamber, driven by Jake:

1. **Spend chart.** The chat Stats tab shows "Token spend by quark" as a categorical
   bar chart. Make it an **area chart** that combines **per-quark and team** spend over
   turns.
2. **Context meter.** The info panel Stats tab shows context Used/Remaining as a tiny
   `BarChart`. Use a **progress bar** instead.
3. **Quark color identity.** Color a quark's model/label by the quark's assigned color
   **everywhere** (log, charts, roster, info), expand the palette, and let the human
   **pick a custom color** per quark, persisted.
4. **About dialog** is too transparent — style it opaque like the info panel and
   Settings.

## Decisions (from Jake)

- **Spend chart:** **cumulative** (running-total) fresh spend over turns; combine
  per-quark areas **and** a team total in one chart.
- **Color scope:** a custom color applies **everywhere** a quark is colored (one
  resolution path), not charts-only.
- **Color storage:** per-repo **`team.json`** (the repo config the chamber already
  writes), i.e. a `SeatOverride` field.

## Architecture

### 1. Combined spend area chart (chat Stats tab)
- Replace the categorical `BarChart` ("Token spend by quark") with a **multi-series
  `AreaChart`** (the fork's `AreaChart` is multi-series — repeated `.y()/.stroke()/
  .fill()/.name()` add series — and **overlays** each series from a shared zero
  baseline, it does not auto-stack).
- **Data:** a new pure model fn
  `spend_timeline(&self, archived, window, now) -> Vec<SpendPoint>` where
  `SpendPoint { step: u32, per_quark_cum: Vec<f64>, team_cum: f64 }`. It folds the same
  windowed, roster-attributed message stream as `stats_for`, in chronological order,
  accumulating a **running total** of `fresh` per quark (roster order) and the team
  sum. One point per quark-turn step (the chronological index). Quarks that haven't
  acted yet carry their last cumulative value (a step function that only rises), so all
  series are defined at every x.
- **Chart:** one **translucent area per quark** in the quark's resolved color (via
  `color_for`), plus a **team-total** series drawn **stroke-only** (accent; transparent
  fill) so it reads as the top trend line without hiding the quark bands. `.natural()`.
  Empty timeline → render nothing (as today).

### 2. Context progress bar (info panel Stats tab)
- Extract the div-based meter already used in the chat stats per-quark cards into a
  shared `progress_meter(frac: f32, fill: Rgba) -> impl IntoElement` helper (SSOT).
- Replace the info panel's context `BarChart` (Used/Remaining) with `progress_meter`,
  keeping the "Context X% · used / total" label row above it. Fill = quark color.

### 3. Per-quark color identity + custom picker
- **Palette:** expand `theme::actor_hue`'s `CHART` array from 6 to ~12 distinct hues.
  `actor_hue` stays the **fallback** (auto hue by name hash).
- **Storage:** add `color: Option<String>` (a `#rrggbb` hex) to `SeatOverride`
  (per-repo, `present_option`-style three-state like `model`/`effort` is unnecessary —
  a plain `Option<String>` skipped-if-none suffices; absent = auto, set = custom).
- **Resolution (one path):** the projection resolves each seat's color onto
  `RosterRow.color: Rgba` (parse the hex; unparseable → auto hue). Add
  `ChamberView::color_for(&self, name: &str) -> Rgba` — look up the roster row's
  `color`; if the name isn't a seated quark (human/gluon/archived), fall back to
  `theme::actor_hue(name)`. Route **every** `actor_hue` call site in `app.rs` (~8: log
  author, chart fills/series, roster, info header) through `color_for` so a custom color
  shows everywhere.
- **Settings control:** in the per-quark Settings editor, a **color swatch** showing the
  current resolved color; clicking opens the fork's `ColorPicker` (an
  `Entity<ColorPickerState>` on `Chamber`). On change, write `SeatOverride.color` (hex)
  via `save_repo_team`. A **Reset** clears it to `None` (back to auto).

### 4. About dialog opacity
- Change the About overlay's card fill to opaque `theme::modal_surface()` + a
  `glass_highlight` border and `INNER_RADIUS`, matching the info panel and Settings
  cards. No behavioral change.

## Security

- A quark **color** is a cosmetic `#rrggbb` string. It is parsed to `Rgba` on read; an
  invalid/oversized/garbage value falls back to the auto hue — never panics, and there is
  no code path where the string is interpreted as anything but a color. No new trust
  boundary (it rides the existing repo-`team.json` write path, already the human's file).

## Phasing (incremental commits)

1. **P1 — small, independent:** About opacity; context `BarChart` → `progress_meter`
   (extract the shared helper).
2. **P2 — color identity:** palette expand; `SeatOverride.color` (lattice) + serde
   round-trip; projection resolves `RosterRow.color`; `color_for` + route the call
   sites; Settings `ColorPicker` swatch + Reset.
3. **P3 — spend chart:** `spend_timeline` (model, tested); swap the chat Stats bar chart
   for the multi-series cumulative `AreaChart`.

## Testing

- `spend_timeline`: cumulation is monotonic; per-quark series align on one step axis; a
  quark that never acts stays flat at 0; window `ts`-filtering matches `stats_for`;
  archived turns fold into wide windows only. Model unit tests.
- `SeatOverride.color` round-trips through serde (present / absent).
- `color_for`: a seat with a custom hex returns it; an invalid hex falls back to auto; a
  non-seat name falls back to `actor_hue`.
- `actor_hue`: the expanded palette is stable per name and has no duplicate hues.
- Full gate `cargo test --workspace --features gui`.

## Out of scope (YAGNI)

- Global (cross-repo) quark colors — per-repo only, matching the existing override model.
- Theming the human/gluon fixed colors (they stay their fixed identity hues).
- Per-series stacked (vs overlaid) areas — overlay + a team line is the chosen shape.
