//! The chamber view-model: a pure projection of the field (`Vec<Event>`) into
//! what the UI renders — a chat row per event plus the derived per-quark roster
//! state. No GPUI here, so this is fully unit-tested.

use std::collections::HashMap;

use hadron_lattice::{Actor, Event, Kind, QuarkId, QuarkState};

/// One rendered chat row. `kind_label` lets the UI style/filter by event type;
/// `body` is a display string synthesized for non-message events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    pub from: String,
    pub to: Option<String>,
    pub body: String,
    pub kind_label: &'static str,
}

/// One roster entry: a quark and its latest known lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRow {
    pub id: String,
    pub state: QuarkState,
}

/// Everything the chamber needs to render one frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChamberView {
    pub messages: Vec<MessageRow>,
    pub roster: Vec<RosterRow>,
}

fn actor_str(a: &Actor) -> String {
    match a {
        Actor::Human => "human".to_string(),
        Actor::Gluon => "gluon".to_string(),
        Actor::Quark(q) => q.as_str().to_string(),
    }
}

fn note(order: &mut Vec<String>, id: &str) {
    if !order.iter().any(|x| x == id) {
        order.push(id.to_string());
    }
}

fn render_row(e: &Event) -> MessageRow {
    let from = actor_str(&e.from);
    let to = e.to.as_ref().map(|t| t.as_str().to_string());
    let (body, kind_label): (String, &'static str) = match &e.kind {
        Kind::Message { body } => (body.clone(), "message"),
        Kind::Status { state } => (format!("{state:?}").to_lowercase(), "status"),
        Kind::Edit { paths, summary, .. } => {
            (format!("edited {} path(s): {summary}", paths.len()), "edit")
        }
        Kind::Command { cmd, exit, .. } => (format!("$ {cmd} (exit {exit})"), "command"),
        Kind::Snapshot { label, .. } => (format!("snapshot: {label}"), "snapshot"),
        Kind::EnergyReport { used_tokens } => (format!("used {used_tokens} tokens"), "energy_report"),
        Kind::Unknown { kind, .. } => (format!("unrecognized event: {kind}"), "unrecognized"),
    };
    MessageRow {
        from,
        to,
        body,
        kind_label,
    }
}

/// Project the field into a renderable view. Roster order is first-seen; a
/// quark's state is the latest `Kind::Status` it authored (default `Ground`).
pub fn project(events: &[Event]) -> ChamberView {
    let mut messages = Vec::with_capacity(events.len());
    let mut order: Vec<String> = Vec::new();
    let mut states: HashMap<String, QuarkState> = HashMap::new();

    for e in events {
        // Roster membership: any quark that authors or is addressed.
        if let Actor::Quark(q) = &e.from {
            note(&mut order, q.as_str());
        }
        if let Some(t) = &e.to {
            note(&mut order, t.as_str());
        }
        // Latest status per quark wins.
        if let (Actor::Quark(q), Kind::Status { state }) = (&e.from, &e.kind) {
            states.insert(q.as_str().to_string(), *state);
        }
        messages.push(render_row(e));
    }

    let roster = order
        .into_iter()
        .map(|id| {
            let state = states.get(&id).copied().unwrap_or(QuarkState::Ground);
            RosterRow { id, state }
        })
        .collect();

    ChamberView { messages, roster }
}

/// Parse a human input line into an optional addressee and the message body.
///
/// A leading `@name ` (name followed by whitespace and a non-empty body) is
/// lifted into the addressee, so `@claude fix the tests` targets `claude` with
/// body `fix the tests`. Anything else — no `@`, a bare `@name`, or an `@` with
/// no body — is sent as-is to no one (`to = None`), matching how the human
/// speaks to the field at large.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub fn parse_mention(text: &str) -> (Option<QuarkId>, String) {
    if let Some(rest) = text.strip_prefix('@') {
        if let Some((name, body)) = rest.split_once(char::is_whitespace) {
            let body = body.trim();
            if !name.is_empty() && !body.is_empty() {
                return (Some(QuarkId::new(name)), body.to_string());
            }
        }
    }
    (None, text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(from: Actor, to: Option<&str>, kind: Kind) -> Event {
        Event::new(from, to.map(QuarkId::new), kind)
    }

    #[test]
    fn message_becomes_a_row() {
        let view = project(&[ev(
            Actor::Human,
            Some("claude"),
            Kind::Message {
                body: "build it".into(),
            },
        )]);
        assert_eq!(view.messages.len(), 1);
        let row = &view.messages[0];
        assert_eq!(row.from, "human");
        assert_eq!(row.to.as_deref(), Some("claude"));
        assert_eq!(row.body, "build it");
        assert_eq!(row.kind_label, "message");
    }

    #[test]
    fn latest_status_wins_in_roster() {
        let agy = || Actor::Quark(QuarkId::new("agy"));
        let view = project(&[
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
            ev(
                agy(),
                None,
                Kind::Status {
                    state: QuarkState::Excited,
                },
            ),
            ev(
                agy(),
                None,
                Kind::Status {
                    state: QuarkState::Ground,
                },
            ),
        ]);
        let agy_row = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy_row.state, QuarkState::Ground); // latest wins
    }

    #[test]
    fn unknown_event_is_a_muted_row_not_dropped() {
        // Construct an Unknown by round-tripping a future-kind JSON line.
        let line = serde_json::to_string(&json!({
            "v": 2,
            "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "ts": "2026-07-10T14:00:00Z",
            "from": "gluon",
            "to": null,
            "kind": "edit_by_hash",
            "block_hash": "9f86d0"
        }))
        .unwrap();
        let e: Event = serde_json::from_str(&line).unwrap();
        let view = project(&[e]);
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].kind_label, "unrecognized");
        assert!(view.messages[0].body.contains("edit_by_hash"));
    }

    #[test]
    fn roster_includes_authors_and_addressees() {
        let view = project(&[
            ev(
                Actor::Human,
                Some("orch"),
                Kind::Message { body: "go".into() },
            ),
            ev(
                Actor::Quark(QuarkId::new("orch")),
                Some("worker"),
                Kind::Message {
                    body: "@worker do it".into(),
                },
            ),
        ]);
        let ids: Vec<&str> = view.roster.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"orch"));
        assert!(ids.contains(&"worker"));
        // human is not a quark → not on the roster.
        assert!(!ids.contains(&"human"));
    }

    #[test]
    fn mention_lifts_addressee_and_body() {
        let (to, body) = parse_mention("@claude fix the failing tests");
        assert_eq!(to.as_ref().map(QuarkId::as_str), Some("claude"));
        assert_eq!(body, "fix the failing tests");
    }

    #[test]
    fn plain_message_has_no_addressee() {
        let (to, body) = parse_mention("hello everyone");
        assert_eq!(to, None);
        assert_eq!(body, "hello everyone");
    }

    #[test]
    fn bare_mention_is_not_treated_as_addressing() {
        // No body after the name → send the whole thing, addressed to no one.
        let (to, body) = parse_mention("@claude");
        assert_eq!(to, None);
        assert_eq!(body, "@claude");
    }

    #[test]
    fn mention_trims_extra_whitespace_in_body() {
        let (to, body) = parse_mention("@agy    run the build   ");
        assert_eq!(to.as_ref().map(QuarkId::as_str), Some("agy"));
        assert_eq!(body, "run the build");
    }
}
