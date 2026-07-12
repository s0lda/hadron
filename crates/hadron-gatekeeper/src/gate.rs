use hadron_lattice::{Actor, Event, Kind, QuarkId, Risk};

/// An outstanding permission request awaiting a human grant/deny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPermission {
    /// The quark that asked — a grant must be addressed back to it so the engine
    /// (`next_pending`) re-selects it and it resumes.
    pub quark: QuarkId,
    pub risk: Risk,
    pub description: String,
}

/// The latest `PermissionReq` with no `PermissionGrant` after it, or `None` if
/// there is no request, the last one was already answered, or it was not authored
/// by a quark (so there is no addressee to resume). Mirrors the stateless
/// reconstruct-from-the-field rule of `router::next_pending`: the daemon can wait
/// on an ungranted request exactly as it waits on a pending turn.
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
    let quark = match &events[idx].from {
        Actor::Quark(id) => id.clone(),
        _ => return None, // a non-quark request has no addressee to resume
    };
    match &events[idx].kind {
        Kind::PermissionReq { risk, description } => Some(PendingPermission {
            quark,
            risk: *risk,
            description: description.clone(),
        }),
        _ => unreachable!("rposition matched PermissionReq"),
    }
}

/// Build the human's answer to a pending request: a `PermissionGrant` addressed
/// back to the asking quark so the engine resumes it. The chamber appends this
/// when the human clicks Approve/Deny.
pub fn grant(pending: &PendingPermission, approved: bool) -> Event {
    Event::new(
        Actor::Human,
        Some(pending.quark.clone()),
        Kind::PermissionGrant { approved, remember: false },
    )
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
    fn grant_ev(approved: bool) -> Event {
        Event::new(Actor::Human, None, Kind::PermissionGrant { approved, remember: false })
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
        assert_eq!(got.quark, QuarkId::new("agy"));
        assert_eq!(got.risk, Risk::BashExec);
        assert_eq!(got.description, "cargo publish");
    }

    #[test]
    fn grant_is_a_human_event_addressed_to_the_asking_quark() {
        let pending = pending_permission(&[req("cargo publish")]).unwrap();
        let approve = grant(&pending, true);
        assert_eq!(approve.from, Actor::Human);
        assert_eq!(approve.to, Some(QuarkId::new("agy")));
        assert!(matches!(approve.kind, Kind::PermissionGrant { approved: true, .. }));
        // And a denial:
        let deny = grant(&pending, false);
        assert!(matches!(deny.kind, Kind::PermissionGrant { approved: false, .. }));
    }

    #[test]
    fn granted_request_is_not_pending() {
        assert_eq!(pending_permission(&[req("x"), grant_ev(true)]), None);
    }

    #[test]
    fn denied_request_is_also_resolved() {
        // A deny (approved=false) still answers the request; not pending.
        assert_eq!(pending_permission(&[req("x"), grant_ev(false)]), None);
    }

    #[test]
    fn newest_unanswered_request_wins() {
        let got = pending_permission(&[req("first"), grant_ev(true), req("second")]).unwrap();
        assert_eq!(got.description, "second");
    }
}
