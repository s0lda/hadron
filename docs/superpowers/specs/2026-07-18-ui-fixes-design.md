# Chamber UI fixes (WS3) — design

**Status:** design (autonomous; **all three are GUI and cannot be verified headless on WSL2 — logic is unit-tested, visual/interaction behaviour is explicitly marked NEEDS-YOUR-EYES**).
**Date:** 2026-07-18
**Branch:** `feat/ui-fixes` (stacked on `feat/prompt-bloat-trim`)
**Grounded in:** `.superpowers/sdd/ws3-diagnosis.md` (read-only root-cause trace).

Three independent issues the user reported. Each is one task. **Honesty contract:** these fixes are traced-correct in source and unit-tested where a pure seam exists, but no gpui window can paint here — every "it now scrolls / switches / looks right" claim needs the user at a real window.

## Issue A — Left sidebar: width + per-quark effort/mode tags + folded avatars

**Findings:** width is a fixed constant `default_roster_width()=245.0` (`config.rs:59-61`), not drag-resizable; `effort_tag` already renders when `r.effort` is `Some` (`render.rs:783-792`); `mode_tag` (`widgets.rs:435-449`) early-returns empty unless the quark has an explicit override, so default-mode quarks show no tag at any width; folded `rail_strip()` (`render.rs:680-724`) renders no per-quark content.

**Design:**
1. **Width:** raise `default_roster_width()` 245→**310** (fits avatar + two small tags without truncating a typical name). Minimal, matches "made wider". (A drag handle for the roster like the terminal rail's is a larger change — deferred; noted.)
2. **Mode tag always (autonomous decision):** drop `mode_tag`'s `!is_override` early return; render the **resolved** mode for every quark, styled per the function's own doc intent — **override = solid, inherited/global = outlined**. This is what actually delivers "a mode tag next to each Quark". The resolved mode comes from the same source the row already has (the row's `Mode` + an `is_override` bool).
3. **Effort tag:** unchanged (renders when set). If desired later, show inherited effort outlined too — out of scope now (effort has no global-default concept as clean as mode; keep the override-only tag).
4. **Folded avatars:** add a per-quark loop to `rail_strip()` for `Rail::Roster`, reusing the avatar+dot from `roster_row()` (`identity_avatar(id, diameter)` in `identity.rs:87-106` + the status dot coloured by `theme::presence(state)`), at a smaller diameter (~24px) stacked above the pinned Settings button in the 44px `RAIL_STRIP`.

**Testable:** the width value; the `mode_tag(mode, is_override)` presence/style branch (a pure `fn → AnyElement` — assert non-empty for the non-override case, which is empty today). **Needs-eyes:** whether 310px looks right, folded-avatar legibility at 24px, truncation with long names + both tags.

## Issue B — Chat input: Shift+Enter newline not visible / caret scrolls off

**Findings:** the `\n` IS inserted on Shift+Enter (`enter()` at fork `state.rs:1615-1653`); the bug is `enter()` never calls `scroll_to(cursor)` after the insert the way `paste()` does (`state.rs:2055`), so the input's own viewport doesn't follow the caret. The chamber-side "shift branch" (`mod.rs:760-768`) scrolls `chat_scrolls` — the **transcript** pane, not the input — dead-weight that looks plausible.

**Design:**
1. **Fork fix (out-of-tree, documented):** in the compiled fork `/tmp/gpui-component` (branch `text-mark-color`, the same fork already patched for `TextMark::color`), `crates/ui/src/input/state.rs`, inside `enter()`'s `if insert_newline` block right after `replace_text_in_range_silent` (~line 1641), add `self.scroll_to(self.cursor(), None, cx);` — mirroring `paste()` exactly. Fixes both symptoms and every newline-insert path, not just Shift+Enter. Add a fork-side `gpui::test` following the existing pattern (`state.rs:3320` `test_scroll_to_eob_does_not_overshoot_safe_range`) asserting `scroll_handle.offset()` moves after a multi-line `enter()` that overflows the visible rows.
2. **In-repo cleanup:** delete the misdirected `mod.rs:760-768` shift branch (it does nothing for the input). If keeping the transcript pinned-to-bottom on send is still wanted, that's a separate concern; do not conflate.
3. **In-repo pin bump:** after committing the fork change, `cargo update -p gpui-component` (and the two sibling crates) so `Cargo.lock` points at the new fork commit — this Cargo.lock change is the reviewable in-repo trace of the fork fix.

**Reviewability caveat (autonomous flag):** the actual scroll fix lives in `/tmp/gpui-component`, outside the hadron repo — it appears in the hadron diff only as a `Cargo.lock` pin bump. The fork commit persists on that branch but `/tmp` is ephemeral; the user should confirm the fork commit is where they want it (or cherry-pick it into their canonical gpui-component fork). **Needs-eyes:** whether it *feels* right (smooth scroll to the new line) at a real window.

## Issue C — Keyboard chat↔terminal switch was never built

**Findings:** the planned `ToggleFocus` action (switch focus chat↔terminal) is absent from the `actions!` macro (`mod.rs:77-92`) — never implemented. Bare `Tab` isn't bound by Chamber, so it falls through to gpui `Root`'s `tab → focus_next()` (ordinary tab-stop traversal = "hamburger → chat"). What shipped are same-column cyclers (`ctrl-tab` Chat/Log/Stats; `ctrl-pagedown` Terminal/FileTree/Changes/Plan). Also: `shift-tab → CycleMode` (`mod.rs:1079`) is already dead while typing because Input claims `shift-tab` for `OutdentInline`.

**Design:**
1. **Add `ToggleFocus`:** register in `actions!` (`mod.rs:77-92`); add an `on_action` handler (near `render.rs:71-86`) that toggles window focus between `self.input`'s focus handle and the terminal pane's focus handle **when `RightRailTab::Terminal` is the active right-rail tab**; otherwise just focus chat (or make the terminal tab active then focus it — autonomous choice: if terminal isn't the active right tab, switch the right rail to Terminal and focus it, so one chord reliably reaches the terminal; a second press returns to chat).
2. **Chord (autonomous decision, must be verified):** primary `ctrl-\`` (backtick — the widespread terminal-toggle convention). The implementer MUST grep every `KeyBinding::new(...)` in the fork's `input/state.rs` and confirm the chosen chord is unclaimed by Input; if `ctrl-\`` is claimed or awkward, fall back to `F6`. Do NOT use bare `Tab` or `shift-tab`.
3. **Fix the dead `shift-tab → CycleMode`:** move `CycleMode` to a verified-free chord (Input does not claim it), so mode-cycling works while typing. Update the inaccurate comment at `mod.rs:1069-1077`.

**Testable:** any pure state-transition/index logic for the toggle (mirror `cycle_chat_tab` tests in `actions.rs:114-127`); the keybinding registration is static. **Needs-eyes (and unverifiable here):** whether `ctrl-\``/`ctrl-tab` actually reach the app past the WSL2/host WM, whether the toggle lands focus where expected, whether the chord feels right and doesn't collide with a browser/WM chord.

## Security note (Rule 7)
None of the three touch auth, permissions, files, network, or untrusted input. Issue B spawns nothing new (it scrolls a viewport); Issue C moves focus; Issue A renders existing data. No new attack surface.

## Autonomous judgment calls (flag for review)
- **A2:** mode tag now renders for ALL quarks (resolved mode), not just overrides — a behaviour change the diagnosis explicitly said "needs a call." Chosen because it's what "a tag next to each Quark" means. Reversible.
- **A1:** width 310 is a guess at "wide enough"; trivially tunable.
- **B:** the real fix is in the /tmp fork; only a Cargo.lock bump lands in-repo. If you'd rather not carry a fork patch, the alternative is a chamber-side workaround (harder — the input's scroll is `pub(crate)`), which I did not take.
- **C1:** if the terminal tab isn't showing, `ToggleFocus` switches the right rail to Terminal and focuses it (one chord always reaches the terminal). Alternative: no-op when terminal hidden. Chosen the more useful behaviour.
- **C2:** chord `ctrl-\`` (fallback `F6`) — pending the implementer's verification against Input bindings and your real-window confirmation it isn't WM-stolen.
