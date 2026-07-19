# Design Specification: Chamber UI, Styling, and Live Activity Stream

This document specifies the design for the text selection bugfix, the code block enhancements, color theme updates, and the real-time ACP activity stream components.

---

## 1. Chat Focus Hover-Selection Bugfix (Fork-Repo Patch)

### Problem
Clicking an inactive Hadron Chamber window to focus it fires a `MouseDownEvent` but occasionally misses the corresponding `MouseUpEvent` upon focus transition, leaving `is_selecting` permanently set to `true`. This causes text selection to update "stickily" on hover as the mouse moves over the chat area, even when no buttons are pressed.

### Design
Modify the `MouseMoveEvent` handler in `crates/gpui-component/crates/ui/src/text/window_selection.rs` to verify that the left mouse button is held down if selection is active:
```rust
window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
    if !phase.bubble() {
        return;
    }
    Root::update(window, cx, |root, window, cx| {
        if root.text_selection.is_selecting && event.pressed_button != Some(MouseButton::Left) {
            root.end_text_selection(cx);
        } else {
            root.update_text_selection(event.position, window, cx);
        }
    });
});
```

---

## 2. Code Block Formatting & Copy Button

### Problem
Currently, code blocks in the chat view lack distinct styling (such as elevated backgrounds or borders) and do not support copying their contents easily.

### Design
1. **Formatting**: Refine `code_block` style in `crates/hadron-chamber/src/app/widgets.rs`'s `markdown_style()` helper:
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

2. **Copy Button**: Wire `code_block_actions` on the `TextView::html` component inside `crates/hadron-chamber/src/app/render/chat.rs` around line 378:
   ```rust
   gpui_component::text::TextView::html((view, ix), html)
       .selectable(true)
       .style(markdown_style())
       .code_block_actions(|code_block, _window, _cx| {
           let code = code_block.code();
           gpui_component::ui::clipboard::Clipboard::new("copy").value(code.clone()).into_any_element()
       })
   ```

---

## 3. UI Color Themes & Roster Width

### Problem
With the new solid `#101010` background (`theme::field_base()`), the right-rail tabs bar background, the Quark Info panel overlay, the About overlay, and the mentions completion card overlay blend or clash with the background instead of maintaining a clean visual hierarchy. The roster width is also too narrow for comfortable reading.

### Design
1. **Roster Width**:
   - Change `default_roster_width()` in `crates/hadron-chamber/src/config.rs` from `410.0` to `500.0`.
   - Add a migration check in `ChamberPrefs::load()` to automatically bump stored configurations from `410.0` to `500.0`.
2. **Tab Bar Header BG**: Set `.bg(theme::field_base())` on the segmented `TabBar` elements:
   - `crates/hadron-chamber/src/app/render/terminal.rs` (Right-rail tabs)
   - `crates/hadron-chamber/src/app/render/chat.rs` (Chat tabs)
3. **Quark Info Panel**: Set `.bg(theme::field_base())` on the wrapper of `info_panel_overlay` inside `crates/hadron-chamber/src/app/render/stats.rs`.
4. **About Page**: Set `.bg(theme::field_base())` on the dialog card in `about_overlay` inside `crates/hadron-chamber/src/app/render/overlays.rs`.
5. **Mentions Completion Overlay**: Set `.bg(theme::field_base())` and change the border to `theme::border()` in `completion_card_overlay` inside `crates/hadron-chamber/src/app/render/overlays.rs`.

---

## 4. ACP Quark Activity Stream (Options B & C)

### Problem
The live activity stream updates in-place, meaning only the latest single-line update is visible as a subtitle in the roster row. We need both a dedicated historical activity view (multitool tab) and a rich live rendering card (chat area) that collapses on completion.

### Design
1. **Option B (Activity Tab in Multitool)**:
   - Add `RightRailTab::Activity` to `crates/hadron-chamber/src/app/tabs.rs`'s enum.
   - Sourcing History: Retrieve the historical activity timeline for the selected quark by scanning the recent events in `.hadron/field.jsonl`.
   - Render a scrolling list of recent tasks, thoughts, and tool invocations, showing detailed formatting, status icons, and elapsed time.

2. **Option C (Live Card in Chat & Collapse on Completion)**:
   - When a turn is active for a quark, render a live card above the input area or inline in the message list using the live activity file payload from `hadron_lattice::live::read`.
   - When the turn finishes and a new message is appended to the event log, render a collapsed summary chip (e.g., `⟳ thought for 12s · executed 3 tools`) immediately preceding the message block.
