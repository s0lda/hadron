//! The mode ladder: fold the field's `ModeSet` events into an effective mode
//! per quark, and decide whether a proposed op is pre-authorized or must ask a
//! human. Pure and offline — no events emitted, no daemon paused, no UI.

use std::collections::HashSet;

use hadron_lattice::{Actor, Event, Kind, Mode, QuarkId, Risk};

/// The set of `(quark, op)` pairs the human has chosen to *always* allow.
/// `op` is the self-declared `PermissionReq` description — the daemon never sees
/// the raw command on the CLI-adapter path, so matching is on the declared
/// string (exact match in v1).
pub type AllowRules = HashSet<(QuarkId, String)>;

/// The matrix's verdict for a single proposed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The mode pre-authorizes this op; proceed without a human prompt (the
    /// gluon grants — in `Bypass` this is the orchestrator's standing authority).
    AutoApprove,
    /// Pause and surface a permission request to the human.
    AskHuman,
}

/// The effective global default mode: the latest `ModeSet` addressed to no one
/// (`to == None`), or `Mode::Ask` if none has been set.
pub fn global_mode(events: &[Event]) -> Mode {
    events
        .iter()
        .rev()
        .find_map(|e| match (&e.to, &e.kind) {
            (None, Kind::ModeSet { mode }) => Some(*mode),
            _ => None,
        })
        .unwrap_or_default()
}

/// The effective mode for `quark`: its latest per-quark override (a `ModeSet`
/// addressed to it) if any, otherwise the global default, otherwise `Mode::Ask`.
pub fn resolve_mode(events: &[Event], quark: &QuarkId) -> Mode {
    // The latest per-quark mode event wins. A `ModeSet` pins an override; a later
    // `ModeClear` (the "Default" rung) reverts the quark to the global default. The
    // global default itself only changes via a global `ModeSet` — so a per-quark
    // override survives any number of later global changes, which is the point.
    for e in events.iter().rev() {
        match (&e.to, &e.kind) {
            (Some(t), Kind::ModeSet { mode }) if t == quark => return *mode,
            (Some(t), Kind::ModeClear) if t == quark => break,
            _ => {}
        }
    }
    global_mode(events)
}

/// Whether `quark` carries an effective per-quark override (vs inheriting global):
/// the latest per-quark mode event is a `ModeSet`. Order-aware, so a `ModeClear`
/// after a `ModeSet` reads as "no override" — the quark is back on the global default.
pub fn has_override(events: &[Event], quark: &QuarkId) -> bool {
    for e in events.iter().rev() {
        match (&e.to, &e.kind) {
            (Some(t), Kind::ModeSet { .. }) if t == quark => return true,
            (Some(t), Kind::ModeClear) if t == quark => return false,
            _ => {}
        }
    }
    false
}

/// Fold remembered approvals into the allow-list. A `PermissionGrant` with
/// `remember == true && approved == true`, addressed to a quark, teaches a rule;
/// the op is recovered by pairing the grant with the most recent preceding
/// `PermissionReq` from that same quark (its `description`).
pub fn allow_rules(events: &[Event]) -> AllowRules {
    let mut rules = AllowRules::new();
    for (i, e) in events.iter().enumerate() {
        let Kind::PermissionGrant { approved: true, remember: true } = &e.kind else {
            continue;
        };
        let Some(quark) = &e.to else { continue };
        // Find the op: the nearest earlier PermissionReq authored by this quark.
        let op = events[..i].iter().rev().find_map(|p| match (&p.from, &p.kind) {
            (Actor::Quark(qk), Kind::PermissionReq { description, .. }) if qk == quark => {
                Some(description.clone())
            }
            _ => None,
        });
        if let Some(op) = op {
            rules.insert((quark.clone(), op));
        }
    }
    rules
}

/// Decide a single self-declared op under the effective `mode`.
///
/// | mode \ risk | WorkspaceEdit | BashExec                         |
/// |-------------|---------------|----------------------------------|
/// | Ask         | AskHuman      | AskHuman                         |
/// | Write       | AutoApprove   | AskHuman                         |
/// | Auto        | AutoApprove   | allow-listed ? AutoApprove : Ask |
/// | Bypass      | AutoApprove   | AutoApprove                      |
pub fn decide(mode: Mode, risk: Risk, op: &str, quark: &QuarkId, rules: &AllowRules) -> Decision {
    match (mode, risk) {
        (Mode::Ask, _) => Decision::AskHuman,
        // Write / Auto / Bypass all auto-approve edits.
        (_, Risk::WorkspaceEdit) => Decision::AutoApprove,
        (Mode::Write, Risk::BashExec) => Decision::AskHuman,
        (Mode::Auto, Risk::BashExec) => {
            if rules.contains(&(quark.clone(), op.to_string())) {
                Decision::AutoApprove
            } else {
                Decision::AskHuman
            }
        }
        (Mode::Bypass, Risk::BashExec) => Decision::AutoApprove,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(id: &str) -> QuarkId {
        QuarkId::new(id)
    }
    fn mode_set(to: Option<&str>, mode: Mode) -> Event {
        Event::new(Actor::Human, to.map(QuarkId::new), Kind::ModeSet { mode })
    }
    fn mode_clear(to: &str) -> Event {
        Event::new(Actor::Human, Some(QuarkId::new(to)), Kind::ModeClear)
    }
    fn req(from: &str, desc: &str) -> Event {
        Event::new(
            Actor::Quark(q(from)),
            None,
            Kind::PermissionReq { risk: Risk::BashExec, description: desc.into() },
        )
    }
    fn remember_grant(to: &str) -> Event {
        Event::new(
            Actor::Human,
            Some(q(to)),
            Kind::PermissionGrant { approved: true, remember: true },
        )
    }

    #[test]
    fn decide_truth_table() {
        let none = AllowRules::new();
        let k = q("agy");
        use Decision::*;
        use Mode::*;
        // Ask: everything asks.
        assert_eq!(decide(Ask, Risk::WorkspaceEdit, "", &k, &none), AskHuman);
        assert_eq!(decide(Ask, Risk::BashExec, "x", &k, &none), AskHuman);
        // Write: edits auto, bash asks.
        assert_eq!(decide(Write, Risk::WorkspaceEdit, "", &k, &none), AutoApprove);
        assert_eq!(decide(Write, Risk::BashExec, "x", &k, &none), AskHuman);
        // Auto: edits auto; bash asks unless remembered.
        assert_eq!(decide(Auto, Risk::WorkspaceEdit, "", &k, &none), AutoApprove);
        assert_eq!(decide(Auto, Risk::BashExec, "cargo test", &k, &none), AskHuman);
        // Bypass: everything auto.
        assert_eq!(decide(Bypass, Risk::WorkspaceEdit, "", &k, &none), AutoApprove);
        assert_eq!(decide(Bypass, Risk::BashExec, "rm -rf", &k, &none), AutoApprove);
    }

    #[test]
    fn auto_mode_honors_the_allow_list_per_quark() {
        let mut rules = AllowRules::new();
        rules.insert((q("agy"), "cargo test".to_string()));
        use Decision::*;
        // Remembered op for agy → auto.
        assert_eq!(decide(Mode::Auto, Risk::BashExec, "cargo test", &q("agy"), &rules), AutoApprove);
        // Same op, different quark → still asks (rules are per-quark).
        assert_eq!(decide(Mode::Auto, Risk::BashExec, "cargo test", &q("kimi"), &rules), AskHuman);
        // Different op for agy → asks.
        assert_eq!(decide(Mode::Auto, Risk::BashExec, "cargo publish", &q("agy"), &rules), AskHuman);
    }

    #[test]
    fn global_mode_defaults_to_ask_then_tracks_latest() {
        assert_eq!(global_mode(&[]), Mode::Ask);
        let evs = vec![mode_set(None, Mode::Write), mode_set(None, Mode::Bypass)];
        assert_eq!(global_mode(&evs), Mode::Bypass);
    }

    #[test]
    fn per_quark_override_beats_global_else_inherits() {
        let evs = vec![
            mode_set(None, Mode::Ask),           // global Ask
            mode_set(Some("agy"), Mode::Bypass), // agy overridden
        ];
        assert_eq!(resolve_mode(&evs, &q("agy")), Mode::Bypass);
        assert!(has_override(&evs, &q("agy")));
        // kimi has no override → inherits global Ask.
        assert_eq!(resolve_mode(&evs, &q("kimi")), Mode::Ask);
        assert!(!has_override(&evs, &q("kimi")));
    }

    #[test]
    fn latest_per_quark_override_wins() {
        let evs = vec![mode_set(Some("agy"), Mode::Auto), mode_set(Some("agy"), Mode::Write)];
        assert_eq!(resolve_mode(&evs, &q("agy")), Mode::Write);
    }

    /// Jake's exact requirement: "I allow Opus to Bypass because he is smart, but I keep
    /// all on Ask or Write." Pinning one quark and THEN moving the global must not move the
    /// pinned quark — only the ones that never got their own setting follow the global.
    #[test]
    fn a_per_quark_pin_survives_a_later_global_change() {
        let evs = vec![
            mode_set(None, Mode::Ask),            // global default: Ask
            mode_set(Some("opus"), Mode::Bypass), // human trusts Opus
            mode_set(None, Mode::Write),          // later: raise everyone else to Write
        ];
        // Opus keeps its pin despite the newer global.
        assert_eq!(resolve_mode(&evs, &q("opus")), Mode::Bypass);
        assert!(has_override(&evs, &q("opus")));
        // An un-pinned quark follows the newest global default.
        assert_eq!(resolve_mode(&evs, &q("kimi")), Mode::Write);
        assert!(!has_override(&evs, &q("kimi")));
    }

    /// The "Default" rung: clearing a pin reverts the quark to inheriting the global, and
    /// `has_override` goes false. Order matters — the clear is newer than the set.
    #[test]
    fn mode_clear_reverts_a_pin_to_the_global_default() {
        let evs = vec![
            mode_set(None, Mode::Write),          // global Write
            mode_set(Some("opus"), Mode::Bypass), // pin Opus
            mode_clear("opus"),                   // "Default" rung: un-pin
        ];
        assert_eq!(resolve_mode(&evs, &q("opus")), Mode::Write, "back on the global default");
        assert!(!has_override(&evs, &q("opus")), "no effective override after a clear");
    }

    /// A clear is not permanent: pinning again after a clear re-establishes the override
    /// (the latest per-quark mode event always wins).
    #[test]
    fn a_pin_after_a_clear_takes_effect_again() {
        let evs = vec![
            mode_set(Some("opus"), Mode::Bypass),
            mode_clear("opus"),
            mode_set(Some("opus"), Mode::Auto),
        ];
        assert_eq!(resolve_mode(&evs, &q("opus")), Mode::Auto);
        assert!(has_override(&evs, &q("opus")));
    }

    #[test]
    fn allow_rules_learns_from_a_remembered_grant() {
        // agy asks to run `cargo test`, human always-allows it.
        let evs = vec![req("agy", "cargo test"), remember_grant("agy")];
        let rules = allow_rules(&evs);
        assert!(rules.contains(&(q("agy"), "cargo test".to_string())));
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn plain_grant_does_not_teach_a_rule() {
        // A one-time approve (remember:false) leaves the allow-list empty.
        let plain = Event::new(
            Actor::Human,
            Some(q("agy")),
            Kind::PermissionGrant { approved: true, remember: false },
        );
        let evs = vec![req("agy", "cargo test"), plain];
        assert!(allow_rules(&evs).is_empty());
    }

    #[test]
    fn remembered_rule_pairs_with_the_right_quarks_request() {
        // Two quarks in flight; the remembered grant for agy must pick agy's op.
        let evs = vec![
            req("kimi", "cargo publish"),
            req("agy", "cargo test"),
            remember_grant("agy"),
        ];
        let rules = allow_rules(&evs);
        assert!(rules.contains(&(q("agy"), "cargo test".to_string())));
        assert!(!rules.contains(&(q("kimi"), "cargo publish".to_string())));
    }
}
