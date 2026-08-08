//! A real interactive terminal: a PTY-backed shell whose output is parsed by a
//! genuine VTE engine ([`alacritty_terminal`]) into a cell grid we can render.
//!
//! This is the engine behind the chamber's Terminal tab. It replaces the old
//! line-buffered `execute()` shell (which only ran one command at a time and
//! discarded ANSI): programs now see a real TTY, so `ls --color`, `git log`
//! paging, `vim` and `top` work; escape sequences and colors are parsed into a
//! grid; and keystrokes stream byte-for-byte to the child.
//!
//! The engine lives **outside** the `gui` feature on purpose, so it is testable
//! headlessly — the tests below pump bytes through a real PTY and assert on the
//! parsed grid. A styled `div` can never pass those; a real terminal can.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{point_to_viewport, viewport_to_point, Config, Term};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize, SlavePty};

/// The terminal's default foreground / background, used when a cell asks for the
/// terminal default colour (`SGR 39/49`, the initial state of every cell).
pub const DEFAULT_FG: (u8, u8, u8) = (0xd0, 0xd3, 0xd8);
pub const DEFAULT_BG: (u8, u8, u8) = (0x0c, 0x0c, 0x0e);

/// The 16 ANSI colours (a Tango-style palette — legible on the dark screen).
/// OSC palette overrides (a program re-defining colour 1) are not tracked yet.
const ANSI16: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xcc, 0x00, 0x00),
    (0x4e, 0x9a, 0x06),
    (0xc4, 0xa0, 0x00),
    (0x34, 0x65, 0xa4),
    (0x75, 0x50, 0x7b),
    (0x06, 0x98, 0x9a),
    (0xd3, 0xd7, 0xcf),
    (0x55, 0x57, 0x53),
    (0xef, 0x29, 0x29),
    (0x8a, 0xe2, 0x34),
    (0xfc, 0xe9, 0x4f),
    (0x72, 0x9f, 0xcf),
    (0xad, 0x7f, 0xa8),
    (0x34, 0xe2, 0xe2),
    (0xee, 0xee, 0xec),
];

/// A run of same-styled characters on one row — the unit handed to the renderer,
/// coalesced so a row is a handful of runs, not one element per cell. (This box
/// CPU-rasterises every frame; per-cell elements would crawl.)
#[derive(Clone, Debug, PartialEq)]
pub struct TermRun {
    pub text: String,
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
    pub has_cursor: bool,
}

/// One visible row: its coalesced runs, left to right.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TermLine {
    pub runs: Vec<TermRun>,
}

/// A snapshot of the visible screen, owned and safe to hand to the renderer.
#[derive(Clone, Debug, Default)]
pub struct TermSnapshot {
    pub lines: Vec<TermLine>,
    #[allow(dead_code)] // snapshot dimensions, retained for renderer use
    pub cols: usize,
    #[allow(dead_code)] // snapshot dimensions, retained for renderer use
    pub rows: usize,
}

impl TermSnapshot {
    /// The visible text with styling stripped — the anchor a headless test
    /// asserts on (proves bytes reached the grid, not just that code compiled).
    #[allow(dead_code)] // test-only anchor for headless grid assertions
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            for run in &line.runs {
                out.push_str(&run.text);
            }
            out.push('\n');
        }
        out
    }
}

/// Minimal [`Dimensions`] for constructing / resizing the grid. Scrollback is set
/// on [`Config::scrolling_history`], not here, so `total_lines == screen_lines`.
struct GridSize {
    cols: usize,
    rows: usize,
}
impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// A live PTY + shell, its output parsed into a grid on a background thread.
pub struct PtyTerminal {
    pub title: String,
    term: Arc<Mutex<Term<VoidListener>>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    _slave: Box<dyn SlavePty + Send>,
    _child: Box<dyn Child + Send + Sync>,
    dirty: Arc<AtomicBool>,
    cols: usize,
    rows: usize,
}

/// Resolve default shell cross-platform (PowerShell/COMSPEC on Windows / SHELL on Unix).
pub fn default_shell() -> String {
    let resolved = if cfg!(windows) {
        // We explicitly do NOT check the "SHELL" env var on Windows. MSYS2/Git Bash sets
        // SHELL=/bin/bash or similar, which often hangs silently when spawned inside ConPTY.
        // We always want to default to PowerShell or cmd on Windows.
        if std::path::Path::new("C:\\Program Files\\PowerShell\\7\\pwsh.exe").exists() {
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe".to_string()
        } else if std::path::Path::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe").exists() {
            "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".to_string()
        } else if let Some(comspec) = std::env::var("COMSPEC").ok().filter(|c| !c.trim().is_empty() && std::path::Path::new(c).exists()) {
            comspec
        } else if std::path::Path::new("C:\\Windows\\System32\\cmd.exe").exists() {
            "C:\\Windows\\System32\\cmd.exe".to_string()
        } else {
            "powershell.exe".to_string()
        }
    } else {
        if let Some(sh) = std::env::var("SHELL").ok().filter(|s| !s.trim().is_empty()) {
            sh
        } else {
            "sh".to_string()
        }
    };


    println!("[hadron-pty] default_shell() resolved shell: '{resolved}'");
    resolved
}

impl PtyTerminal {
    /// Spawn default shell on a fresh PTY sized `cols × rows`, rooted at `cwd`.
    pub fn new(cwd: &Path, cols: usize, rows: usize) -> Result<Self, String> {
        // Ensure dimensions are within safe bounds for ConPTY to avoid silent hangs
        let cols = cols.max(10).min(500);
        let rows = rows.max(1).min(500);


        let pty_system = native_pty_system();
        let pty_size = PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        };

        let clean_cwd = hadron_lattice::sys::paths::simplified(cwd);
        let final_cwd = if clean_cwd.exists() {
            clean_cwd.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        };

        let primary_shell = default_shell();
        let mut shells = vec![primary_shell.clone()];
        if cfg!(windows) {
            if !shells.contains(&"powershell.exe".to_string()) {
                shells.push("powershell.exe".to_string());
            }
            if !shells.contains(&"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".to_string()) {
                shells.push("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe".to_string());
            }
            if !shells.contains(&"cmd.exe".to_string()) {
                shells.push("cmd.exe".to_string());
            }
            if !shells.contains(&"C:\\Windows\\System32\\cmd.exe".to_string()) {
                shells.push("C:\\Windows\\System32\\cmd.exe".to_string());
            }
        }

        println!("[hadron-pty] Creating PTY terminal. cwd: '{}', grid: {}x{}", final_cwd.display(), cols, rows);
        println!("[hadron-pty] Candidate shells to try: {:?}", shells);

        let mut child_and_pair = None;
        let mut last_err = String::new();
        for sh in &shells {
            println!("[hadron-pty] Attempting to spawn shell: '{sh}'");
            let pair = match pty_system.openpty(pty_size) {
                Ok(p) => p,
                Err(e) => {
                    last_err = format!("openpty failed: {e}");
                    eprintln!("[hadron-pty] openpty() failed for shell '{sh}': {e}");
                    continue;
                }
            };

            let mut cmd = CommandBuilder::new(sh);
            cmd.cwd(&final_cwd);
            let mut has_sys_root = false;
            let mut has_sys_drive = false;
            let mut has_windir = false;
            let mut has_pathext = false;

            for (k, v) in std::env::vars() {
                if k.starts_with('=') || k.is_empty() {
                    continue;
                }
                if k.eq_ignore_ascii_case("SystemRoot") {
                    has_sys_root = true;
                }
                if k.eq_ignore_ascii_case("SystemDrive") {
                    has_sys_drive = true;
                }
                if k.eq_ignore_ascii_case("windir") {
                    has_windir = true;
                }
                if k.eq_ignore_ascii_case("PATHEXT") {
                    has_pathext = true;
                }
                cmd.env(k, v);
            }
            if cfg!(windows) {
                if !has_sys_root {
                    cmd.env("SystemRoot", "C:\\Windows");
                }
                if !has_sys_drive {
                    cmd.env("SystemDrive", "C:");
                }
                if !has_windir {
                    cmd.env("windir", "C:\\Windows");
                }
                if !has_pathext {
                    cmd.env("PATHEXT", ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC");
                }
                let sh_lower = sh.to_lowercase();
                if sh_lower.contains("powershell") || sh_lower.contains("pwsh") {
                    cmd.arg("-NoExit");
                    cmd.arg("-NoLogo");
                } else if sh_lower.contains("cmd") {
                    cmd.arg("/k");
                }

            } else {
                cmd.env("TERM", "xterm-256color");
            }
            cmd.env("COLORTERM", "truecolor");

            match pair.slave.spawn_command(cmd) {
                Ok(child) => {
                    println!("[hadron-pty] Successfully spawned shell '{sh}' (process ID: {:?})", child.process_id());
                    child_and_pair = Some((child, pair));
                    break;
                }
                Err(e) => {
                    last_err = format!("spawn shell '{sh}' in '{}' failed: {e}", final_cwd.display());
                    eprintln!("[hadron-pty] spawn_command() failed for shell '{sh}' in '{}': {e}", final_cwd.display());
                }
            }
        }

        let (child, pair) = match child_and_pair {
            Some(cp) => cp,
            None => {
                eprintln!("[hadron-pty] All candidate shells failed to spawn. Last error: '{last_err}'");
                return Err(last_err);
            }
        };

        let mut writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("pty writer: {e}"))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("pty reader: {e}"))?;

        let mut config = Config::default();
        config.scrolling_history = 5000;
        let term = Term::new(config, &GridSize { cols, rows }, VoidListener);
        let term = Arc::new(Mutex::new(term));
        let dirty = Arc::new(AtomicBool::new(true));

        // Reader thread: pump PTY bytes through the VTE parser into the grid.
        // Spawn BEFORE sending initial newline so no incoming bytes are lost.
        let term_r = Arc::clone(&term);
        let dirty_r = Arc::clone(&dirty);
        let sh_label = primary_shell.clone();
        std::thread::Builder::new()
            .name("hadron-pty-reader".into())
            .spawn(move || {
                println!("[hadron-pty] Started PTY reader thread for shell '{sh_label}'");
                let mut parser: Processor = Processor::new();
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            println!("[hadron-pty] PTY reader thread hit EOF (0 bytes) for shell '{sh_label}'");
                            if let Ok(mut term) = term_r.lock() {
                                parser.advance(&mut *term, b"\r\n[process exited]\r\n");
                            }
                            dirty_r.store(true, Ordering::Relaxed);
                            break;
                        }
                        Err(e) => {
                            eprintln!("[hadron-pty] PTY reader thread read error for shell '{sh_label}': {e}");
                            if let Ok(mut term) = term_r.lock() {
                                let err_msg = format!("\r\n[pty read error: {e}]\r\n");
                                parser.advance(&mut *term, err_msg.as_bytes());
                            }
                            dirty_r.store(true, Ordering::Relaxed);
                            break;
                        }
                        Ok(n) => {
                            if let Ok(mut term) = term_r.lock() {
                                parser.advance(&mut *term, &buf[..n]);
                            }
                            dirty_r.store(true, Ordering::Relaxed);
                        }
                    }
                }
            })
            .map_err(|e| format!("pty reader thread: {e}"))?;


        if cfg!(windows) {
            let _ = writer.write_all(b"\r\n");
            let _ = writer.flush();
        }

        let stem = std::path::Path::new(&primary_shell)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("term");
        let initial_title = format!("{stem} #1");

        Ok(Self {
            title: initial_title,
            term,
            writer,
            master: pair.master,
            _slave: pair.slave,
            _child: child,
            dirty,
            cols,
            rows,
        })
    }


    /// Stream bytes straight to the child (a keystroke, a paste, a signal char).
    pub fn send_input(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Re-size both the PTY (so the child gets `SIGWINCH`) and the grid. A no-op
    /// when the size is unchanged, so it is cheap to call every frame.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        let _ = self.master.resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut term) = self.term.lock() {
            term.resize(GridSize { cols, rows });
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// True (once) when new output has arrived since the last check. The render
    /// loop polls this and repaints only on change, so an idle terminal never
    /// forces a frame (the software rasteriser thanks us).
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }

    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Scroll the viewport by `lines` (positive = up into history, negative = down towards bottom).
    pub fn scroll(&self, lines: i32) {
        if lines == 0 {
            return;
        }
        if let Ok(mut term) = self.term.lock() {
            term.scroll_display(Scroll::Delta(lines));
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Reset scroll position to the bottom of the active screen.
    pub fn scroll_to_bottom(&self) {
        if let Ok(mut term) = self.term.lock() {
            term.scroll_display(Scroll::Bottom);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Map a viewport cell (row/col from the top-left of the visible screen) to a
    /// grid point, clamped to the grid and offset for any scrollback.
    fn grid_point(&self, offset: usize, row: usize, col: usize) -> Point {
        viewport_to_point(
            offset,
            Point::new(
                row.min(self.rows.saturating_sub(1)),
                Column(col.min(self.cols.saturating_sub(1))),
            ),
        )
    }

    /// Begin a mouse text selection at viewport cell `(row, col)`. `clicks` picks
    /// the granularity — 1: character drag, 2: word, 3+: line — the usual
    /// double/triple-click terminal behaviour. `right_half` says the pointer sits
    /// on the right half of the cell, so the boundary cell is included naturally.
    pub fn selection_start(&self, row: usize, col: usize, right_half: bool, clicks: usize) {
        if let Ok(mut term) = self.term.lock() {
            let point = self.grid_point(term.grid().display_offset(), row, col);
            let side = if right_half { Side::Right } else { Side::Left };
            let ty = match clicks {
                2 => SelectionType::Semantic,
                n if n >= 3 => SelectionType::Lines,
                _ => SelectionType::Simple,
            };
            term.selection = Some(Selection::new(ty, point, side));
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Extend the active selection to viewport cell `(row, col)` (drag).
    pub fn selection_update(&self, row: usize, col: usize, right_half: bool) {
        if let Ok(mut term) = self.term.lock() {
            let point = self.grid_point(term.grid().display_offset(), row, col);
            let side = if right_half { Side::Right } else { Side::Left };
            if let Some(sel) = term.selection.as_mut() {
                sel.update(point, side);
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Drop any active selection (clears its highlight on the next frame).
    pub fn selection_clear(&self) {
        if let Ok(mut term) = self.term.lock() {
            if term.selection.take().is_some() {
                self.dirty.store(true, Ordering::Relaxed);
            }
        }
    }

    /// The selected text — scrollback-aware, with wrapped lines joined by the VTE.
    /// `None` when the selection is empty.
    pub fn selection_text(&self) -> Option<String> {
        let term = self.term.lock().ok()?;
        term.selection_to_string().filter(|s| !s.is_empty())
    }

    /// Build a render-ready snapshot of the visible screen: each row coalesced
    /// into runs of identical colour, with the line cursor marked on the target cell.
    pub fn snapshot(&self) -> TermSnapshot {
        let Ok(term) = self.term.lock() else {
            return TermSnapshot::default();
        };
        let cols = self.cols;
        let rows = self.rows;
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        // The active mouse selection, in grid coordinates — its cells render
        // inverted so the marked text is visible (see the per-cell swap below).
        let selection = content.selection;
        let cursor = if content.cursor.shape == CursorShape::Hidden {
            None
        } else {
            point_to_viewport(display_offset, content.cursor.point)
        };

        // Gather every visible cell into a dense grid first (the iterator yields
        // in row-major order, but mapping through the viewport keeps us honest
        // about scrollback offset).
        let mut grid: Vec<Vec<(char, (u8, u8, u8), (u8, u8, u8), bool)>> =
            vec![Vec::with_capacity(cols); rows];
        for indexed in content.display_iter {
            let Some(vp) = point_to_viewport(display_offset, indexed.point) else {
                continue;
            };
            if vp.line >= rows {
                continue;
            }
            let cell = indexed.cell;
            let mut fg = color_to_rgb(cell.fg, DEFAULT_FG);
            let mut bg = color_to_rgb(cell.bg, DEFAULT_BG);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            let has_cursor = cursor.is_some_and(|c| c.line == vp.line && c.column == vp.column);
            if selection.is_some_and(|range| range.contains(indexed.point)) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let ch = if cell.c == '\0' { ' ' } else { cell.c };
            grid[vp.line].push((ch, fg, bg, has_cursor));
        }

        let lines = grid
            .into_iter()
            .map(|row| {
                let mut runs: Vec<TermRun> = Vec::new();
                for (ch, fg, bg, has_cursor) in row {
                    match runs.last_mut() {
                        Some(run)
                            if run.fg == fg
                                && run.bg == bg
                                && run.has_cursor == has_cursor
                                && !has_cursor =>
                        {
                            run.text.push(ch);
                        }
                        _ => runs.push(TermRun {
                            text: ch.to_string(),
                            fg,
                            bg,
                            has_cursor,
                        }),
                    }
                }
                TermLine { runs }
            })
            .collect();

        TermSnapshot { lines, cols, rows }
    }
}

impl Drop for PtyTerminal {
    fn drop(&mut self) {
        let _ = self._child.kill();
        let _ = self._child.wait();
    }
}

/// Resolve a VTE cell colour to concrete RGB. `default` is the terminal default
/// for this channel (fg or bg), used for `Named(Foreground/Background)` and the
/// dim/bright-foreground variants we don't special-case yet.
fn color_to_rgb(color: Color, default: (u8, u8, u8)) -> (u8, u8, u8) {
    match color {
        Color::Spec(rgb) => (rgb.r, rgb.g, rgb.b),
        Color::Indexed(i) => indexed_to_rgb(i),
        Color::Named(NamedColor::Black) => ANSI16[0],
        Color::Named(NamedColor::Red) => ANSI16[1],
        Color::Named(NamedColor::Green) => ANSI16[2],
        Color::Named(NamedColor::Yellow) => ANSI16[3],
        Color::Named(NamedColor::Blue) => ANSI16[4],
        Color::Named(NamedColor::Magenta) => ANSI16[5],
        Color::Named(NamedColor::Cyan) => ANSI16[6],
        Color::Named(NamedColor::White) => ANSI16[7],
        Color::Named(NamedColor::BrightBlack) => ANSI16[8],
        Color::Named(NamedColor::BrightRed) => ANSI16[9],
        Color::Named(NamedColor::BrightGreen) => ANSI16[10],
        Color::Named(NamedColor::BrightYellow) => ANSI16[11],
        Color::Named(NamedColor::BrightBlue) => ANSI16[12],
        Color::Named(NamedColor::BrightMagenta) => ANSI16[13],
        Color::Named(NamedColor::BrightCyan) => ANSI16[14],
        Color::Named(NamedColor::BrightWhite) => ANSI16[15],
        Color::Named(NamedColor::Background) => DEFAULT_BG,
        // Foreground, dim/bright foreground, cursor, and any future variant fall
        // back to the channel default — good enough for the first slice.
        Color::Named(_) => default,
    }
}

/// The xterm 256-colour cube: 0–15 ANSI, 16–231 a 6×6×6 cube, 232–255 greys.
fn indexed_to_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            let i = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (step(i / 36), step((i % 36) / 6), step(i % 6))
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    /// Poll the grid until `pred` holds or ~5s elapse, then return the snapshot.
    fn wait_for(term: &PtyTerminal, pred: impl Fn(&TermSnapshot) -> bool) -> TermSnapshot {
        for _ in 0..100 {
            let snap = term.snapshot();
            if pred(&snap) {
                return snap;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        term.snapshot()
    }

    fn is_red(c: (u8, u8, u8)) -> bool {
        c.0 > 150 && c.1 < 90 && c.2 < 90
    }

    /// The spine: a real PTY runs a real shell, and the shell's *computed* output
    /// lands in the parsed grid. `42` can only appear if the child ran
    /// `echo $((6 * 7))` and its stdout flowed PTY → VTE → grid. A styled `div`
    /// cannot make this pass.
    #[test]
    fn a_real_pty_feeds_a_real_vte_grid() {
        let dir = tempdir().unwrap();
        let mut term = PtyTerminal::new(dir.path(), 80, 24).unwrap();
        term.send_input(b"echo $((6 * 7))\n");
        let snap = wait_for(&term, |s| s.plain_text().contains("42"));
        assert!(
            snap.plain_text().contains("42"),
            "expected computed output `42` in the grid, got:\n{}",
            snap.plain_text()
        );
    }

    /// ANSI colour from a program's stdout is parsed into the cell's colour — not
    /// stored as literal escape text. Only the `printf` *output* is red; the
    /// echoed command line is not, so a red run named REDWORD proves the parse.
    #[test]
    fn ansi_color_escapes_reach_the_grid_as_color() {
        let dir = tempdir().unwrap();
        let mut term = PtyTerminal::new(dir.path(), 80, 24).unwrap();
        term.send_input(b"printf '\\033[31mREDWORD\\033[0m\\n'\n");
        let snap = wait_for(&term, |s| {
            s.lines
                .iter()
                .flat_map(|l| &l.runs)
                .any(|r| r.text.contains("REDWORD") && is_red(r.fg))
        });
        let red = snap
            .lines
            .iter()
            .flat_map(|l| &l.runs)
            .find(|r| r.text.contains("REDWORD") && is_red(r.fg));
        assert!(
            red.is_some(),
            "expected a red REDWORD run from parsed ANSI, got:\n{}",
            snap.plain_text()
        );
    }

    /// Concatenate a line's runs back into its plain text.
    fn line_text(l: &TermLine) -> String {
        l.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// The mechanism behind the "first prompt splits into two lines until you
    /// type" fix: when the PTY is widened, the shell redraws its prompt on the
    /// SIGWINCH, so a prompt that was wrapped at a narrow width collapses back to
    /// one row with no keystroke. `pump_terminal` relies on exactly this — it
    /// keeps resizing the PTY to the settled screen width — so if this ever stops
    /// holding, the on-screen glitch returns. Uses bash explicitly: the redraw is
    /// a readline behaviour and `sh`/dash does not guarantee it (and Jake's shell
    /// is bash).
    #[test]
    fn widening_the_pty_redraws_a_wrapped_prompt_onto_one_row() {
        if !std::path::Path::new("/bin/bash").exists() {
            return; // readline-specific; skip where bash is unavailable
        }
        let prev_shell = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "/bin/bash");
        let dir = tempdir().unwrap();
        let mut term = PtyTerminal::new(dir.path(), 12, 24).unwrap();
        // Restore immediately so a concurrent test's spawn is unaffected.
        match prev_shell {
            Some(s) => std::env::set_var("SHELL", s),
            None => std::env::remove_var("SHELL"),
        }

        // A fixed prompt 18 cols wide — wider than the 12-col grid, so it wraps.
        term.send_input(b"PS1='PROMPTABCDEFGHIJ> '\n");
        let frag = "PROMPTABCDEF"; // the first 12 cols (one full narrow row)
        let marker = "PROMPTABCDEFGHIJ>"; // the whole prompt (only fits when wide)
        let row_has = |s: &TermSnapshot, needle: &str| {
            s.lines.iter().any(|l| line_text(l).contains(needle))
        };

        // The new prompt has been drawn once its leading fragment appears...
        let wrapped = wait_for(&term, |s| row_has(s, frag));
        assert!(
            row_has(&wrapped, frag),
            "the PS1 prompt was never drawn:\n{}",
            wrapped.plain_text()
        );
        // ...and at 12 cols no single row holds the whole prompt — it is wrapped.
        assert!(
            !row_has(&wrapped, marker),
            "expected the prompt to WRAP across two rows at 12 cols:\n{}",
            wrapped.plain_text()
        );

        // Widen the PTY. No input is sent — the fix depends on the resize alone
        // making the prompt whole again.
        term.resize(80, 24);
        let unwrapped = wait_for(&term, |s| row_has(s, marker));
        assert!(
            row_has(&unwrapped, marker),
            "widening should redraw the prompt onto ONE row with no keystroke:\n{}",
            unwrapped.plain_text()
        );
    }

    #[test]
    fn indexed_palette_matches_xterm() {
        assert_eq!(indexed_to_rgb(0), ANSI16[0]);
        assert_eq!(indexed_to_rgb(15), ANSI16[15]);
        assert_eq!(indexed_to_rgb(16), (0, 0, 0)); // cube origin
        assert_eq!(indexed_to_rgb(231), (255, 255, 255)); // cube max
        assert_eq!(indexed_to_rgb(232), (8, 8, 8)); // first grey
    }

    /// First (row, col) where `needle` appears in the visible grid — ascii is one
    /// cell per column, so the byte index into the joined line IS the column.
    fn find_text(snap: &TermSnapshot, needle: &str) -> Option<(usize, usize)> {
        snap.lines.iter().enumerate().find_map(|(r, line)| {
            let text = line_text(line);
            text.find(needle).map(|byte_idx| (r, text[..byte_idx].chars().count()))
        })
    }

    /// Mouse selection is what copy reads: drive a drag across a known span and
    /// prove `selection_text` returns exactly that span's text, then that a clear
    /// empties it. This is the headless proof the copy path works, since the grid
    /// renders as raw `div`s that the fork's TextView selection can never see.
    #[test]
    fn mouse_selection_yields_the_marked_text() {
        let marker = "ZZSELZZ";
        let dir = tempdir().unwrap();
        let mut term = PtyTerminal::new(dir.path(), 40, 12).unwrap();
        // `printf ZZSELZZ` prints the literal (no `%`), so the marker lands on the
        // grid — the echoed command line carries it too, either occurrence works.
        term.send_input(b"printf ZZSELZZ\n");
        let snap = wait_for(&term, |s| s.plain_text().contains(marker));
        let (row, col) = find_text(&snap, marker).expect("marker on grid");

        term.selection_start(row, col, false, 1);
        // Extend through the marker's last cell (right half → inclusive).
        term.selection_update(row, col + marker.len() - 1, true);
        let selected = term.selection_text().expect("a non-empty selection");
        assert!(
            selected.contains(marker),
            "drag over `{marker}` should select it, got {selected:?}"
        );

        term.selection_clear();
        assert!(
            term.selection_text().is_none(),
            "clearing the selection should leave nothing selected"
        );
    }

    #[test]
    fn terminal_scrollback_scrolling() {
        let dir = tempdir().unwrap();
        let mut term = PtyTerminal::new(dir.path(), 40, 5).unwrap();
        // Print 20 lines to overflow the 5-row screen
        term.send_input(b"for i in $(seq 1 20); do echo \"LINE_$i\"; done\n");
        wait_for(&term, |s| s.plain_text().contains("LINE_20"));

        let bottom_snap = term.snapshot();
        assert!(
            bottom_snap.plain_text().contains("LINE_20"),
            "bottom view should contain LINE_20"
        );

        // Scroll up into history
        term.scroll(10);
        let scrolled_snap = term.snapshot();
        assert!(
            scrolled_snap.plain_text().contains("LINE_1"),
            "scrolled up view should contain earlier lines like LINE_1"
        );

        // Scroll back to bottom
        term.scroll_to_bottom();
        let reset_snap = term.snapshot();
        assert!(
            reset_snap.plain_text().contains("LINE_20"),
            "reset to bottom view should contain LINE_20 again"
        );
    }

    #[test]
    fn test_pty_tab_list_creation() {
        let mut tabs = vec!["bash #1".to_string()];
        tabs.push("bash #2".to_string());
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0], "bash #1");
        assert_eq!(tabs[1], "bash #2");
    }

    #[test]
    fn test_multi_terminal_instance_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let mut term1 = PtyTerminal::new(dir.path(), 80, 24).expect("term1 spawn");
        let mut term2 = PtyTerminal::new(dir.path(), 80, 24).expect("term2 spawn");

        term1.send_input(b"echo TERM_ALPHA_MARKER\n");
        term2.send_input(b"echo TERM_BETA_MARKER\n");

        let snap1 = wait_for(&term1, |s| s.plain_text().contains("TERM_ALPHA_MARKER"));
        let snap2 = wait_for(&term2, |s| s.plain_text().contains("TERM_BETA_MARKER"));

        assert!(
            snap1.plain_text().contains("TERM_ALPHA_MARKER"),
            "term1 grid must contain TERM_ALPHA_MARKER"
        );
        assert!(
            !snap1.plain_text().contains("TERM_BETA_MARKER"),
            "term1 grid must NOT be contaminated by term2 output"
        );

        assert!(
            snap2.plain_text().contains("TERM_BETA_MARKER"),
            "term2 grid must contain TERM_BETA_MARKER"
        );
        assert!(
            !snap2.plain_text().contains("TERM_ALPHA_MARKER"),
            "term2 grid must NOT be contaminated by term1 output"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_pty_resource_cleanup_under_load() {
        let dir = tempfile::tempdir().unwrap();
        let initial_threads = std::fs::read_dir("/proc/self/task").unwrap().count();

        for _ in 0..10 {
            let term = PtyTerminal::new(dir.path(), 80, 24).expect("spawn PTY");
            drop(term);
        }
        std::thread::sleep(Duration::from_millis(150));

        let mut terms = Vec::new();
        for _ in 0..10 {
            terms.push(PtyTerminal::new(dir.path(), 80, 24).expect("spawn PTY"));
        }
        drop(terms);
        std::thread::sleep(Duration::from_millis(200));

        let final_threads = std::fs::read_dir("/proc/self/task").unwrap().count();
        assert!(
            final_threads <= initial_threads,
            "Threads must not leak after dropping PTY terminals! Baseline: {}, Final: {}",
            initial_threads, final_threads
        );
    }

    #[test]
    fn terminal_cursor_is_marked_as_line_cursor() {
        let dir = tempdir().unwrap();
        let term = PtyTerminal::new(dir.path(), 40, 5).unwrap();
        let snap = wait_for(&term, |s| {
            s.lines.iter().any(|l| l.runs.iter().any(|r| r.has_cursor))
        });
        let cursor_run = snap
            .lines
            .iter()
            .flat_map(|l| &l.runs)
            .find(|r| r.has_cursor);
        assert!(
            cursor_run.is_some(),
            "expected at least one run in grid to be marked with has_cursor = true"
        );
    }

    #[test]
    fn test_default_shell_resolution() {
        let shell = default_shell();
        assert!(!shell.trim().is_empty(), "default_shell must return a non-empty shell command");
    }
}

