//! Runtime-free field IO. Lives in `hadron-lattice` (no tokio/anyhow) so both the
//! gluon daemon and the GPUI chamber can read/append the field without pulling in
//! each other's runtime. The two-process decoupling depends on this being here.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::Event;

/// Append a single event as one JSON line. Line-atomic; creates the file if
/// missing. Never rewrites existing content.
pub fn append_event(path: &Path, event: &Event) -> std::io::Result<()> {
    let line = serde_json::to_string(event)?;
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Read every event in order. A missing file yields an empty vec. Blank lines
/// are skipped. A line that fails to parse is skipped rather than crashing the
/// reader (append-only integrity means a torn final line can be ignored).
pub fn read_events(path: &Path) -> std::io::Result<Vec<Event>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(ev) => out.push(ev),
            Err(_) => continue,
        }
    }
    Ok(out)
}

/// Incrementally read events appended after byte `offset`. Returns the newly
/// parsed events and the byte offset to pass on the next call.
///
/// Only *complete* lines (terminated by a newline at or before EOF) are
/// consumed; a torn final line — bytes written after the last newline, e.g. a
/// half-flushed append — is left unread, and the returned offset points at the
/// last newline boundary so the completed line is picked up next time. Blank
/// and un-parseable lines are skipped (same contract as [`read_events`]). A
/// missing file yields `(vec![], offset)`.
pub fn read_new(path: &Path, offset: u64) -> std::io::Result<(Vec<Event>, u64)> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), offset)),
        Err(e) => return Err(e),
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    // Consume only up to and including the last newline; anything after it is a
    // torn final line we must not parse yet.
    let last_nl = match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => i,
        None => return Ok((Vec::new(), offset)), // no complete line yet
    };
    let complete = &buf[..=last_nl];
    let consumed = complete.len() as u64;

    let mut out = Vec::new();
    for line in complete.split(|&b| b == b'\n') {
        let text = match std::str::from_utf8(line) {
            Ok(t) => t.trim(),
            Err(_) => continue,
        };
        if text.is_empty() {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<Event>(text) {
            out.push(ev);
        }
    }
    Ok((out, offset + consumed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, Kind, QuarkId, QuarkState};
    use tempfile::tempdir;

    #[test]
    fn append_then_read_preserves_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");

        let e1 = Event::new(Actor::Human, Some(QuarkId::new("claude")), Kind::Message { body: "one".into() });
        let e2 = Event::new(Actor::Quark(QuarkId::new("claude")), None, Kind::Status { state: QuarkState::Ground });
        append_event(&path, &e1).unwrap();
        append_event(&path, &e2).unwrap();

        let events = read_events(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], e1);
        assert_eq!(events[1], e2);
    }

    #[test]
    fn missing_file_reads_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.jsonl");
        assert_eq!(read_events(&path).unwrap().len(), 0);
    }

    #[test]
    fn unknown_kind_line_survives_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        std::fs::write(
            &path,
            "{\"v\":2,\"id\":\"01ARZ3NDEKTSV4RRFFQ69G5FAV\",\"ts\":\"2026-07-10T14:00:00Z\",\"from\":\"gluon\",\"to\":null,\"kind\":\"future_thing\",\"x\":1}\n",
        )
        .unwrap();
        let events = read_events(&path).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, Kind::Unknown { .. }));
    }

    #[test]
    fn torn_line_is_skipped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        std::fs::write(&path, "{not valid json\n").unwrap();
        assert_eq!(read_events(&path).unwrap().len(), 0);
    }

    #[test]
    fn read_new_reads_only_appended_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");

        let e1 = Event::new(Actor::Human, Some(QuarkId::new("claude")), Kind::Message { body: "one".into() });
        append_event(&path, &e1).unwrap();

        // First read from offset 0 sees e1.
        let (batch1, off1) = read_new(&path, 0).unwrap();
        assert_eq!(batch1, vec![e1.clone()]);
        assert_eq!(off1, std::fs::metadata(&path).unwrap().len());

        // Nothing new yet.
        let (batch_empty, off_same) = read_new(&path, off1).unwrap();
        assert!(batch_empty.is_empty());
        assert_eq!(off_same, off1);

        // Append e2; only e2 comes back.
        let e2 = Event::new(Actor::Quark(QuarkId::new("claude")), None, Kind::Status { state: QuarkState::Ground });
        append_event(&path, &e2).unwrap();
        let (batch2, off2) = read_new(&path, off1).unwrap();
        assert_eq!(batch2, vec![e2]);
        assert_eq!(off2, std::fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn read_new_missing_file_is_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.jsonl");
        let (batch, off) = read_new(&path, 0).unwrap();
        assert!(batch.is_empty());
        assert_eq!(off, 0);
    }

    #[test]
    fn read_new_does_not_consume_a_torn_final_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");

        // A complete line followed by a torn (newline-less) partial line.
        let e1 = Event::new(Actor::Human, None, Kind::Message { body: "done".into() });
        let complete = serde_json::to_string(&e1).unwrap();
        std::fs::write(&path, format!("{complete}\n{{\"partial\": ")).unwrap();

        let (batch, off) = read_new(&path, 0).unwrap();
        assert_eq!(batch, vec![e1.clone()]);
        // Offset stops at the newline after the complete line, NOT at EOF.
        assert_eq!(off, (complete.len() + 1) as u64);

        // Completing the torn line makes it readable on the next call.
        let e2 = Event::new(Actor::Human, None, Kind::Message { body: "more".into() });
        std::fs::write(&path, format!("{complete}\n{}\n", serde_json::to_string(&e2).unwrap())).unwrap();
        let (batch2, _off2) = read_new(&path, off).unwrap();
        assert_eq!(batch2, vec![e2]);
    }

    #[test]
    fn read_new_skips_blank_and_unparseable_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        let e1 = Event::new(Actor::Human, None, Kind::Message { body: "ok".into() });
        let good = serde_json::to_string(&e1).unwrap();
        // blank line, garbage line, then a good line — all newline-terminated.
        std::fs::write(&path, format!("\n{{bad json\n{good}\n")).unwrap();
        let (batch, off) = read_new(&path, 0).unwrap();
        assert_eq!(batch, vec![e1]);
        assert_eq!(off, std::fs::metadata(&path).unwrap().len());
    }
}
