# Windows ConPTY Terminal & Autocompletions UI Layout Fix Design

## Context & Problem Statement
Recent commits introduced two issues:
1. **Windows Terminal Bug**: `PtyTerminal` preserved `_slave: Box<dyn SlavePty + Send>` handle open inside the struct (`crates/hadron-chamber/src/pty.rs`). On Windows ConPTY, holding the slave handle open prevents the master reader from receiving stdout, rendering a black screen with a sluggish blinking cursor. Additionally, `term_dims` in `crates/hadron-chamber/src/app/mod.rs` was forced to minimum 80x24 grid even when the UI terminal view was smaller.
2. **Autocompletions Alignment Bug**: In `crates/hadron-chamber/src/app/render/overlays.rs`, each completion row uses `flex()` and `justify_between()` for the label (left) and description (right). Without a minimum gap (`gap_2`), `flex_shrink_0`, and `truncate` / `min_w_0` constraints, smaller window sizes cause the description to overflow outside the completion card container.

## Architecture & Changes

### 1. Windows PTY Terminal (`crates/hadron-chamber/src/pty.rs` & `crates/hadron-chamber/src/app/mod.rs`)
- Remove `_slave: Box<dyn SlavePty + Send>` from `PtyTerminal` struct.
- Call `drop(pair.slave)` explicitly after spawning `child` on Windows/cross-platform.
- Revert `term_dims` in `src/app/mod.rs` to floored cell counts with a minimum grid of 2x2 based on available view dimensions.

### 2. Autocomplete Completion Overlay (`crates/hadron-chamber/src/app/render/overlays.rs`)
- Update `completion_card_overlay` row layout:
  - Row container: `div().id(("completion-row", i)).flex().justify_between().items_center().gap_2().w_full().overflow_hidden()`
  - Left item (Label): `div().text_sm().text_color(theme::text()).flex_shrink_0().child(label)`
  - Right item (Detail): `div().text_xs().text_color(theme::text_muted()).min_w_0().truncate().text_right().child(detail)`

## Verification Plan
1. `cargo check --workspace` to ensure clean compilation.
2. `cargo test --workspace` to ensure all unit and integration tests pass.
3. Verify PTY terminal initialization behavior on Windows/Linux.
4. Verify completion card overlay layout rendering on narrow and wide window sizes.
