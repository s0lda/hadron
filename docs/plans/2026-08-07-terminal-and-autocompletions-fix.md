# Windows PTY Terminal & Autocompletions Layout Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve Windows PTY loading/black screen and sluggish typing by dropping the slave handle post-spawn and restoring dynamic term dims, and fix the completion overlay row alignment so descriptions stay right-aligned without overflowing on small window sizes.

**Architecture:** 
- In `crates/hadron-chamber/src/pty.rs`, remove `_slave` field from `PtyTerminal` struct and call `drop(pair.slave)` post-spawn so ConPTY doesn't block master output reads.
- In `crates/hadron-chamber/src/app/mod.rs`, revert `term_dims` to floored cell counts with a minimum grid of 2x2.
- In `crates/hadron-chamber/src/app/render/overlays.rs`, update `completion_card_overlay` to include `gap_2()`, `w_full()`, `overflow_hidden()`, `flex_shrink_0()` on label, and `min_w_0()`, `truncate()`, `text_right()` on detail.

**Tech Stack:** Rust, GPUI, portable-pty, alacritty_terminal.

## Global Constraints

- Preserve all existing tests and docstrings.
- Maintain single-row `justify_between` completion layout for tag/file/command on left and description on right.
- Ensure strict type safety and clean workspace build across crates.

---

### Task 1: Fix Windows PTY Slave Lifecycle and Terminal Dimension Calculation

**Files:**
- Modify: `crates/hadron-chamber/src/pty.rs:116-126,247-255,315-326`
- Modify: `crates/hadron-chamber/src/app/mod.rs:143-147`

**Interfaces:**
- Consumes: `portable_pty::{MasterPty, SlavePty, Child}`
- Produces: `PtyTerminal` struct without persistent `_slave` handle and dynamic `term_dims` function.

- [ ] **Step 1: Write/Update unit test or verify build failure setup for PTY lifecycle**

Run: `cargo test --package hadron-chamber --lib pty::tests`
Expected: PASS existing pty tests.

- [ ] **Step 2: Update `PtyTerminal` struct and spawn logic in `crates/hadron-chamber/src/pty.rs`**

In `crates/hadron-chamber/src/pty.rs`:
1. Remove `_slave: Box<dyn SlavePty + Send>,` from `pub struct PtyTerminal`.
2. After `let (child, pair) = match child_and_pair ...`, add `drop(pair.slave);`.
3. In `Self { ... }` instantiation, remove `_slave: pair.slave,`.

Code snippet for `crates/hadron-chamber/src/pty.rs`:
```rust
// Lines 116-126
pub struct PtyTerminal {
    pub title: String,
    term: Arc<Mutex<Term<VoidListener>>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    _child: Box<dyn Child + Send + Sync>,
    dirty: Arc<AtomicBool>,
    cols: usize,
    rows: usize,
}
```
And post-spawn:
```rust
        let (child, pair) = match child_and_pair {
            Some(cp) => cp,
            None => return Err(last_err),
        };

        // Dropping the slave handle post-spawn is required on Windows ConPTY;
        // holding it open prevents the master reader thread from receiving output.
        drop(pair.slave);
```

- [ ] **Step 3: Update `term_dims` in `crates/hadron-chamber/src/app/mod.rs`**

In `crates/hadron-chamber/src/app/mod.rs` (lines 143-147):
```rust
fn term_dims((w, h): (f32, f32)) -> (usize, usize) {
    (
        ((w / TERM_CELL_W).floor() as usize).max(2),
        ((h / TERM_CELL_H).floor() as usize).max(2),
    )
}
```

- [ ] **Step 4: Run cargo test to verify PTY unit tests pass**

Run: `cargo test --package hadron-chamber --lib pty`
Expected: PASS.

- [ ] **Step 5: Commit PTY & term_dims fixes**

```bash
git add crates/hadron-chamber/src/pty.rs crates/hadron-chamber/src/app/mod.rs
git commit -m "fix(windows): drop slave pty handle post-spawn and restore dynamic term_dims"
```

---

### Task 2: Fix Autocompletions Overlay Row Layout & Text Truncation

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/overlays.rs:176-192`

**Interfaces:**
- Consumes: GPUI `div()`, `theme` tokens, `CompletionCard` candidates (`label`, `detail`).
- Produces: Flex single-row item with bounded label and truncated detail right-aligned.

- [ ] **Step 1: Inspect existing overlay rendering**

Check `completion_card_overlay` in `crates/hadron-chamber/src/app/render/overlays.rs`.

- [ ] **Step 2: Update row layout elements in `crates/hadron-chamber/src/app/render/overlays.rs`**

In `crates/hadron-chamber/src/app/render/overlays.rs` (around lines 176-192):
```rust
                list = list.child(
                    div()
                        .id(("completion-row", i))
                        .flex()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .overflow_hidden()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .when(selected, |s| s.bg(theme::glass_highlight()))
                        .hover(|s| s.bg(theme::glass_highlight()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text())
                                .flex_shrink_0()
                                .child(label),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .min_w_0()
                                .truncate()
                                .text_right()
                                .child(detail),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(c) = this.completion.as_mut() {
                                c.selected = i;
                            }
                            this.accept_completion(window, cx);
                        })),
                );
```

- [ ] **Step 3: Run workspace tests to verify compilation and tests pass**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 4: Commit autocompletions overlay fix**

```bash
git add crates/hadron-chamber/src/app/render/overlays.rs
git commit -m "fix(ui): enforce gap and truncation on completion card overlay rows"
```

---

### Task 3: Full Workspace Verification and Spec Alignment

- [ ] **Step 1: Check workspace compilation across all crates**

Run: `cargo check --workspace`
Expected: PASS with 0 errors.

- [ ] **Step 2: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 3: Verify git status is clean**

Run: `git status`
Expected: Clean working tree.
