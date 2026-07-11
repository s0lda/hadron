//! Field IO for the gluon. Single implementation lives in
//! [`hadron_lattice::io`] (runtime-free, shared with the chamber); this module
//! re-exports it so existing `crate::field::*` call sites are unchanged.

pub use hadron_lattice::io::{append_event, read_events};

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Actor, Event, Kind, QuarkId, QuarkState};
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
