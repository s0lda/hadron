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
}

impl ReseatPlan {
    /// Nothing to do — the running roster already matches the file. The overwhelmingly
    /// common case: the daemon reconciles on a timer and the team almost never changes.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.replaced.is_empty() && self.removed.is_empty()
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
        parts.join(" ")
    }
}

/// Diff the roster the daemon is *running* against the one `team.json` now *describes*.
///
/// Seat equality is whole-struct equality, so any field the human can change (model,
/// provider, transport, boot command, flavor) counts as a change and forces a rebuild
/// of that one seat — and only that one.
pub fn plan(running: &Team, desired: &Team) -> ReseatPlan {
    let mut out = ReseatPlan::default();

    for want in &desired.quarks {
        match running.get(&want.id) {
            // Unchanged: says nothing, does nothing, keeps its session. The point.
            Some(have) if have == want => {}
            Some(_) => out.replaced.push(want.clone()),
            None => out.added.push(want.clone()),
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
        Team { quarks: seats.to_vec() }
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
            id: QuarkId::new("acp-claude"),
            provider: "acp-claude".into(),
            model: "claude".into(),
            flavor: Flavor::Worker,
            transport: Transport::Acp,
            command: None,
        };
        let mut desired = running.clone();
        desired.quarks.push(new_seat.clone());

        let p = plan(&running, &desired);
        assert_eq!(p.added, vec![new_seat]);
        assert!(p.replaced.is_empty(), "the working quarks must not be rebuilt");
        assert!(p.removed.is_empty());
        assert_eq!(p.summary(), "+acp-claude");
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
            id: QuarkId::new("acp-claude"),
            provider: "acp-claude".into(),
            model: "claude".into(),
            flavor: Flavor::Worker,
            transport: Transport::Acp,
            command: Some(AcpCommand {
                program: "npx".into(),
                args: args.iter().map(|s| s.to_string()).collect(),
            }),
        };
        let running = team(&[seat(&["-y", "old-agent"])]);
        let desired = team(&[seat(&["-y", "new-agent"])]);
        assert_eq!(plan(&running, &desired).replaced, vec![seat(&["-y", "new-agent"])]);
    }
}
