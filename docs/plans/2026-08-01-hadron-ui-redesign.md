# Hadron UI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign `hadron-chamber` into "The Unified Swarm Command Deck"—a borderless glass desktop UI with zero-lag AI streaming, multi-terminal PTY inspector, and AST Forge diff visualization.

**Architecture:** Refactor `hadron-chamber` render layers (`theme.rs`, `roster.rs`, `chat.rs`, `terminal.rs`, `git.rs`, `stats.rs`) to use elevated glass containers without 1px box borders, pre-allocated plain-text streaming cards, and multi-instance PTY tab state.

**Tech Stack:** Rust 2021, GPUI framework, `gpui-component`, Tree-Sitter, Blake3.

## Global Constraints

- **Borderless Surfaces**: Replace rigid 1px box borders with elevated translucency and subtle top/side highlight rims.
- **Zero-Lag Invariants**: No full-window blur filters during token streaming. Draft text streams as plain text within pre-allocated cards to avoid markdown parsing overhead.
- **Multi-Terminal PTY**: Support multiple tabbed terminal sessions inside the Inspector (`Ctrl+Shift+T` / `Ctrl+Shift+W`).
- **Compiler Floor**: Rust 1.96.0+, GPUI rendering loop optimization profile enabled.

---

### Task 1: Glass Theme Tokens & Surface Hierarchy

**Files:**
- Modify: `crates/hadron-chamber/src/theme.rs`
- Test: `crates/hadron-chamber/src/theme.rs` (unit tests)

**Interfaces:**
- Consumes: GPUI `hsla`, `rgba`, `Element` utilities
- Produces: `theme::canvas_base()`, `theme::glass_surface()`, `theme::glass_card()`, `theme::glass_highlight()`, `theme::halo_active()`, `theme::halo_reasoning()`

- [ ] **Step 1: Write tests for theme color tokens**

Add unit tests to `theme.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glass_theme_tokens() {
        let base = canvas_base();
        let surface = glass_surface();
        let card = glass_card();
        assert_ne!(base, surface);
        assert_ne!(surface, card);
    }
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p hadron-chamber --lib theme::tests::test_glass_theme_tokens`
Expected: FAIL (missing `canvas_base`, `glass_card`, etc.)

- [ ] **Step 3: Implement glass surface theme tokens in `theme.rs`**

Add token functions in `crates/hadron-chamber/src/theme.rs`:
```rust
pub fn canvas_base() -> gpui::Hsla {
    gpui::rgb(0x090b10).into()
}

pub fn glass_surface() -> gpui::Hsla {
    gpui::rgba(0x111520e6).into()
}

pub fn glass_card() -> gpui::Hsla {
    gpui::rgba(0x181e2ce6).into()
}

pub fn glass_highlight() -> gpui::Hsla {
    gpui::rgba(0xffffff0f).into()
}

pub fn halo_active() -> gpui::Hsla {
    gpui::rgb(0x00e5ff).into() // Cyan for tool execution
}

pub fn halo_reasoning() -> gpui::Hsla {
    gpui::rgb(0xaa00ff).into() // Violet for reasoning
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib theme::tests::test_glass_theme_tokens`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/theme.rs
git commit -m "feat(chamber): add borderless glass theme tokens and halo color utilities"
```

---

### Task 2: Left Rail Fleet Roster Redesign

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/roster.rs`
- Test: `crates/hadron-chamber/src/app/render/roster.rs`

**Interfaces:**
- Consumes: `theme::glass_surface()`, `theme::halo_active()`, `QuarkId`, `View`
- Produces: `Chamber::roster_pane(&mut self, cx)` with borderless glass cards, status halos, and F6 security deck.

- [ ] **Step 1: Write test for Quark status halo resolution**

```rust
#[test]
fn test_quark_status_halo_color() {
    assert_eq!(theme::halo_active(), gpui::rgb(0x00e5ff).into());
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib app::render::roster::tests`
Expected: PASS

- [ ] **Step 3: Refactor `roster_pane` to borderless glass cards**

Update `crates/hadron-chamber/src/app/render/roster.rs` to render:
1. Floating Worktree Chip header (`bg(theme::glass_card())`, `rounded_lg()`).
2. Quark Fleet list using `theme::glass_surface()` with 8px vector halo indicators (no animated box shadows).
3. Bottom F6 security posture pill (`ASK` / `WRITE` / `AUTO` / `BYPASS`).

- [ ] **Step 4: Verify build clean**

Run: `cargo check -p hadron-chamber`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/app/render/roster.rs
git commit -m "feat(chamber): update left rail fleet roster with borderless glass cards and status halos"
```

---

### Task 3: Center Deck Stream & Floating Command Capsule

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/chat.rs`
- Test: `crates/hadron-chamber/src/app/render/chat.rs`

**Interfaces:**
- Consumes: `streaming_drafts`, `active_quarks`, `chat_pane`
- Produces: `chat_pane` featuring floating capsule tabs (`Chat` | `Event Log` | `Timeline`), zero-lag plain text streaming cards, AST Forge diff cards, and floating 12px elevated command dock.

- [ ] **Step 1: Verify draft streaming plain text card logic**

Check `chat.rs` line 55-68 to ensure plain text rendering is maintained for zero-lag streaming.

- [ ] **Step 2: Refactor input deck to Floating Command Capsule**

Update `chat_pane` in `crates/hadron-chamber/src/app/render/chat.rs`:
```rust
let input_capsule = v_flex()
    .w_full()
    .px_4()
    .pb_3()
    .child(
        v_flex()
            .w_full()
            .bg(theme::glass_card())
            .rounded_xl()
            .shadow_lg()
            .border_1()
            .border_color(theme::glass_highlight())
            .child(self.message_input(window, cx))
    );
```

- [ ] **Step 3: Run cargo check**

Run: `cargo check -p hadron-chamber`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/hadron-chamber/src/app/render/chat.rs
git commit -m "feat(chamber): convert center deck to floating command capsule and elevated glass timeline"
```

---

### Task 4: Multi-Terminal PTY Inspector

**Files:**
- Modify: `crates/hadron-chamber/src/pty.rs`
- Modify: `crates/hadron-chamber/src/app/render/terminal.rs`
- Test: `crates/hadron-chamber/src/app/render/terminal.rs`

**Interfaces:**
- Consumes: `PtyState`, `terminal_pane`
- Produces: Tabbed PTY terminal manager supporting `+` (new tab), tab switching, and `Ctrl+Shift+T` / `Ctrl+Shift+W` shortcuts.

- [ ] **Step 1: Write test for PTY tab management state**

In `crates/hadron-chamber/src/pty.rs`:
```rust
#[test]
fn test_pty_tab_list_creation() {
    let mut tabs = vec!["bash #1".to_string()];
    tabs.push("bash #2".to_string());
    assert_eq!(tabs.len(), 2);
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib pty::tests::test_pty_tab_list_creation`
Expected: PASS

- [ ] **Step 3: Add multi-terminal tab bar to `terminal_pane`**

Update `crates/hadron-chamber/src/app/render/terminal.rs` to render terminal instance tab pills (`bash #1`, `npm run dev`) with `+` new tab button and close (`×`) actions.

- [ ] **Step 4: Verify build clean**

Run: `cargo check -p hadron-chamber`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/pty.rs crates/hadron-chamber/src/app/render/terminal.rs
git commit -m "feat(chamber): implement multi-terminal PTY tabs in inspector pane"
```

---

### Task 5: Git Worktrees, Merge Gate & Telemetry Panels

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/git.rs`
- Modify: `crates/hadron-chamber/src/app/render/stats.rs`

**Interfaces:**
- Consumes: `git_pane`, `stats_pane`
- Produces: Borderless glass cards for Git Worktrees, Merge Gate test pipeline status, and zero-CPU token telemetry stats.

- [ ] **Step 1: Update `git.rs` and `stats.rs` to use glass surfaces**

Replace 1px box borders in `git.rs` and `stats.rs` with `theme::glass_surface()` and `theme::glass_card()`.

- [ ] **Step 2: Verify full crate build & unit tests**

Run: `cargo test -p hadron-chamber`
Expected: PASS (all tests pass clean)

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-chamber/src/app/render/git.rs crates/hadron-chamber/src/app/render/stats.rs
git commit -m "feat(chamber): update Git Worktrees and Telemetry panels to borderless glass hierarchy"
```

---

### Task 6: Full Binary Suite Verification & Release Check

**Files:**
- Test: Entire workspace (`cargo check --workspace`, `cargo test --workspace`)

- [ ] **Step 1: Run full workspace build check**

Run: `cargo check --workspace`
Expected: PASS clean with exit code 0.

- [ ] **Step 2: Run full workspace tests**

Run: `cargo test --workspace`
Expected: PASS clean.

- [ ] **Step 3: Commit final plan verification check**

```bash
git add .
git commit -m "chore(hadron): verify Hadron UI redesign compilation and test suite"
```
