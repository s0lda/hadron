# Hadron Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement text selection hover bugfix, code block copying and styling, flat dark theme UI adjustments, and Options B & C for the live ACP quark activity stream.

**Architecture:**
- ignore first_mouse clicks in window_selection to prevent sticky highlighting.
- Refine markdown code block styling and wire a Clipboard copy element to the HTML renderer actions hook.
- Set theme backgrounds and roster widths to align with the solid #101010 theme field base.
- Implement the right-rail Activity tab parsing history from field.jsonl, and a live inline chat card that collapses to a summary chip.

**Tech Stack:** Rust, GPUI.

## Global Constraints

- Passing tests only prove compile, find the caller (Rule 1)
- Reuse before you create (Rule 2)
- One definition, one place (SSOT) (Rule 3)
- Never remove a layer because it looks redundant (Rule 4)
- Know your baseline before you touch anything (Rule 5)
- Evidence, not adjectives (Rule 6)
- Name the risk when there is one (Rule 7)
- Make invalid states unrepresentable (Rule 8)

---

### Task 1: Focus Hover-Selection Bugfix (Completed)

**Files:**
- Modify: `crates/gpui-component/crates/ui/src/text/window_selection.rs`

- [x] **Step 1: Check for first_mouse clicks**
  The first_mouse click filter is already implemented in fork `main@448c2d16`.
- [x] **Step 2: Run test suite to verify**
  Tests pass at baseline.
- [x] **Step 3: Commit**
  Landed in fork.

---

### Task 2: Code Block Styling & Copy Button

**Files:**
- Modify: `crates/hadron-chamber/src/app/widgets.rs`
- Modify: `crates/hadron-chamber/src/app/render/chat.rs`

**Interfaces:**
- Consumes: `gpui_component::ui::clipboard::Clipboard` component, `TextView::code_block_actions` hook.
- Produces: Bordered, padded code blocks with a hover "Copy" button.

- [ ] **Step 1: Add code block styling**
  In `crates/hadron-chamber/src/app/widgets.rs` inside the `markdown_style()` helper, find `style.code_block` and change it to use `theme::bg_elevated()` and borders:
  ```rust
  style.code_block = {
      let mut s = gpui::StyleRefinement::default();
      s.background = Some(theme::bg_elevated().into());
      s.border_width = Some(gpui::EdgeRefinement::all(px(1.0)));
      s.border_color = Some(gpui::EdgeRefinement::all(theme::border()));
      s.padding = Some(gpui::EdgesRefinement::all(px(8.0)));
      s.rounded_corner = Some(gpui::CornersRefinement::all(px(6.0)));
      s
  };
  ```

- [ ] **Step 2: Wire copy button to TextView::html**
  In `crates/hadron-chamber/src/app/render/chat.rs` inside `pub(super) fn message_row`, look for the `gpui_component::text::TextView::html` call and append `.code_block_actions(...)`:
  ```rust
  gpui_component::text::TextView::html((view, ix), html)
      .selectable(true)
      .style(markdown_style())
      .code_block_actions(|code_block, _window, _cx| {
          let code = code_block.code();
          gpui_component::ui::clipboard::Clipboard::new("copy").value(code.clone()).into_any_element()
      })
  ```

- [ ] **Step 3: Verify workspace compiles cleanly**
  Run: `cargo check --workspace --features gui`
  Expected: PASS

- [ ] **Step 4: Run unit tests**
  Run: `cargo test -p hadron-chamber --features gui`
  Expected: PASS

- [ ] **Step 5: Commit changes**
  ```bash
  git add crates/hadron-chamber/src/app/widgets.rs crates/hadron-chamber/src/app/render/chat.rs
  git commit -m "feat(chamber): add styling and clipboard copy button to markdown code blocks"
  ```

---

### Task 3: UI Color Themes & Roster Width

**Files:**
- Modify: `crates/hadron-chamber/src/config.rs`
- Modify: `crates/hadron-chamber/src/app/render/chat.rs`
- Modify: `crates/hadron-chamber/src/app/render/terminal.rs`
- Modify: `crates/hadron-chamber/src/app/render/stats.rs`
- Modify: `crates/hadron-chamber/src/app/render/overlays.rs`
- Modify: `crates/hadron-chamber/src/app/actions.rs`

**Interfaces:**
- Consumes: `theme::field_base()` color token.
- Produces: Dark modal backgrounds, wider roster defaults, and matching TabBar segments.

- [ ] **Step 1: Increase default roster width and add configuration migration**
  In `crates/hadron-chamber/src/config.rs`, modify `default_roster_width()`:
  ```rust
  fn default_roster_width() -> f32 {
      500.0
  }
  ```
  In `crates/hadron-chamber/src/config.rs`, modify `load_from()` to migrate 410.0 layout values:
  ```rust
  pub fn load_from(path: &Path) -> ChamberPrefs {
      match std::fs::read_to_string(path) {
          Ok(text) => {
              let mut prefs: ChamberPrefs = serde_json::from_str(&text).unwrap_or_default();
              if prefs.roster_width == 410.0 {
                  prefs.roster_width = 500.0;
              }
              prefs
          }
          Err(_) => ChamberPrefs::default(),
      }
  }
  ```

- [ ] **Step 2: Update segmented TabBar backgrounds**
  In `crates/hadron-chamber/src/app/render/terminal.rs` (Right-rail tabs) and `crates/hadron-chamber/src/app/render/chat.rs` (Chat tabs), append `.bg(theme::field_base())` to the `TabBar::new` builders.
  Example:
  ```rust
  let tabs = TabBar::new("right-rail-tabs")
      .segmented()
      .selected_index(selected.index())
      .bg(theme::field_base())
  ```

- [ ] **Step 3: Update overlays and info panel card backgrounds to match**
  In `crates/hadron-chamber/src/app/render/stats.rs` inside `info_panel_overlay`:
  - Change `.bg(theme::modal_surface())` to `.bg(theme::field_base())` for the `#quark-info-panel` div.
  In `crates/hadron-chamber/src/app/render/overlays.rs`:
  - In `about_overlay`: Change `.bg(theme::modal_surface())` to `.bg(theme::field_base())` for the dialog container.
  - In `completion_card_overlay`: Change `.bg(theme::bg_surface())` to `.bg(theme::field_base())`.

- [x] **Step 4: Update RightRailTab focus test array** *(done alongside Task 4 Step 1, once `RightRailTab::Activity` existed)*
  In `crates/hadron-chamber/src/app/actions.rs` inside `toggle_focus_else_case_switches_rail_to_terminal`, add `RightRailTab::Activity`:
  ```rust
  for tab in [RightRailTab::FileTree, RightRailTab::Changes, RightRailTab::Plan, RightRailTab::Activity]
  ```

- [ ] **Step 5: Verify tests and commit**
  Run: `cargo test -p hadron-chamber --features gui`
  Expected: PASS
  ```bash
  git add crates/hadron-chamber/src/config.rs crates/hadron-chamber/src/app/render/chat.rs crates/hadron-chamber/src/app/render/terminal.rs crates/hadron-chamber/src/app/render/stats.rs crates/hadron-chamber/src/app/render/overlays.rs crates/hadron-chamber/src/app/actions.rs
  git commit -m "style(chamber): set bg colors to theme field_base and bump roster default width to 500"
  ```

---

### Task 4: ACP Quark Activity Stream (Options B & C)

**Files:**
- Modify: `crates/hadron-chamber/src/app/tabs.rs`
- Modify: `crates/hadron-chamber/src/app/render/terminal.rs`
- Modify: `crates/hadron-chamber/src/app/render/chat.rs`

**Interfaces:**
- Consumes: `.hadron/field.jsonl` event log via `io::read_events`.
- Produces: Detailed "Activity" multitool tab, live collapsible chat thought cards.

- [x] **Step 1: Declare RightRailTab::Activity**
  In `crates/hadron-chamber/src/app/tabs.rs`, add the `Activity` variant to `RightRailTab` and update `ALL`, `index()`, `from_index()`, and `label()`:
  ```rust
  #[derive(Clone, Copy, PartialEq, Eq)]
  pub(super) enum RightRailTab {
      Terminal,
      FileTree,
      Changes,
      Plan,
      Activity,
  }

  impl RightRailTab {
      pub(super) const ALL: [RightRailTab; 5] = [
          RightRailTab::Terminal,
          RightRailTab::FileTree,
          RightRailTab::Changes,
          RightRailTab::Plan,
          RightRailTab::Activity,
      ];
      // match index 4 for Activity
  ```

- [x] **Step 2: Render Activity feed in terminal.rs**
  In `crates/hadron-chamber/src/app/render/terminal.rs` inside `terminal_pane`, add `RightRailTab::Activity` branch:
  - Retrieve the events from the log: `let events = io::read_events(&self.path).unwrap_or_default();`
  - Filter events related to the selected/focused roster quark.
  - Render an auto-scrolling log of thoughts, tool executions, and statuses.

- [ ] **Step 3: Render live card and collapsed chip in chat** *(Option C — NOT done this turn; deferred to a focused follow-up, see report)*
  In `crates/hadron-chamber/src/app/render/chat.rs`:
  - When a turn is active for the selected quark, read the live activity payload from `hadron_lattice::live::read`.
  - Render it inline/above the text input.
  - Once completed, render a collapsed summary chip above the generated chat response.

- [x] **Step 4: Verify compilation and tests**
  Run: `cargo test --workspace --features gui`
  Expected: PASS

- [x] **Step 5: Commit changes** *(Option B committed as `8e55814`; chat.rs untouched — that is Option C, deferred)*
  ```bash
  git add crates/hadron-chamber/src/app/tabs.rs crates/hadron-chamber/src/app/render/terminal.rs crates/hadron-chamber/src/app/render/chat.rs
  git commit -m "feat(chamber): implement Option B & C live activity stream views"
  ```
