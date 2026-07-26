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

/// `target`'s latest `PermissionReq` with no `PermissionGrant` addressed back to
/// it after that, or `None` if `target` never asked or its latest ask was
/// already answered. Scoped per-quark deliberately: turns run in parallel, so
/// two different quarks can have simultaneous outstanding asks, and a scan for
/// "the field's newest request overall" would let one quark's grant mask
/// another quark's still-unanswered one (the same shape `router::next_pending`
/// had to fix). Mirrors the stateless reconstruct-from-the-field rule there:
/// the daemon can wait on an ungranted request exactly as it waits on a pending
/// turn.
pub fn pending_permission(events: &[Event], target: &QuarkId) -> Option<PendingPermission> {
    pending_permission_at(events, target).map(|(_, p)| p)
}

/// Same as [`pending_permission`], plus the source event's index — so
/// [`any_pending_permission`] can compare how long each quark has been waiting.
fn pending_permission_at(events: &[Event], target: &QuarkId) -> Option<(usize, PendingPermission)> {
    let idx = events.iter().rposition(|e| {
        matches!(&e.from, Actor::Quark(q) if q == target) && matches!(e.kind, Kind::PermissionReq { .. })
    })?;
    let answered = events[idx + 1..].iter().any(|e| {
        matches!(e.kind, Kind::PermissionGrant { .. }) && matches!(&e.to, Some(t) if t == target)
    });
    if answered {
        return None;
    }
    match &events[idx].kind {
        Kind::PermissionReq { risk, description } => Some((
            idx,
            PendingPermission { quark: target.clone(), risk: *risk, description: description.clone() },
        )),
        _ => unreachable!("rposition matched PermissionReq"),
    }
}

/// The single request to show as the chamber's one permission toast: the
/// OLDEST still-unanswered request across every quark that has ever asked
/// (FIFO by request event, not by field position) — so a second quark's
/// answered ask can never hide a first quark's still-pending one, and whichever
/// the human answers first, the next-oldest surfaces in its place.
pub fn any_pending_permission(events: &[Event]) -> Option<PendingPermission> {
    let mut askers: Vec<&QuarkId> = Vec::new();
    for e in events {
        if let Actor::Quark(q) = &e.from {
            if matches!(e.kind, Kind::PermissionReq { .. }) && !askers.contains(&q) {
                askers.push(q);
            }
        }
    }
    askers
        .into_iter()
        .filter_map(|q| pending_permission_at(events, q))
        .min_by_key(|(idx, _)| *idx)
        .map(|(_, p)| p)
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

/// Build an "always allow" answer: an approving grant that *remembers* the op,
/// so `allow_rules` will auto-approve the same `(quark, op)` next time. The
/// chamber appends this when the human clicks "Always allow".
pub fn grant_remembering(pending: &PendingPermission) -> Event {
    Event::new(
        Actor::Human,
        Some(pending.quark.clone()),
        Kind::PermissionGrant { approved: true, remember: true },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Actor, QuarkId};

    fn req(desc: &str) -> Event {
        req_from("agy", desc)
    }
    fn req_from(quark: &str, desc: &str) -> Event {
        Event::new(
            Actor::Quark(QuarkId::new(quark)),
            None,
            Kind::PermissionReq { risk: Risk::BashExec, description: desc.into() },
        )
    }
    fn grant_ev(approved: bool) -> Event {
        grant_to("agy", approved)
    }
    fn grant_to(quark: &str, approved: bool) -> Event {
        Event::new(Actor::Human, Some(QuarkId::new(quark)), Kind::PermissionGrant { approved, remember: false })
    }
    fn msg() -> Event {
        Event::new(Actor::Human, None, Kind::Message { body: "hi".into() })
    }

    #[test]
    fn an_older_quarks_unanswered_request_survives_a_newer_quarks_granted_one() {
        // agy asks, then a different quark asks and gets granted. agy's own
        // request was never answered — the field's globally-newest-request scan
        // must not let cli-agy's resolution mask it.
        let events = [
            req_from("agy", "first quark's op"),
            req_from("cli-agy", "second quark's op"),
            grant_to("cli-agy", true),
        ];
        assert!(
            pending_permission(&events, &QuarkId::new("agy")).is_some(),
            "agy's request was never answered and must still surface"
        );
    }

    #[test]
    fn any_pending_permission_picks_the_oldest_across_quarks_fifo() {
        // cli-agy's ask is resolved; agy's, the OLDER still-open one, must win —
        // never the field's newest event regardless of who it belongs to.
        let events = [
            req_from("agy", "first quark's op"),
            req_from("cli-agy", "second quark's op"),
            grant_to("cli-agy", true),
        ];
        let got = any_pending_permission(&events).unwrap();
        assert_eq!(got.quark, QuarkId::new("agy"));
        assert_eq!(got.description, "first quark's op");
    }

    #[test]
    fn any_pending_permission_is_none_once_every_asker_is_answered() {
        let events = [
            req_from("agy", "op a"),
            grant_to("agy", true),
            req_from("cli-agy", "op b"),
            grant_to("cli-agy", true),
        ];
        assert_eq!(any_pending_permission(&events), None);
    }

    #[test]
    fn none_when_no_request() {
        assert_eq!(pending_permission(&[msg()], &QuarkId::new("agy")), None);
    }

    #[test]
    fn returns_unanswered_request() {
        let got = pending_permission(&[req("cargo publish")], &QuarkId::new("agy")).unwrap();
        assert_eq!(got.quark, QuarkId::new("agy"));
        assert_eq!(got.risk, Risk::BashExec);
        assert_eq!(got.description, "cargo publish");
    }

    #[test]
    fn grant_is_a_human_event_addressed_to_the_asking_quark() {
        let pending = pending_permission(&[req("cargo publish")], &QuarkId::new("agy")).unwrap();
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
        assert_eq!(pending_permission(&[req("x"), grant_ev(true)], &QuarkId::new("agy")), None);
    }

    #[test]
    fn denied_request_is_also_resolved() {
        // A deny (approved=false) still answers the request; not pending.
        assert_eq!(pending_permission(&[req("x"), grant_ev(false)], &QuarkId::new("agy")), None);
    }

    #[test]
    fn newest_unanswered_request_wins() {
        let got = pending_permission(&[req("first"), grant_ev(true), req("second")], &QuarkId::new("agy")).unwrap();
        assert_eq!(got.description, "second");
    }
}
