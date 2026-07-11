use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use hadron_lattice::Event;

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

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Actor, Kind, QuarkId, QuarkState};
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
        assert!(matches!(events[0].kind, hadron_lattice::Kind::Unknown { .. }));
    }

    #[test]
    fn torn_line_is_skipped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        std::fs::write(&path, "{not valid json\n").unwrap();
        assert_eq!(read_events(&path).unwrap().len(), 0);
    }
}
