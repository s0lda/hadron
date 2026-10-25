use super::*;
use hadron_lattice::term::{self, Source};

impl super::Chamber {
    /// Drive the live terminal each tick: spawn the PTY lazily when the Terminal
    /// tab is open (once its size has settled), keep it sized to the measured
    /// screen, and repaint only when the child has produced new output (an idle
    /// terminal forces no frames).
    pub(super) fn pump_terminal(&mut self, cx: &mut Context<Self>) {
        if self.right_rail_tab != RightRailTab::Terminal || self.prefs.inspector_collapsed {
            return;
        }
        crate::sys::init_windows_app_icon();
        // Translate the last painted screen size into a column/row grid (None
        // until the first frame has measured it).
        let dims = self.terminal_px.get()
            .filter(|&(_, _, w, h)| w > 100.0 && h > 100.0)
            .map(|(_, _, w, h)| term_dims((w, h)));

        if self.terminals.is_empty() {
            match dims {
                Some((cols, rows)) => {
                    let root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                    let custom_shell = self.prefs.terminal_shell.as_deref();
                    let scrollback = self.prefs.terminal_scrollback;
                    let title = crate::pty::terminal_tab_title(&root, custom_shell, 1);
                    term::info(
                        Source::Chamber,
                        &format!(
                            "terminal: initializing tab '{title}' in '{}' ({}x{})",
                            root.display(),
                            cols,
                            rows
                        ),
                    );
                    match crate::pty::PtyTerminal::new_with_options(&root, cols, rows, custom_shell, scrollback) {
                        Ok(mut term) => {
                            term::info(Source::Chamber, &format!("terminal: created tab '{title}'"));
                            term.title = title.clone();
                            self.terminals.push(TerminalTab {
                                title,
                                term: Some(term),
                                error: None,
                            });
                            self.active_terminal_index = 0;
                            // The panel is still settling its width as the window
                            // opens; keep re-measuring so we track it to the final
                            // size (see `terminal_warmup`).
                            self.terminal_warmup = 20;
                        }
                        Err(err) => {
                            term::error(Source::Chamber, &format!("terminal: error creating tab '{title}': {err}"));
                            self.terminals.push(TerminalTab {
                                title,
                                term: None,
                                error: Some(err),
                            });
                        }
                    }
                }
                // No frame has measured the screen yet — force one so we can size
                // the PTY before spawning it.
                None => {}
            }
            cx.notify();

            return;
        }

        let active_ix = self.active_terminal_index;
        if let Some(term) = self.terminals.get_mut(active_ix).and_then(|tab| tab.term.as_mut()) {
            if let Some((cols, rows)) = dims {
                if term.size() != (cols, rows) {
                    // The measured size moved (the window is still opening, or the
                    // user dragged the splitter). Resize the PTY — bash redraws its
                    // prompt on the resulting SIGWINCH — and re-arm the warmup so we
                    // keep re-measuring until the size stops moving.
                    term.resize(cols, rows);
                    self.terminal_warmup = 20;
                }
            }
            let mut want_paint = term.take_dirty();
            if self.terminal_warmup > 0 {
                self.terminal_warmup -= 1;
                want_paint = true;
            }
            if want_paint {
                cx.notify();
            }
        }
    }


    /// Handle Escape key (or Dismiss action) while the terminal is focused.
    /// Sends `0x1b` (ASCII ESC) to the active PTY unless a modal/overlay is open,
    /// and stops action propagation so focus is not stolen away to the chat prompt.
    pub(super) fn on_terminal_escape(&mut self, cx: &mut Context<Self>) {
        if self.settings_open
            || self.info_panel.is_some()
            || self.about_open
            || self.changelog_open
            || self.app_menu_open
            || self.process_manager_open
            || self.file_tree_open.is_some()
        {
            return;
        }
        let active_ix = self.active_terminal_index;
        if let Some(term) = self.terminals.get_mut(active_ix).and_then(|tab| tab.term.as_mut()) {
            term.selection_clear();
            term.scroll_to_bottom();
            term.send_input(&[0x1b]);
            cx.notify();
        }
        cx.stop_propagation();
    }

    /// Translate a keystroke into the bytes a TTY expects and stream them to the
    /// child. Covers the printable range, the essential control keys, Ctrl+letter
    /// control codes, and the arrow/nav escape sequences. (Function keys, mouse
    /// reporting, and the kitty keyboard protocol are not wired yet.)
    pub(super) fn on_terminal_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active_ix = self.active_terminal_index;
        let ks = &event.keystroke;
        let m = &ks.modifiers;

        let is_paste = (m.control && ks.key == "v")
            || (m.control && m.shift && (ks.key == "v" || ks.key == "V"))
            || (m.platform && ks.key == "v");
        if is_paste {
            self.paste_to_terminal(None, cx);
            return;
        }

        // Support Ctrl+C/Cmd+C/Ctrl+Shift+C copying selected text first
        let is_copy = (m.control && ks.key == "c")
            || (m.control && m.shift && (ks.key == "c" || ks.key == "C"))
            || (m.platform && ks.key == "c");
        if is_copy {
            // The mouse selection lives in the VTE grid (`pty::selection_*`), not in
            // the fork's TextView layer — the grid renders as raw `div`s the TextView
            // selection can't see, so read the copy text from the terminal itself.
            if self.copy_terminal_selection(cx) {
                return;
            }
            // If no text is selected, fallback to copying the entire screen
            // ONLY if they used the explicit full-copy shortcut Ctrl+Shift+C / Cmd+C.
            // If they just pressed bare Ctrl+C, let it fall through so they can send SIGINT!
            if (m.control && m.shift && (ks.key == "c" || ks.key == "C"))
                || (m.platform && ks.key == "c")
            {
                self.copy_terminal_screen(None, cx);
                return;
            }
        }

        let Some(term) = self.terminals.get_mut(active_ix).and_then(|tab| tab.term.as_mut()) else {
            return;
        };

        // Support Shift+PageUp / Shift+PageDown or Shift+Up / Shift+Down for terminal scrollback
        if m.shift {
            let (_, rows) = term.size();
            let page_lines = rows.saturating_sub(2).max(1) as i32;
            match ks.key.as_str() {
                "pageup" => {
                    term.scroll(page_lines);
                    cx.notify();
                    return;
                }
                "pagedown" => {
                    term.scroll(-page_lines);
                    cx.notify();
                    return;
                }
                "up" => {
                    term.scroll(1);
                    cx.notify();
                    return;
                }
                "down" => {
                    term.scroll(-1);
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }

        let key_lower = ks.key.to_lowercase();
        let bytes: Vec<u8> = match key_lower.as_str() {
            "enter" | "return" => vec![b'\r'],
            "backspace" => {
                if cfg!(windows) {
                    vec![0x08]
                } else {
                    vec![0x7f]
                }
            }
            "tab" => vec![b'\t'],
            "escape" => vec![0x1b],
            "space" | " " => vec![b' '],
            "up" | "arrowup" => vec![0x1b, b'[', b'A'],
            "down" | "arrowdown" => vec![0x1b, b'[', b'B'],
            "right" | "arrowright" => vec![0x1b, b'[', b'C'],
            "left" | "arrowleft" => vec![0x1b, b'[', b'D'],
            "home" => vec![0x1b, b'[', b'H'],
            "end" => vec![0x1b, b'[', b'F'],
            "delete" => vec![0x1b, b'[', b'3', b'~'],
            _ => {
                if m.control && ks.key.len() == 1 {
                    // Ctrl+letter → its control byte (Ctrl-C = 0x03, Ctrl-D = 0x04…).
                    let c = ks.key.as_bytes()[0].to_ascii_lowercase();
                    if c.is_ascii_lowercase() {
                        vec![c - b'a' + 1]
                    } else {
                        return;
                    }
                } else if !m.control && !m.alt {
                    // A printable key: prefer the platform-resolved character
                    // (handles Shift and dead keys), else the single-char key.
                    if let Some(ch) = &ks.key_char {
                        ch.clone().into_bytes()
                    } else if ks.key.chars().count() == 1 {
                        ks.key.clone().into_bytes()
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        };
        // Typing dismisses any mouse selection and resets scroll to bottom, as a real terminal does.
        term.selection_clear();
        term.scroll_to_bottom();
        term.send_input(&bytes);
        cx.notify();
    }

    /// Take and clear any active mouse selection from the active terminal (or any open terminal tab).
    pub(super) fn take_terminal_selection_text(&mut self) -> Option<String> {
        if let Some(term) = self.active_terminal() {
            if let Some(text) = term.selection_text() {
                term.selection_clear();
                return Some(text);
            }
        }
        for tab in &self.terminals {
            if let Some(term) = &tab.term {
                if let Some(text) = term.selection_text() {
                    term.selection_clear();
                    return Some(text);
                }
            }
        }
        None
    }

    /// Copy the active terminal mouse selection to both system and GPUI clipboards,
    /// clear the highlight, and display a confirmation toast. Returns `true` if a
    /// terminal selection was present and copied.
    pub(super) fn copy_terminal_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(selected) = self.take_terminal_selection_text() {
            crate::sys::copy_to_system_clipboard(&selected);
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(selected));
            self.toast_manager.push(
                ToastKind::Success,
                "Copied to clipboard".to_string(),
                Some(2),
            );
            cx.notify();
            true
        } else {
            false
        }
    }

    /// Copy the visible screen lines of the specified terminal tab (or the active terminal)
    /// to both system and GPUI clipboards and display a confirmation toast.
    pub(super) fn copy_terminal_screen(
        &mut self,
        tab_ix: Option<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        let idx = tab_ix.unwrap_or(self.active_terminal_index);
        let Some(term) = self.terminals.get(idx).and_then(|t| t.term.as_ref()) else {
            return false;
        };
        let text = term.snapshot().trimmed_screen_text();
        if text.is_empty() {
            return false;
        }
        crate::sys::copy_to_system_clipboard(&text);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.toast_manager.push(
            ToastKind::Success,
            "Copied terminal screen to clipboard".to_string(),
            Some(2),
        );
        cx.notify();
        true
    }

    /// Copy the terminal selection if any exists, otherwise copy the visible terminal screen.
    pub(super) fn copy_terminal_selection_or_screen(
        &mut self,
        tab_ix: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if !self.copy_terminal_selection(cx) {
            self.copy_terminal_screen(tab_ix, cx);
        }
    }

    /// Paste the system/GPUI clipboard text into the specified terminal tab (or active terminal).
    pub(super) fn paste_to_terminal(&mut self, tab_ix: Option<usize>, cx: &mut Context<Self>) {
        let idx = tab_ix.unwrap_or(self.active_terminal_index);
        if let Some(clipboard) = cx.read_from_clipboard() {
            if let Some(text) = clipboard.text() {
                if let Some(term) = self.terminals.get_mut(idx).and_then(|t| t.term.as_mut()) {
                    term.selection_clear();
                    term.scroll_to_bottom();
                    term.send_input(text.as_bytes());
                    cx.notify();
                }
            }
        }
    }

    /// Unified handler for `gpui_component::input::Copy` (`Ctrl+C` / `Cmd+C`).
    ///
    /// 1. If a terminal mouse selection is active, copies it and clears the terminal selection.
    /// 2. Otherwise, if the terminal has focus:
    ///    - On macOS (`Cmd+C`), copies the visible terminal screen.
    ///    - On Windows/Linux (`Ctrl+C`), sends `SIGINT` (`0x03`) to the active PTY so a running
    ///      command can be interrupted when nothing is selected.
    /// 3. Otherwise, copies any active `TextView` selection from the chat/inspector panes.
    pub(super) fn on_copy_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.copy_terminal_selection(cx) {
            return;
        }
        if self.terminal_focus.is_focused(window) {
            if cfg!(target_os = "macos") {
                self.copy_terminal_screen(None, cx);
            } else if let Some(term) = self.active_terminal_mut() {
                term.scroll_to_bottom();
                term.send_input(&[0x03]);
                cx.notify();
            }
            return;
        }
        use gpui_component::WindowExt as _;
        let selected = window.selected_text(cx);
        if !selected.is_empty() {
            crate::sys::copy_to_system_clipboard(&selected);
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(selected));
            self.toast_manager.push(
                ToastKind::Success,
                "Copied to clipboard".to_string(),
                Some(2),
            );
            cx.notify();
        }
    }

    /// Handler for `TerminalCopy` (`Ctrl+Shift+C`).
    /// Copies the terminal mouse selection if present, else any chat `TextView` selection,
    /// else falls back to copying the visible terminal screen.
    pub(super) fn on_terminal_copy_shortcut(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.copy_terminal_selection(cx) {
            return;
        }
        if !self.terminal_focus.is_focused(window) {
            use gpui_component::WindowExt as _;
            let selected = window.selected_text(cx);
            if !selected.is_empty() {
                crate::sys::copy_to_system_clipboard(&selected);
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(selected));
                self.toast_manager.push(
                    ToastKind::Success,
                    "Copied to clipboard".to_string(),
                    Some(2),
                );
                cx.notify();
                return;
            }
        }
        self.copy_terminal_screen(None, cx);
    }

    /// Map a pointer position (window pixels) to a terminal grid cell, as
    /// `(row, col, right_half)`. Uses the grid's painted origin (from the
    /// `size_probe` canvas) and the fixed monospace cell metrics; the cell is
    /// clamped into the grid by the PTY, so an out-of-bounds drag selects to the
    /// nearest edge. `None` before the first frame has measured the screen.
    pub(super) fn terminal_cell_at(
        &self,
        pos: gpui::Point<gpui::Pixels>,
    ) -> Option<(usize, usize, bool)> {
        let bounds = self.terminal_px.get()?;
        Some(terminal_grid_cell(
            (f32::from(pos.x), f32::from(pos.y)),
            bounds,
        ))
    }
}

/// Pure coordinate mapping from window pointer pixel `(px_x, px_y)` and painted terminal
/// screen bounds `(ox, oy, w, h)` to viewport grid cell `(row, col, right_half)`.
/// Subtracts `TERM_PAD` (12px for `.p_3()`) so pointer coordinates align with the padded text rows.
pub(super) fn terminal_grid_cell(
    (px_x, px_y): (f32, f32),
    (ox, oy, _w, _h): (f32, f32, f32, f32),
) -> (usize, usize, bool) {
    let x = (px_x - ox - TERM_PAD).max(0.0);
    let y = (px_y - oy - TERM_PAD).max(0.0);
    let col_f = x / TERM_CELL_W;
    ((y / TERM_CELL_H) as usize, col_f as usize, col_f.fract() >= 0.5)
}

