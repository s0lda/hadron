# Adjust Stats Tabs and Context Menus Colors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Adjust the Stats tabs (Session / Week / Month / All time) and the context menus to the current UI palette (`field_base` `#101010` flatten from `47c8ed1`).

**Architecture:** Use `theme::field_base()` on the TabBars in `stats.rs` (both `info_tabs` and `stats_window_tabs`) to dissolve them into the background. Update `theme::popover()` in `theme.rs` to return `field_base()` and override `t.tokens.popover` in `app/mod.rs` to inherit from `theme::popover()`.

**Tech Stack:** Rust, GPUI

## Global Constraints

- Adjust colors to flat `#101010` field_base.
- Maintain existing styles and ensure all tests compile and pass.

---

### Task 1: Update theme::popover and app::mod.rs theme override

**Files:**
- Modify: `crates/hadron-chamber/src/theme.rs:123-125`
- Modify: `crates/hadron-chamber/src/app/mod.rs:681-688`
- Test: Build the workspace to verify it compiles.

- [ ] **Step 1: Modify theme::popover to return field_base**
  Change `theme::popover()` in `crates/hadron-chamber/src/theme.rs` to:
  ```rust
  pub fn popover() -> Rgba {
      field_base() // flat #101010 field color for context menus (Jake's request)
  }
  ```

- [ ] **Step 2: Update gpui-component popover theme token overrides in app/mod.rs**
  Change `t.tokens.popover` in `crates/hadron-chamber/src/app/mod.rs` to use `theme::popover()`:
  ```rust
              t.popover = theme::popover().into();
              // Context menus, dropdown menus and tooltips paint from `tokens.popover`, which
              // is computed once at theme construction and does NOT re-derive from the mutated
              // `colors.popover` above — so without this line they stay the stock-dark theme
              // colour (near-black) instead of our surface. Same gotcha as `tokens.title_bar`.
              t.tokens.popover = gpui::Hsla::from(theme::popover()).into();
  ```

- [ ] **Step 3: Verify build**
  Run: `cargo check`
  Expected: Successful compilation

- [ ] **Step 4: Commit changes**
  ```bash
  git add crates/hadron-chamber/src/theme.rs crates/hadron-chamber/src/app/mod.rs
  git commit -m "style(chamber): flatten context menu popover background to field_base"
  ```

### Task 2: Update TabBars in stats.rs to dissolve into field_base

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/stats.rs:290-293`, `426-429`
- Test: Build the workspace and run workspace tests.

- [ ] **Step 1: Add .bg(theme::field_base()) to info_tabs in stats.rs**
  Modify `info_tabs` rendering:
  ```rust
          let info_tabs = TabBar::new("info-tabs")
              .segmented()
              .bg(theme::field_base())
              .selected_index(info_selected.index())
  ```

- [ ] **Step 2: Add .bg(theme::field_base()) to stats_window_tabs in stats.rs**
  Modify `stats_window_tabs` rendering:
  ```rust
          TabBar::new(id)
              .segmented()
              .bg(theme::field_base())
              .selected_index(sel_ix)
  ```

- [ ] **Step 3: Run full workspace tests**
  Run: `cargo test --workspace --features gui`
  Expected: PASS

- [ ] **Step 4: Commit changes**
  ```bash
  git add crates/hadron-chamber/src/app/render/stats.rs
  git commit -m "style(chamber): flatten stats tab bars background to field_base"
  ```
