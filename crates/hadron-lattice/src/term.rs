//! The one home for everything Hadron prints to a console.
//!
//! Two processes share the human's terminal — the chamber and the daemon it
//! auto-spawns — and before this module they did it with five different prefix
//! conventions (`hadron-chamber:`, `hadron-gluon:`, `chamber:`, `gluon:`, `[acp]`)
//! invented independently at ~200 `println!`/`eprintln!` sites. That drift is what
//! [`Source`] exists to make impossible: a new call site picks from an enum, and the
//! prefix text has exactly one home ([`Source`]'s `Display`).
//!
//! Colour is decided at **runtime**, per stream: ANSI only when that stream is a
//! terminal and `NO_COLOR` is unset. The decision ([`colour_enabled`]) is deliberately
//! separate from the rendering ([`render`]), so the renderer is a pure function the
//! tests can drive without a TTY.
//!
//! No new dependency: the escape codes are `&'static str` constants and the clock is
//! `chrono`, which this crate already carries.

use std::fmt;
use std::io::{IsTerminal, Write};

use chrono::Local;

/// Which process, and which subsystem of it, is speaking.
///
/// An enum rather than a string so a call site cannot invent a sixth spelling. The
/// `Display` impl below is the ONE place the prefix text lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The chamber GUI process.
    Chamber,
    /// The swarm daemon.
    Gluon,
    /// An ACP agent subprocess speaking through the daemon.
    Acp,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `f.pad`, not `write_str`: a Display impl that writes directly ignores the
        // formatter's width, so `{source:<SOURCE_WIDTH$}` would silently not pad and
        // the message column would wander by source.
        f.pad(match self {
            Source::Chamber => "chamber",
            Source::Gluon => "gluon",
            Source::Acp => "acp",
        })
    }
}

/// The width the source column is padded to, so the message column starts at the same
/// offset on every line. It is the length of the longest [`Source`] spelling; the test
/// `the_source_column_is_wide_enough_for_every_source` fails if a new variant outgrows it.
pub const SOURCE_WIDTH: usize = 7;

/// How loud a line is. Carries **only** styling: a `Warn` and an `Error` line have the
/// same text an `Info` line would, because a level must never reword a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// Which of the process's two streams a line goes to. Existing call sites are converted
/// onto the stream they already chose — that split may be load-bearing for someone's pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

const RESET: &str = "\u{1b}[0m";
const DIM: &str = "\u{1b}[2m";
const BLUE: &str = "\u{1b}[34m";
const YELLOW: &str = "\u{1b}[33m";
const RED: &str = "\u{1b}[31m";

impl Level {
    /// The ANSI sequence this level paints its message with, or `""` for the plain one.
    fn ansi(self) -> &'static str {
        match self {
            Level::Info => "",
            Level::Warn => YELLOW,
            Level::Error => RED,
        }
    }
}

/// Render one console line. Pure: no clock, no environment, no I/O — `stamp` and
/// `colour` are handed in so a test can pin both.
///
/// The shape is `HH:MM:SS  <source>  <message>`, with the source padded to
/// [`SOURCE_WIDTH`] so the message column lines up down the log.
pub fn render(stamp: &str, source: Source, level: Level, message: &str, colour: bool) -> String {
    if !colour {
        return format!("{stamp}  {source:<SOURCE_WIDTH$}  {message}");
    }
    let tint = level.ansi();
    let (open, close) = if tint.is_empty() {
        ("", "")
    } else {
        (tint, RESET)
    };
    format!(
        "{DIM}{stamp}{RESET}  {BLUE}{source:<SOURCE_WIDTH$}{RESET}  {open}{message}{close}"
    )
}

/// May we paint `stream`? A terminal and no `NO_COLOR` in the environment.
///
/// Kept out of [`render`] so the renderer stays testable without a TTY, and consulted
/// per call rather than cached: a stream can be redirected between calls, and the
/// syscall is far cheaper than a startup line is rare.
pub fn colour_enabled(stream: Stream) -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match stream {
        Stream::Out => std::io::stdout().is_terminal(),
        Stream::Err => std::io::stderr().is_terminal(),
    }
}

fn now_stamp() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

/// Write one line to `stream`, stamped with the local clock and styled if that stream
/// can take it. A failed write is dropped: a console line must never be the thing that
/// takes a daemon down.
pub fn print(stream: Stream, source: Source, level: Level, message: &str) {
    let line = render(
        &now_stamp(),
        source,
        level,
        message,
        colour_enabled(stream),
    );
    match stream {
        Stream::Out => {
            let mut w = std::io::stdout();
            let _ = writeln!(w, "{line}");
        }
        Stream::Err => {
            let mut w = std::io::stderr();
            let _ = writeln!(w, "{line}");
        }
    }
}

/// An informational line on **stderr** — the stream the startup narrative already uses.
pub fn info(source: Source, message: &str) {
    print(Stream::Err, source, Level::Info, message);
}

/// An informational line on **stdout**, for the sites that deliberately chose it.
pub fn out(source: Source, message: &str) {
    print(Stream::Out, source, Level::Info, message);
}

/// Something the human should notice but that did not stop anything.
pub fn warn(source: Source, message: &str) {
    print(Stream::Err, source, Level::Warn, message);
}

/// Something failed.
pub fn error(source: Source, message: &str) {
    print(Stream::Err, source, Level::Error, message);
}

/// One seat, as the startup summary shows it.
#[derive(Debug, Clone)]
pub struct SeatRow {
    pub id: String,
    pub agent: String,
    pub model: String,
    pub flavor: String,
    /// `false` renders a `disabled` note in the last column instead of a later,
    /// separate line — the six `seated …` lines and the three `is seated but
    /// DISABLED` lines were the same information printed twice.
    pub enabled: bool,
    /// Whether this seat also carries a chat lane.
    pub chat_lane: bool,
}

/// Render the seating summary as an aligned table, one `String` per row.
///
/// Columns are padded by **character** count, which lines the model column up at the
/// same byte offset for the ASCII ids `validate_quark_id` permits.
pub fn seating_table(rows: &[SeatRow]) -> Vec<String> {
    let width = |f: fn(&SeatRow) -> &str| rows.iter().map(|r| f(r).chars().count()).max().unwrap_or(0);
    let id_w = width(|r| r.id.as_str());
    let agent_w = width(|r| r.agent.as_str());
    let model_w = width(|r| r.model.as_str());
    rows.iter()
        .map(|r| {
            let mut line = format!(
                "  {id:<id_w$}  {agent:<agent_w$}  {model:<model_w$}  {flavor}",
                id = r.id,
                agent = r.agent,
                model = r.model,
                flavor = r.flavor,
            );
            if !r.enabled {
                line.push_str("  · disabled");
            }
            if r.chat_lane {
                line.push_str("  · chat lane");
            }
            line
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat(id: &str, model: &str) -> SeatRow {
        SeatRow {
            id: id.into(),
            agent: "claude".into(),
            model: model.into(),
            flavor: "Worker".into(),
            enabled: true,
            chat_lane: false,
        }
    }

    /// The plain line is exactly `HH:MM:SS  <source>  <message>` and carries no escape
    /// bytes at all — this is what `… 2>&1 | cat` and every log file gets.
    #[test]
    fn a_colourless_line_has_the_documented_shape_and_no_escape_bytes() {
        let s = render("12:00:00", Source::Chamber, Level::Info, "hello", false);
        assert_eq!(s, "12:00:00  chamber  hello");
        assert!(!s.contains('\u{1b}'), "escape byte in a colourless line: {s:?}");
    }

    /// Every source pads to the same width, so the message column never wanders.
    #[test]
    fn the_source_column_is_wide_enough_for_every_source() {
        let offsets: Vec<usize> = [Source::Chamber, Source::Gluon, Source::Acp]
            .into_iter()
            .map(|src| {
                // A marker no source spelling contains, or `find` matches the prefix.
                let line = render("12:00:00", src, Level::Info, "~msg~", false);
                line.find("~msg~").expect("the message is in the line")
            })
            .collect();
        assert!(
            offsets.windows(2).all(|w| w[0] == w[1]),
            "the message column moved between sources: {offsets:?}"
        );
        assert_eq!(SOURCE_WIDTH, Source::Chamber.to_string().chars().count());
    }

    /// A level styles a line; it never rewords it. Strip the escapes from a `warn` or
    /// an `error` and you get the `info` line back, byte for byte.
    #[test]
    fn a_level_changes_the_styling_and_never_the_text() {
        let plain = render("12:00:00", Source::Gluon, Level::Info, "the tree is dirty", false);
        for level in [Level::Warn, Level::Error] {
            let colourless = render("12:00:00", Source::Gluon, level, "the tree is dirty", false);
            assert_eq!(colourless, plain, "{level:?} reworded the message");
            let coloured = render("12:00:00", Source::Gluon, level, "the tree is dirty", true);
            assert!(coloured.contains("the tree is dirty"), "{level:?} lost the message");
            assert!(coloured.contains('\u{1b}'), "{level:?} produced no styling");
        }
    }

    /// Info and error must not paint identically, or colour carries no information.
    #[test]
    fn the_levels_paint_differently() {
        let of = |level| render("12:00:00", Source::Gluon, level, "m", true);
        assert_ne!(of(Level::Info), of(Level::Warn));
        assert_ne!(of(Level::Warn), of(Level::Error));
    }

    /// `NO_COLOR` is honoured whatever the stream is. Serialised with the other
    /// environment-reading test by being the only one that sets it.
    #[test]
    fn no_color_disables_colour_even_on_a_terminal() {
        // SAFETY: single-threaded within this test; no other test reads NO_COLOR.
        unsafe { std::env::set_var("NO_COLOR", "1") };
        assert!(!colour_enabled(Stream::Out));
        assert!(!colour_enabled(Stream::Err));
        unsafe { std::env::remove_var("NO_COLOR") };
    }

    /// Different-length ids must not push the model column around.
    #[test]
    fn the_seating_table_aligns_its_model_column() {
        let rows = [
            seat("acp-claude", "opus[1m]"),
            seat("cli-agy", "gemini-3.6-flash"),
            seat("acp-copilot-longer-id", "Copilot"),
        ];
        let lines = seating_table(&rows);
        let offsets: Vec<usize> = lines
            .iter()
            .zip(rows.iter())
            .map(|(line, row)| line.find(&row.model).expect("the model is in the row"))
            .collect();
        assert!(
            offsets.windows(2).all(|w| w[0] == w[1]),
            "model column moved: {offsets:?}\n{}",
            lines.join("\n")
        );
    }

    /// A disabled seat is a column in the table, not a separate line printed later.
    #[test]
    fn a_disabled_seat_is_a_column_not_a_second_line() {
        let mut off = seat("acp-codex", "gpt-5.6-terra");
        off.enabled = false;
        let lines = seating_table(&[off, seat("acp-claude", "opus[1m]")]);
        assert_eq!(lines.len(), 2, "a disabled seat produced an extra line");
        assert!(lines[0].contains("disabled"), "{}", lines[0]);
        assert!(!lines[1].contains("disabled"), "{}", lines[1]);
    }
}
