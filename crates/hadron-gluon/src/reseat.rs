//! Reconciling the **live** roster with `team.json`.
//!
//! The daemon used to read `team.json` exactly once, at boot. A seat the human
//! added in Settings therefore existed on disk and nowhere else: the wizard said
//! "Ready", the swarm had never heard of it, and `@<its-id>` resolved to nobody.
//!
//! The fix is not "reload the team" — it is **reconcile** the team. A reload
//! rebuilds every quark, and rebuilding a quark that did not change is exactly the
//! thing we must never do: an ACP seat holds a *resident* session (a live
//! subprocess, a conversation the next turn can see), and a silent rebuild would
//! drop it on the floor to no purpose.
//!
//! So a [`ReseatPlan`] can only name work that has to happen. An **unchanged seat is
//! unrepresentable in it** — there is no `unchanged` field to accidentally act on.
//! Everything the plan does not mention keeps the exact quark instance, and the exact
//! session, it already had.
//!
//! *When* the plan is applied matters as much as what is in it; that safety argument
//! lives in the daemon, which only reconciles at a quiescent point.

use hadron_lattice::{QuarkId, Seat, Team};

/// The work required to turn the running roster into the one `team.json` describes.
///
/// Construct it with [`plan`]. There is deliberately no way to say "leave this seat
/// alone": that is the default, and it is the default *by omission*, which is the
/// only way to be sure a live ACP session is never dropped by accident.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReseatPlan {
    /// Seats the human added — no quark of this id is running.
    pub added: Vec<Seat>,
    /// Seats whose id is already running but whose *definition* changed (a different
    /// model, provider, transport or boot command). The old quark is torn down and a
    /// new one seated: a changed seat is a different agent, and it must not inherit
    /// the previous one's session.
    pub replaced: Vec<Seat>,
    /// Seats the human removed from `team.json`.
    pub removed: Vec<QuarkId>,
    /// Seats whose **participation** changed — the same agent, switched on or off.
    ///
    /// This carries `(id, enabled)` and **deliberately not a `Seat`**: there is nothing
    /// here to build a quark *from*, so it is not possible to accidentally rebuild one
    /// while applying a toggle. That is the guarantee, made structural rather than
    /// remembered — a disabled ACP quark keeps its resident subprocess and its
    /// conversation, because the only thing this entry can do is flip a flag.
    pub toggled: Vec<(QuarkId, bool)>,
    /// Seats whose **display name** changed — the same agent, renamed. Like `toggled`,
    /// this carries only `(id, name)`, never a `Seat`, so applying a rename cannot rebuild
    /// a quark: it only updates the roster card the router matches `@mentions` against, so
    /// `@NewName` resolves without dropping a live ACP session just to change a label.
    pub renamed: Vec<(QuarkId, Option<String>)>,
}

impl ReseatPlan {
    /// Nothing to do — the running roster already matches the file. The overwhelmingly
    /// common case: the daemon reconciles on a timer and the team almost never changes.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.replaced.is_empty()
            && self.removed.is_empty()
            && self.toggled.is_empty()
            && self.renamed.is_empty()
    }

    /// A one-line summary for the daemon's log, so a re-seat is something the human
    /// can *see* happen rather than infer.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for s in &self.added {
            parts.push(format!("+{}", s.id.as_str()));
        }
        for s in &self.replaced {
            parts.push(format!("~{}", s.id.as_str()));
        }
        for id in &self.removed {
            parts.push(format!("-{}", id.as_str()));
        }
        for (id, on) in &self.toggled {
            parts.push(format!("{}{}", if *on { "on:" } else { "off:" }, id.as_str()));
        }
        for (id, name) in &self.renamed {
            parts.push(format!("name:{}={}", id.as_str(), name.as_deref().unwrap_or("<id>")));
        }
        parts.join(" ")
    }
}

/// Diff the roster the daemon is *running* against the one `team.json` now *describes*.
///
/// Any field the human can change (model, provider, transport, boot command, flavor)
/// counts as a change and forces a rebuild of that one seat — and only that one.
///
/// **`enabled` is the sole exception**, and it has to be. Comparing whole structs would
/// make switching a quark off a *replace*: the engine would tear down its adapter and
/// build a new one, which for an ACP seat kills a live subprocess and throws away the
/// conversation — to flip a boolean. So identity is [`Seat::same_agent`] (everything
/// but `enabled`), and a participation change becomes a `toggled` entry, which carries
/// no `Seat` and therefore cannot rebuild anything.
pub fn plan(running: &Team, desired: &Team) -> ReseatPlan {
    let mut out = ReseatPlan::default();

    for want in &desired.quarks {
        match running.get(&want.id) {
            None => out.added.push(want.clone()),
            // A changed *definition* is a different agent — rebuilt, not kept.
            Some(have) if !have.same_agent(want) => out.replaced.push(want.clone()),
            // Same agent: only cheap metadata can differ, and each is applied WITHOUT a
            // rebuild so a live ACP session survives. Both can change at once (the human
            // renamed and toggled in one edit), so they are independent, not exclusive.
            Some(have) => {
                if have.enabled != want.enabled {
                    out.toggled.push((want.id.clone(), want.enabled));
                }
                if have.display_name != want.display_name {
                    out.renamed.push((want.id.clone(), want.display_name.clone()));
                }
            }
        }
    }
    for have in &running.quarks {
        if desired.get(&have.id).is_none() {
            out.removed.push(have.id.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{AcpCommand, Flavor, Transport};

    fn cli(id: &str, model: &str) -> Seat {
        Seat::cli(QuarkId::new(id), "claude", model, Flavor::Worker)
    }

    fn team(seats: &[Seat]) -> Team {
        Team { quarks: seats.to_vec(), roster: vec![], max_exchanges: None }
    }

    /// The load-bearing property: a seat that did not change generates **no work**.
    /// This is what protects a live ACP session from being rebuilt for nothing.
    #[test]
    fn an_unchanged_seat_produces_an_empty_plan() {
        let t = team(&[cli("opus", "opus"), cli("agy", "gemini")]);
        assert!(plan(&t, &t).is_empty(), "an identical team must be a no-op");
    }

    /// Jake's case: the Settings wizard appends a seat. Only that seat is added; the
    /// two quarks already working are not mentioned, so they are not touched.
    #[test]
    fn a_seat_the_wizard_added_is_the_only_work_in_the_plan() {
        let running = team(&[cli("opus", "opus"), cli("agy", "gemini")]);
        let new_seat = Seat {
            id: QuarkId::new("new-guy"),
            display_name: None,
            vendor: "claude".into(),
            model: "claude".into(),
            flavor: Flavor::Worker,
            transport: Transport::Acp,
            command: None,
            cli: None,
            enabled: true,
            effort: None,
            mode_config: None,
            roles: vec![],
            exclusive: false,
            commands: hadron_lattice::SeatCommands::default(),
            secret_env: Vec::new(),
            energy_limit: None,
            deny_skills: vec![],
        };
        let mut desired = running.clone();
        desired.quarks.push(new_seat.clone());

        let p = plan(&running, &desired);
        assert_eq!(p.added, vec![new_seat]);
        assert!(p.replaced.is_empty(), "the working quarks must not be rebuilt");
        assert!(p.removed.is_empty());
        assert_eq!(p.summary(), "+new-guy");
    }

    #[test]
    fn a_removed_seat_is_unseated() {
        let running = team(&[cli("opus", "opus"), cli("agy", "gemini")]);
        let desired = team(&[cli("opus", "opus")]);
        let p = plan(&running, &desired);
        assert_eq!(p.removed, vec![QuarkId::new("agy")]);
        assert!(p.added.is_empty() && p.replaced.is_empty());
    }

    /// A changed *definition* on the same id is a different agent. It must be rebuilt,
    /// not silently kept — otherwise the human changes the model in Settings and the
    /// old model keeps answering.
    #[test]
    fn changing_the_model_replaces_that_seat_and_only_that_seat() {
        let running = team(&[cli("opus", "opus"), cli("agy", "gemini")]);
        let desired = team(&[cli("opus", "opus"), cli("agy", "gemini-3-pro")]);
        let p = plan(&running, &desired);
        assert_eq!(p.replaced, vec![cli("agy", "gemini-3-pro")]);
        assert!(p.added.is_empty() && p.removed.is_empty());
    }

    /// Re-pointing an ACP seat at a different agent binary changes the boot command
    /// but nothing else. Whole-struct equality is what catches this; comparing only
    /// id+model would have called it "unchanged" and kept booting the old agent.
    #[test]
    fn changing_only_the_acp_boot_command_still_counts_as_a_change() {
        let seat = |args: &[&str]| Seat {
            id: QuarkId::new("agent"),
            display_name: None,
            vendor: "agent".into(),
            model: "claude".into(),
            flavor: Flavor::Worker,
            transport: Transport::Acp,
            enabled: true,
            command: Some(AcpCommand {
                program: "npx".into(),
                args: args.iter().map(|s| s.to_string()).collect(),
            }),
            cli: None,
            effort: None,
            mode_config: None,
            roles: vec![],
            exclusive: false,
            commands: hadron_lattice::SeatCommands::default(),
            secret_env: Vec::new(),
            energy_limit: None,
            deny_skills: vec![],
        };
        let running = team(&[seat(&["-y", "old-agent"])]);
        let desired = team(&[seat(&["-y", "new-agent"])]);
        assert_eq!(plan(&running, &desired).replaced, vec![seat(&["-y", "new-agent"])]);
    }
}

#[cfg(test)]
mod enabled_tests {
    use super::*;
    use hadron_lattice::{AcpCommand, Flavor, Transport};

    fn acp(id: &str, enabled: bool) -> Seat {
        Seat {
            id: QuarkId::new(id),
            display_name: None,
            vendor: "claude".into(),
            model: "claude".into(),
            flavor: Flavor::Worker,
            transport: Transport::Acp,
            command: Some(AcpCommand { program: "npx".into(), args: vec![] }),
            cli: None,
            enabled,
            effort: None,
            mode_config: None,
            roles: vec![],
            exclusive: false,
            commands: hadron_lattice::SeatCommands::default(),
            secret_env: Vec::new(),
            energy_limit: None,
            deny_skills: vec![],
        }
    }
    fn team(seats: &[Seat]) -> Team {
        Team { quarks: seats.to_vec(), roster: vec![], max_exchanges: None }
    }

    /// **The trap this design exists to avoid.** Whole-struct equality would make a
    /// disable look like a changed seat, and a changed seat is REBUILT — which for an
    /// ACP quark kills its subprocess and discards the conversation, to flip a boolean.
    ///
    /// Disabling must produce a `toggled` entry and *nothing else*. If this ever shows
    /// up in `replaced`, a live session is being thrown away.
    #[test]
    fn disabling_a_quark_toggles_it_and_never_replaces_it() {
        let running = team(&[acp("acp-claude", true)]);
        let desired = team(&[acp("acp-claude", false)]);

        let p = plan(&running, &desired);

        assert_eq!(p.toggled, vec![(QuarkId::new("acp-claude"), false)]);
        assert!(p.replaced.is_empty(), "a disable must NOT rebuild the quark — its ACP session would die");
        assert!(p.added.is_empty());
        assert!(p.removed.is_empty(), "disabling is not unseating");
        assert_eq!(p.summary(), "off:acp-claude");
    }

    /// And back on again — same shape, and still not a rebuild.
    #[test]
    fn enabling_a_quark_toggles_it_and_never_replaces_it() {
        let p = plan(&team(&[acp("x", false)]), &team(&[acp("x", true)]));
        assert_eq!(p.toggled, vec![(QuarkId::new("x"), true)]);
        assert!(p.replaced.is_empty() && p.removed.is_empty() && p.added.is_empty());
        assert_eq!(p.summary(), "on:x");
    }

    /// The carve-out is exactly one field wide. A real definition change on a seat that
    /// is *also* being toggled is still a replace — otherwise "change the model and
    /// disable it" would keep the old model seated and merely switch it off.
    #[test]
    fn a_real_change_still_replaces_even_when_enabled_also_changed() {
        let running = team(&[acp("x", true)]);
        let mut want = acp("x", false);
        want.model = "sonnet".into();

        let p = plan(&running, &team(&[want.clone()]));

        assert_eq!(p.replaced, vec![want], "a different model is a different agent");
        assert!(p.toggled.is_empty(), "the replace already carries the new enabled flag");
    }

    /// An unchanged disabled seat is still a no-op. A quark that is off must not be
    /// re-toggled (and so re-logged) on every reconcile tick.
    #[test]
    fn an_unchanged_disabled_seat_is_still_no_work() {
        let t = team(&[acp("x", false)]);
        assert!(plan(&t, &t).is_empty(), "off and staying off is nothing to do");
    }

    /// A display-name change is metadata, not a new agent: it must be a `renamed`, never a
    /// `replaced`. Rebuilding to change a label would kill a live ACP session — the same
    /// class of mistake the toggle carve-out exists to prevent.
    #[test]
    fn renaming_a_quark_renames_it_and_never_replaces_it() {
        let mut before = acp("acp-claude", true);
        before.display_name = None;
        let mut after = acp("acp-claude", true);
        after.display_name = Some("Claude".into());

        let p = plan(&team(&[before]), &team(&[after]));

        assert_eq!(p.renamed, vec![(QuarkId::new("acp-claude"), Some("Claude".into()))]);
        assert!(p.replaced.is_empty(), "a rename must NOT rebuild — the ACP session would die");
        assert!(p.toggled.is_empty() && p.added.is_empty() && p.removed.is_empty());
        assert_eq!(p.summary(), "name:acp-claude=Claude");
    }

    /// A real definition change alongside a rename is still a replace (the definition wins);
    /// the rebuilt seat already carries the new name, so it is not ALSO a separate rename.
    #[test]
    fn a_model_change_with_a_rename_is_a_replace_not_a_rename() {
        let mut before = acp("x", true);
        before.display_name = None;
        let mut after = acp("x", true);
        after.display_name = Some("New".into());
        after.model = "sonnet".into();

        let p = plan(&team(&[before]), &team(&[after.clone()]));

        assert_eq!(p.replaced, vec![after]);
        assert!(p.renamed.is_empty(), "a replace already carries the new name");
    }
}
