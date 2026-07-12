//! The chamber view-model: a pure projection of the field (`Vec<Event>`) into
//! what the UI renders — a chat row per event plus the derived per-quark roster
//! state. No GPUI here, so this is fully unit-tested.

use std::collections::HashMap;

use hadron_gatekeeper::{global_mode, has_override, resolve_mode, Mode};
use hadron_lattice::{Actor, Event, Kind, QuarkId, QuarkState, Team};

/// One rendered chat row. `kind_label` lets the UI style/filter by event type;
/// `body` is a display string synthesized for non-message events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    pub from: String,
    pub to: Option<String>,
    pub body: String,
    pub kind_label: &'static str,
}

/// One roster entry: a quark, its latest lifecycle state, its effective
/// permission mode (and whether that's an explicit per-quark override vs the
/// inherited global), and its legibility (`provider`/`model` from the team
/// config — empty strings when the seat isn't in `team.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRow {
    pub id: String,
    pub state: QuarkState,
    pub mode: Mode,
    pub mode_is_override: bool,
    pub provider: String,
    pub model: String,
}

/// Everything the chamber needs to render one frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChamberView {
    pub messages: Vec<MessageRow>,
    pub roster: Vec<RosterRow>,
    /// The global default permission mode (the status-bar mode badge), folded
    /// from the field's `ModeSet` events. `Mode::Ask` if none has been set.
    pub global_mode: Mode,
    /// An outstanding permission request awaiting the human's Approve/Deny, if any.
    /// The UI renders this as a toast; `None` means nothing to decide.
    pub pending_permission: Option<hadron_gatekeeper::PendingPermission>,
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
        Kind::EnergyReport { used_tokens } => {
            (format!("used {used_tokens} tokens"), "energy_report")
        }
        Kind::Assign { task, invariants } => (
            format!("assigned: {task} (invariants: {:?})", invariants),
            "assign",
        ),
        Kind::PermissionReq { risk, description } => (
            format!("⚠️ permission requested ({risk:?}): {description}"),
            "permission_req",
        ),
        Kind::PermissionGrant { approved, remember } => (
            format!(
                "permission {}{}",
                if *approved { "approved" } else { "denied" },
                if *remember { " (remembered)" } else { "" },
            ),
            "permission_grant",
        ),
        Kind::ModeSet { mode } => (format!("mode → {mode:?}").to_lowercase(), "mode_set"),
        Kind::Unknown { kind, .. } => (format!("unrecognized event: {kind}"), "unrecognized"),
    };
    MessageRow {
        from,
        to,
        body,
        kind_label,
    }
}

/// Project the field into a renderable view, with no team annotations
/// (`provider`/`model` blank). Convenience for tests and callers without a team.
#[cfg_attr(not(test), allow(dead_code))]
pub fn project(events: &[Event]) -> ChamberView {
    project_with_team(events, &Team::default())
}

/// Project the field into a renderable view. Roster order is first-seen; a
/// quark's state is the latest `Kind::Status` it authored (default `Ground`);
/// its mode is folded from `ModeSet` events (per-quark override over global);
/// its `provider`/`model` come from `team` (blank when the seat is unknown).
pub fn project_with_team(events: &[Event], team: &Team) -> ChamberView {
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
            let qid = QuarkId::new(&id);
            let (provider, model) = team
                .get(&qid)
                .map(|s| (s.provider.clone(), s.model.clone()))
                .unwrap_or_default();
            RosterRow {
                state,
                mode: resolve_mode(events, &qid),
                mode_is_override: has_override(events, &qid),
                provider,
                model,
                id,
            }
        })
        .collect();

    ChamberView {
        messages,
        roster,
        global_mode: global_mode(events),
        pending_permission: hadron_gatekeeper::pending_permission(events),
    }
}

// NOTE: the human's `@mention` routing lives in the daemon now
// (`hadron_gluon::router::human_mentions`), not the chamber. The chamber writes
// the raw message with `to: None` and leaves mentions in the body, so ONE message
// can address several quarks; the daemon resolves and fans them out. (A former
// `parse_mention` here lifted a single leading mention into `to` — which silently
// dropped the second addressee — and has been removed.)

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(from: Actor, to: Option<&str>, kind: Kind) -> Event {
        Event::new(from, to.map(QuarkId::new), kind)
    }

    #[test]
    fn global_mode_and_per_quark_override_are_surfaced() {
        let evs = vec![
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
            ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Auto }), // global Auto
            ev(
                Actor::Human,
                Some("agy"),
                Kind::ModeSet { mode: Mode::Bypass },
            ), // agy override
        ];
        let view = project(&evs);
        assert_eq!(view.global_mode, Mode::Auto);
        let agy = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy.mode, Mode::Bypass);
        assert!(agy.mode_is_override);
    }

    #[test]
    fn roster_row_inherits_global_and_carries_team_legibility() {
        use hadron_lattice::{Flavor, Seat};
        let team = Team {
            quarks: vec![Seat {
                id: QuarkId::new("agy"),
                provider: "agy".into(),
                model: "gemini-3-pro".into(),
                flavor: Flavor::Worker,
            }],
        };
        let evs = vec![
            ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Write }),
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
        ];
        let view = project_with_team(&evs, &team);
        let agy = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy.mode, Mode::Write, "inherits the global default");
        assert!(!agy.mode_is_override);
        assert_eq!(agy.provider, "agy");
        assert_eq!(agy.model, "gemini-3-pro");
    }

    #[test]
    fn mode_set_renders_as_a_row() {
        let view = project(&[ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Bypass })]);
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].kind_label, "mode_set");
        assert!(view.messages[0].body.contains("bypass"));
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
    fn pending_permission_is_surfaced_then_cleared_by_a_grant() {
        let req = ev(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::PermissionReq {
                risk: hadron_gatekeeper::Risk::BashExec,
                description: "cargo publish".into(),
            },
        );
        // With an outstanding request, the view carries it (addressed to the asker).
        let view = project(std::slice::from_ref(&req));
        let pending = view
            .pending_permission
            .expect("outstanding request surfaced");
        assert_eq!(pending.quark, QuarkId::new("agy"));
        assert_eq!(pending.description, "cargo publish");

        // Once granted, the toast clears.
        let grant = ev(
            Actor::Human,
            Some("agy"),
            Kind::PermissionGrant {
                approved: true,
                remember: false,
            },
        );
        let view = project(&[req, grant]);
        assert!(view.pending_permission.is_none());
    }

    #[test]
    fn assign_becomes_a_row() {
        let view = project(&[ev(
            Actor::Human,
            Some("agy"),
            Kind::Assign {
                task: "work".into(),
                invariants: vec!["no errors".into()],
            },
        )]);
        assert_eq!(view.messages.len(), 1);
        let row = &view.messages[0];
        assert_eq!(row.kind_label, "assign");
        assert_eq!(row.body, "assigned: work (invariants: [\"no errors\"])");
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
}
