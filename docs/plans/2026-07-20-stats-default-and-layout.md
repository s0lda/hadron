# Stats Sub-tabs Default and Width Adjustment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change the default stats window view from Session to Current and wrap the stats tab bar in a horizontal flex layout so its width shrinks to fit its children rather than stretching full-width.

**Architecture:** 
1. Modify `Chamber::new` initialization inside `crates/hadron-chamber/src/app/mod.rs` to set the default `stats_window` to `StatsWindow::Current`.
2. Wrap `TabBar::new(id)` inside `stats_window_tabs` in `crates/hadron-chamber/src/app/render/stats.rs` in `h_flex()` so it naturally shrinks to fit.
3. Update the fallback default in the `on_click` listener of the stats tabs to `StatsWindow::Current`.

**Tech Stack:** Rust, GPUI, GPUI-Component

## Global Constraints
- Do not modify `gpui-component` crate files directly (outside hadron worktree).
- Maintain existing styling tokens, backgrounds, and layout structures.

---

### Task 1: Modify Default Stats Window and Tab Layout

**Files:**
- Modify: `crates/hadron-chamber/src/app/mod.rs:520-525`
- Modify: `crates/hadron-chamber/src/app/render/stats.rs:440-475`

**Interfaces:**
- Consumes: `StatsWindow::Current`
- Produces: Correct default selection and custom width fit for stats tabs.

- [ ] **Step 1: Edit app initialization to default to Current**
  Modify `crates/hadron-chamber/src/app/mod.rs` to set `stats_window` to `StatsWindow::Current`.

  ```rust
  // Line 522
  stats_window: StatsWindow::Current,
  ```

- [ ] **Step 2: Update tab bar wrapper and fallback selection in stats.rs**
  Modify `crates/hadron-chamber/src/app/render/stats.rs` to wrap the tab bar in `h_flex().child(...)` and update fallback in `on_click`.

  ```rust
  pub(super) fn stats_window_tabs(&self, id: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
      let selected = self.stats_window;
      let sel_ix = StatsWindow::ALL
          .iter()
          .position(|w| *w == selected)
          .unwrap_or(0);
      h_flex().child(
          TabBar::new(id)
              .segmented()
              .bg(theme::field_base())
              .selected_index(sel_ix)
              .children(StatsWindow::ALL.map(|w| {
                  if w == selected {
                      Tab::new().child(
                          div()
                              .text_color(theme::accent())
                              .child(w.label().to_string()),
                      )
                  } else {
                      Tab::new().label(w.label())
                  }
              }))
              .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                  this.stats_window = StatsWindow::ALL
                      .get(*ix)
                      .copied()
                      .unwrap_or(StatsWindow::Current);
                  cx.notify();
              }))
      )
  }
  ```

- [ ] **Step 3: Run the tests to verify the compile and pass**
  Run: `cargo test -p hadron-chamber --features gui`
  Expected: All 111 tests pass successfully.

- [ ] **Step 4: Commit**
  Run:
  ```bash
  git add crates/hadron-chamber/src/app/mod.rs crates/hadron-chamber/src/app/render/stats.rs
  git commit -m "feat(chamber): default stats tabs to Current and adjust width to wrap content"
  ```
