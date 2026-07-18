//! The mode ladder: fold the field's `ModeSet` events into an effective mode
//! per quark, and decide whether a proposed op is pre-authorized or must ask a
//! human. Pure and offline — no events emitted, no daemon paused, no UI.

use std::collections::HashSet;

use hadron_lattice::{Actor, Event, Kind, Mode, QuarkId, Risk};

/// The set of `(quark, op)` pairs the human has chosen to *always* allow.
/// `op` is the self-declared `PermissionReq` description — the daemon never sees
/// the raw command on the CLI-adapter path, so matching is on the declared
/// string (exact match, or a trailing-`*` prefix glob — see [`op_matches`]).
pub type AllowRules = HashSet<(QuarkId, String)>;

/// The set of `(quark, op)` pairs the human has explicitly *denied* — only
/// consulted under No-Human-Mode (`decide(..., no_human = true, ...)`); a deny
/// match always wins over an allow match. Same matching rules as `AllowRules`.
pub type DenyRules = HashSet<(QuarkId, String)>;

/// The matrix's verdict for a single proposed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The mode pre-authorizes this op; proceed without a human prompt (the
    /// gluon grants — in `Bypass` this is the orchestrator's standing authority).
    AutoApprove,
    /// Pause and surface a permission request to the human.
    AskHuman,
    /// No-Human-Mode only (global `Bypass`): pause and surface the request to
    /// the orchestrator quark for adjudication instead of a human. Never
    /// returned unless `decide` is called with `no_human = true`.
    AskOrchestrator,
}

/// Exact match, or a trailing-`*` prefix glob: `"git push*"` matches any op
/// starting with `"git push"` (e.g. `"git push origin main"`). No other glob
/// syntax is supported — this is a hand-rolled, deliberately tiny matcher, not
/// a general glob engine.
pub fn op_matches(pattern: &str, op: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => op.starts_with(prefix),
        None => pattern == op,
    }
}

/// Whether any rule in `rules` names `quark` with a pattern matching `op`
/// (see [`op_matches`]). Shared by the allow- and deny-list checks under
/// No-Human-Mode.
fn rules_match(rules: &HashSet<(QuarkId, String)>, quark: &QuarkId, op: &str) -> bool {
    rules.iter().any(|(q, pattern)| q == quark && op_matches(pattern, op))
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
/// Two tables, selected by `no_human` (additive — see below):
///
/// **`no_human == false` (today's table, byte-for-byte — `global`/`deny` ignored,
/// `AskOrchestrator` never returned):**
///
/// | mode \ risk | WorkspaceEdit | BashExec                         |
/// |-------------|---------------|----------------------------------|
/// | Ask         | AskHuman      | AskHuman                         |
/// | Write       | AutoApprove   | AskHuman                         |
/// | Auto        | AutoApprove   | allow-listed ? AutoApprove : Ask |
/// | Bypass      | AutoApprove   | AutoApprove                      |
///
/// **`no_human == true` (No-Human-Mode, worker `mode` already clamped by the
/// caller — see spec §2):**
/// 1. `mode == Bypass` → `AutoApprove` (an explicit per-quark standing grant).
/// 2. a `deny` match (exact or trailing-`*` glob) → escalate — **deny wins even
///    over an allow match**.
/// 3. an `allow` match → `AutoApprove`.
/// 4. else → escalate.
///
/// Where `escalate = AskOrchestrator` if `global == Bypass`, else `AskHuman`
/// (No-Human-Mode itself is only meaningful once the global default is
/// `Bypass`; short of that it degrades to the ordinary human-ask path).
#[allow(clippy::too_many_arguments)]
pub fn decide(
    mode: Mode,
    global: Mode,
    no_human: bool,
    risk: Risk,
    op: &str,
    quark: &QuarkId,
    allow: &AllowRules,
    deny: &DenyRules,
) -> Decision {
    if !no_human {
        // Byte-for-byte today's logic. `global` and `deny` are intentionally
        // unused here — see `additive_off_matches_todays_matrix`.
        return match (mode, risk) {
            (Mode::Ask, _) => Decision::AskHuman,
            // Write / Auto / Bypass all auto-approve edits.
            (_, Risk::WorkspaceEdit) => Decision::AutoApprove,
            (Mode::Write, Risk::BashExec) => Decision::AskHuman,
            (Mode::Auto, Risk::BashExec) => {
                if allow.contains(&(quark.clone(), op.to_string())) {
                    Decision::AutoApprove
                } else {
                    Decision::AskHuman
                }
            }
            (Mode::Bypass, Risk::BashExec) => Decision::AutoApprove,
        };
    }

    if mode == Mode::Bypass {
        return Decision::AutoApprove;
    }
    let escalate = if global == Mode::Bypass { Decision::AskOrchestrator } else { Decision::AskHuman };
    if rules_match(deny, quark, op) {
        return escalate; // deny wins, even if also allow-listed
    }
    if rules_match(allow, quark, op) {
        return Decision::AutoApprove;
    }
    escalate
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

    /// `no_human = false` throughout: `global` is irrelevant (pass `Ask` as a
    /// stand-in) and `deny` is unconsulted (pass empty).
    fn decide_today(mode: Mode, risk: Risk, op: &str, quark: &QuarkId, allow: &AllowRules) -> Decision {
        decide(mode, Mode::Ask, false, risk, op, quark, allow, &DenyRules::new())
    }

    #[test]
    fn decide_truth_table() {
        let none = AllowRules::new();
        let k = q("agy");
        use Decision::*;
        use Mode::*;
        // Ask: everything asks.
        assert_eq!(decide_today(Ask, Risk::WorkspaceEdit, "", &k, &none), AskHuman);
        assert_eq!(decide_today(Ask, Risk::BashExec, "x", &k, &none), AskHuman);
        // Write: edits auto, bash asks.
        assert_eq!(decide_today(Write, Risk::WorkspaceEdit, "", &k, &none), AutoApprove);
        assert_eq!(decide_today(Write, Risk::BashExec, "x", &k, &none), AskHuman);
        // Auto: edits auto; bash asks unless remembered.
        assert_eq!(decide_today(Auto, Risk::WorkspaceEdit, "", &k, &none), AutoApprove);
        assert_eq!(decide_today(Auto, Risk::BashExec, "cargo test", &k, &none), AskHuman);
        // Bypass: everything auto.
        assert_eq!(decide_today(Bypass, Risk::WorkspaceEdit, "", &k, &none), AutoApprove);
        assert_eq!(decide_today(Bypass, Risk::BashExec, "rm -rf", &k, &none), AutoApprove);
    }

    #[test]
    fn auto_mode_honors_the_allow_list_per_quark() {
        let mut rules = AllowRules::new();
        rules.insert((q("agy"), "cargo test".to_string()));
        use Decision::*;
        // Remembered op for agy → auto.
        assert_eq!(decide_today(Mode::Auto, Risk::BashExec, "cargo test", &q("agy"), &rules), AutoApprove);
        // Same op, different quark → still asks (rules are per-quark).
        assert_eq!(decide_today(Mode::Auto, Risk::BashExec, "cargo test", &q("kimi"), &rules), AskHuman);
        // Different op for agy → asks.
        assert_eq!(decide_today(Mode::Auto, Risk::BashExec, "cargo publish", &q("agy"), &rules), AskHuman);
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

    /// THE additive proof: `no_human = false` reproduces today's `decide` body
    /// (copied here verbatim as `old_decide`), across the full mode × risk grid,
    /// with both an empty and a populated allow-list, and — critically — across
    /// every `global` value and with `deny` populated (proving both are ignored
    /// on this path, exactly as the spec requires).
    #[test]
    fn additive_off_matches_todays_matrix() {
        // Verbatim snapshot of the pre-change `decide` body.
        fn old_decide(mode: Mode, risk: Risk, op: &str, quark: &QuarkId, rules: &AllowRules) -> Decision {
            match (mode, risk) {
                (Mode::Ask, _) => Decision::AskHuman,
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

        let modes = [Mode::Ask, Mode::Write, Mode::Auto, Mode::Bypass];
        let risks = [Risk::WorkspaceEdit, Risk::BashExec];
        let globals = [Mode::Ask, Mode::Write, Mode::Auto, Mode::Bypass];
        let k = q("agy");
        let op = "cargo test";

        let mut populated = AllowRules::new();
        populated.insert((k.clone(), op.to_string()));
        // A deny match on the very same (quark, op) — proves `deny` is
        // unconsulted when `no_human = false`, since old_decide never denies.
        let mut deny = DenyRules::new();
        deny.insert((k.clone(), op.to_string()));

        for allow in [AllowRules::new(), populated.clone()] {
            for &mode in &modes {
                for &risk in &risks {
                    let expected = old_decide(mode, risk, op, &k, &allow);
                    for &global in &globals {
                        let got = decide(mode, global, false, risk, op, &k, &allow, &deny);
                        assert_eq!(
                            got, expected,
                            "mode={mode:?} risk={risk:?} global={global:?} allow_populated={}",
                            !allow.is_empty()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn deny_wins_over_allow_when_no_human() {
        let k = q("agy");
        let op = "cargo publish";
        let mut allow = AllowRules::new();
        allow.insert((k.clone(), op.to_string()));
        let mut deny = DenyRules::new();
        deny.insert((k.clone(), op.to_string()));

        // Worker mode is Auto (not Bypass), so the lists are consulted. Even
        // though `op` is allow-listed, the deny match wins → escalate.
        assert_eq!(
            decide(Mode::Auto, Mode::Auto, true, Risk::BashExec, op, &k, &allow, &deny),
            Decision::AskHuman
        );
        assert_eq!(
            decide(Mode::Auto, Mode::Bypass, true, Risk::BashExec, op, &k, &allow, &deny),
            Decision::AskOrchestrator
        );
    }

    #[test]
    fn glob_deny_matches() {
        let k = q("agy");
        let mut deny = DenyRules::new();
        deny.insert((k.clone(), "git push*".to_string()));
        let allow = AllowRules::new();

        assert_eq!(
            decide(Mode::Auto, Mode::Auto, true, Risk::BashExec, "git push origin main", &k, &allow, &deny),
            Decision::AskHuman
        );
        // A different op the glob doesn't cover is unaffected by the deny entry.
        assert_eq!(
            decide(Mode::Auto, Mode::Auto, true, Risk::BashExec, "git pull", &k, &allow, &deny),
            Decision::AskHuman // still escalates: not allow-listed either
        );
    }

    #[test]
    fn no_human_escalates_to_orchestrator_only_under_global_bypass() {
        let k = q("agy");
        let allow = AllowRules::new();
        let deny = DenyRules::new();
        // Not allow-listed, not deny-listed, worker mode Auto (not Bypass).
        assert_eq!(
            decide(Mode::Auto, Mode::Bypass, true, Risk::BashExec, "cargo publish", &k, &allow, &deny),
            Decision::AskOrchestrator
        );
        assert_eq!(
            decide(Mode::Auto, Mode::Auto, true, Risk::BashExec, "cargo publish", &k, &allow, &deny),
            Decision::AskHuman
        );
    }

    #[test]
    fn worker_bypass_override_auto_approves() {
        let k = q("agy");
        // Even with a deny match and a non-Bypass global, an explicit per-quark
        // Bypass override on the worker short-circuits straight to AutoApprove.
        let mut deny = DenyRules::new();
        deny.insert((k.clone(), "rm -rf*".to_string()));
        let allow = AllowRules::new();
        assert_eq!(
            decide(Mode::Bypass, Mode::Auto, true, Risk::BashExec, "rm -rf /tmp/x", &k, &allow, &deny),
            Decision::AutoApprove
        );
    }

    #[test]
    fn op_matches_exact() {
        assert!(op_matches("git push", "git push"));
        assert!(!op_matches("git push", "git pull"));
    }

    #[test]
    fn op_matches_trailing_glob() {
        assert!(op_matches("git push*", "git push"));
        assert!(op_matches("git push*", "git push origin main"));
        assert!(!op_matches("git push*", "git pull origin main"));
    }

    #[test]
    fn op_matches_non_match() {
        assert!(!op_matches("cargo test", "cargo test --release"));
        assert!(!op_matches("cargo test*", "cargo publish"));
    }
}
