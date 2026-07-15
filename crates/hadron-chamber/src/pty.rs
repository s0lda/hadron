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
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{point_to_viewport, Config, Term};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

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
    pub cols: usize,
    pub rows: usize,
}

impl TermSnapshot {
    /// The visible text with styling stripped — the anchor a headless test
    /// asserts on (proves bytes reached the grid, not just that code compiled).
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
    term: Arc<Mutex<Term<VoidListener>>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    _child: Box<dyn Child + Send + Sync>,
    dirty: Arc<AtomicBool>,
    cols: usize,
    rows: usize,
}

impl PtyTerminal {
    /// Spawn `$SHELL` on a fresh PTY sized `cols × rows`, rooted at `cwd`.
    pub fn new(cwd: &Path, cols: usize, rows: usize) -> Result<Self, String> {
        let cols = cols.max(1);
        let rows = rows.max(1);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("openpty failed: {e}"))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
        let mut cmd = CommandBuilder::new(shell);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn shell failed: {e}"))?;
        drop(pair.slave); // parent keeps only the master end

        let writer = pair
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
        // Ends when the shell exits and the master reader returns EOF.
        let term_r = Arc::clone(&term);
        let dirty_r = Arc::clone(&dirty);
        std::thread::Builder::new()
            .name("hadron-pty-reader".into())
            .spawn(move || {
                let mut parser: Processor = Processor::new();
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
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

        Ok(Self {
            term,
            writer,
            master: pair.master,
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

    /// Build a render-ready snapshot of the visible screen: each row coalesced
    /// into runs of identical colour, with the block cursor rendered as an
    /// inverted cell.
    pub fn snapshot(&self) -> TermSnapshot {
        let Ok(term) = self.term.lock() else {
            return TermSnapshot::default();
        };
        let cols = self.cols;
        let rows = self.rows;
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        let cursor = if content.cursor.shape == CursorShape::Hidden {
            None
        } else {
            point_to_viewport(display_offset, content.cursor.point)
        };

        // Gather every visible cell into a dense grid first (the iterator yields
        // in row-major order, but mapping through the viewport keeps us honest
        // about scrollback offset).
        let mut grid: Vec<Vec<(char, (u8, u8, u8), (u8, u8, u8))>> =
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
            if cursor.is_some_and(|c| c.line == vp.line && c.column == vp.column) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let ch = if cell.c == '\0' { ' ' } else { cell.c };
            grid[vp.line].push((ch, fg, bg));
        }

        let lines = grid
            .into_iter()
            .map(|row| {
                let mut runs: Vec<TermRun> = Vec::new();
                for (ch, fg, bg) in row {
                    match runs.last_mut() {
                        Some(run) if run.fg == fg && run.bg == bg => run.text.push(ch),
                        _ => runs.push(TermRun {
                            text: ch.to_string(),
                            fg,
                            bg,
                        }),
                    }
                }
                TermLine { runs }
            })
            .collect();

        TermSnapshot { lines, cols, rows }
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

    #[test]
    fn indexed_palette_matches_xterm() {
        assert_eq!(indexed_to_rgb(0), ANSI16[0]);
        assert_eq!(indexed_to_rgb(15), ANSI16[15]);
        assert_eq!(indexed_to_rgb(16), (0, 0, 0)); // cube origin
        assert_eq!(indexed_to_rgb(231), (255, 255, 255)); // cube max
        assert_eq!(indexed_to_rgb(232), (8, 8, 8)); // first grey
    }
}
