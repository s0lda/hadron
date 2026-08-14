use super::*;

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
                    let shell = crate::pty::default_shell();
                    let stem = std::path::Path::new(&shell)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("term");
                    let title = format!("{stem} #1");
                    println!("[hadron-terminal] Lazily initializing first terminal tab '{title}' in '{}' (dims: {}x{})...", root.display(), cols, rows);
                    match crate::pty::PtyTerminal::new(&root, cols, rows) {
                        Ok(mut term) => {
                            println!("[hadron-terminal] Successfully created terminal tab '{title}'");
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
                            eprintln!("[hadron-terminal] ERROR creating terminal tab '{title}': {err}");
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
        let Some(term) = self.terminals.get_mut(active_ix).and_then(|tab| tab.term.as_mut()) else {
            return;
        };
        let ks = &event.keystroke;
        let m = &ks.modifiers;

        let is_paste = (m.control && ks.key == "v")
            || (m.control && m.shift && (ks.key == "v" || ks.key == "V"))
            || (m.platform && ks.key == "v");
        if is_paste {
            if let Some(clipboard) = cx.read_from_clipboard() {
                if let Some(text) = clipboard.text() {
                    term.scroll_to_bottom();
                    term.send_input(text.as_bytes());
                    cx.notify();
                }
            }
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
            if let Some(selected) = term.selection_text() {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(selected));
                return;
            }
            // If no text is selected, fallback to copying the entire screen
            // ONLY if they used the explicit full-copy shortcut Ctrl+Shift+C / Cmd+C.
            // If they just pressed bare Ctrl+C, let it fall through so they can send SIGINT!
            if (m.control && m.shift && (ks.key == "c" || ks.key == "C"))
                || (m.platform && ks.key == "c")
            {
                let snap = term.snapshot();
                let mut text = String::new();
                for line in &snap.lines {
                    let mut line_text = String::new();
                    for run in &line.runs {
                        line_text.push_str(&run.text);
                    }
                    text.push_str(line_text.trim_end());
                    text.push('\n');
                }
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                return;
            }
        }

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

    /// Map a pointer position (window pixels) to a terminal grid cell, as
    /// `(row, col, right_half)`. Uses the grid's painted origin (from the
    /// `size_probe` canvas) and the fixed monospace cell metrics; the cell is
    /// clamped into the grid by the PTY, so an out-of-bounds drag selects to the
    /// nearest edge. `None` before the first frame has measured the screen.
    pub(super) fn terminal_cell_at(
        &self,
        pos: gpui::Point<gpui::Pixels>,
    ) -> Option<(usize, usize, bool)> {
        let (ox, oy, _, _) = self.terminal_px.get()?;
        let x = (f32::from(pos.x) - ox).max(0.0);
        let y = (f32::from(pos.y) - oy).max(0.0);
        let col_f = x / TERM_CELL_W;
        Some(((y / TERM_CELL_H) as usize, col_f as usize, col_f.fract() >= 0.5))
    }
}
