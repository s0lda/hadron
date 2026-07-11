use hadron_lattice::{Event, Kind, Risk};

/// An outstanding permission request awaiting a human grant/deny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPermission {
    pub risk: Risk,
    pub description: String,
}

/// The latest `PermissionReq` with no `PermissionGrant` after it, or `None` if
/// there is no request or the last one was already answered. Mirrors the
/// stateless reconstruct-from-the-field rule of `router::next_pending`: the
/// daemon can wait on an ungranted request exactly as it waits on a pending turn.
pub fn pending_permission(events: &[Event]) -> Option<PendingPermission> {
    let idx = events
        .iter()
        .rposition(|e| matches!(e.kind, Kind::PermissionReq { .. }))?;
    let granted = events[idx + 1..]
        .iter()
        .any(|e| matches!(e.kind, Kind::PermissionGrant { .. }));
    if granted {
        return None;
    }
    match &events[idx].kind {
        Kind::PermissionReq { risk, description } => Some(PendingPermission {
            risk: *risk,
            description: description.clone(),
        }),
        _ => unreachable!("rposition matched PermissionReq"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Actor, QuarkId};

    fn req(desc: &str) -> Event {
        Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::PermissionReq { risk: Risk::BashExec, description: desc.into() },
        )
    }
    fn grant(approved: bool) -> Event {
        Event::new(Actor::Human, None, Kind::PermissionGrant { approved })
    }
    fn msg() -> Event {
        Event::new(Actor::Human, None, Kind::Message { body: "hi".into() })
    }

    #[test]
    fn none_when_no_request() {
        assert_eq!(pending_permission(&[msg()]), None);
    }

    #[test]
    fn returns_unanswered_request() {
        let got = pending_permission(&[req("cargo publish")]).unwrap();
        assert_eq!(got.risk, Risk::BashExec);
        assert_eq!(got.description, "cargo publish");
    }

    #[test]
    fn granted_request_is_not_pending() {
        assert_eq!(pending_permission(&[req("x"), grant(true)]), None);
    }

    #[test]
    fn denied_request_is_also_resolved() {
        // A deny (approved=false) still answers the request; not pending.
        assert_eq!(pending_permission(&[req("x"), grant(false)]), None);
    }

    #[test]
    fn newest_unanswered_request_wins() {
        let got = pending_permission(&[req("first"), grant(true), req("second")]).unwrap();
        assert_eq!(got.description, "second");
    }
}
