use serde::{Deserialize, Serialize};

use crate::{Event, Mode, QuarkCard, Risk};

/// The curated context handed to a quark on excitation. The single chokepoint
/// where cost-control (what context), invariants (methodology), nucleus (project
/// SSOT), and roster (who to delegate to) converge.
// `PartialEq` but not `Eq` — contains `Vec<Event>`, and `Event` is not `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    /// The assignment this quark is being asked to act on.
    pub task: String,
    /// Enforced working protocol, injected as a Markdown preamble. v1: static.
    pub invariants: String,
    /// Available invariants loaded from the nucleus.
    #[serde(default)]
    pub available_invariants: Vec<String>,
    /// Relevant slice of the project SSOT (nucleus). v1: may be empty.
    pub nucleus_digest: String,
    /// Who exists, their flavor and energy — enables orchestration.
    pub roster: Vec<QuarkCard>,
    /// Recent relevant events. v1: a dumb recent window.
    pub field_window: Vec<Event>,
    /// Current working diff, not whole files. v1: may be empty.
    pub git_diff: String,
    /// **Where this quark works.** The directory every tool the adapter spawns
    /// runs in — the quark's own `git worktree` when worktree discipline is on,
    /// else the workspace root. Non-optional on purpose: an `Option` defaulting
    /// to `None` would let a forgetful adapter silently inherit the *daemon's*
    /// cwd, which is precisely the shared-tree hazard this field exists to close.
    #[serde(default)]
    pub cwd: std::path::PathBuf,
    /// Whether `cwd` is a worktree of this quark's own, or the workspace root it
    /// shares with the human and every other quark. The prompt used to assert
    /// isolation unconditionally — so a quark in the shared tree was told its
    /// commits could only reach `main` "through the merge gate", refused to
    /// commit at all, and left its work to be lost. Say which world it is in.
    #[serde(default)]
    pub isolated: bool,
    /// The permission authority this quark runs under this turn. The engine
    /// resolves it from the field (`resolve_mode`) before excitation; real
    /// adapters translate it into the CLI's permission posture. Defaults to the
    /// most restrictive rung.
    #[serde(default)]
    pub mode: Mode,
}

/// What an adapter returns after a turn. File mutations are NOT reported here —
/// the gluon derives them from git diff (Plan 2). A `None` message means the
/// quark produced no field message this turn.
/// A quark's self-declared request to perform a risky operation, surfaced on its
/// `TurnOutcome`. The engine turns this into a `Kind::PermissionReq` and consults
/// the god-mode policy. Mirror of gatekeeper's `PendingPermission` (lattice can't
/// depend on gatekeeper, so the shape is duplicated deliberately).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionAsk {
    pub risk: Risk,
    pub description: String,
}

/// What one turn produced. `PartialEq` but no longer `Eq`: `usage` carries floats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TurnOutcome {
    pub message: Option<String>,
    #[serde(default)]
    pub used_tokens: u32,
    #[serde(default)]
    pub permission: Option<PermissionAsk>,
    /// Context + quota the provider reported for this turn. Empty when the provider
    /// reports nothing (a mock) or has no quota concept (claude reports context only).
    #[serde(default, skip_serializing_if = "crate::Usage::is_empty")]
    pub usage: crate::Usage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, EnergyState, Flavor, Kind, QuarkId};

    #[test]
    fn projection_holds_events_and_roster() {
        let proj = Projection {
            task: "Build auth".into(),
            invariants: "Snapshot before editing.".into(),
            available_invariants: vec![],
            nucleus_digest: String::new(),
            roster: vec![QuarkCard {
                id: QuarkId::new("agy"),
                flavor: Flavor::Worker,
                energy: EnergyState::Available,
                provider: String::new(),
                model: String::new(),
            }],
            field_window: vec![Event::new(
                Actor::Human,
                Some(QuarkId::new("claude")),
                Kind::Message { body: "go".into() },
            )],
            git_diff: String::new(),
            isolated: true,
            cwd: std::path::PathBuf::from("/tmp/wt"),
            mode: Mode::Bypass,
        };
        assert_eq!(proj.cwd, std::path::Path::new("/tmp/wt"));
        assert_eq!(proj.roster.len(), 1);
        assert_eq!(proj.field_window.len(), 1);
        assert_eq!(proj.mode, Mode::Bypass);
    }

    #[test]
    fn projection_mode_defaults_to_ask_when_absent() {
        // A pre-mode field snapshot (no `mode` key) deserializes to the most
        // restrictive rung, not an accidental Bypass.
        let json = r#"{
            "task":"x","invariants":"","available_invariants":[],
            "nucleus_digest":"","roster":[],"field_window":[],"git_diff":""
        }"#;
        let proj: Projection = serde_json::from_str(json).unwrap();
        assert_eq!(proj.mode, Mode::Ask);
    }

    #[test]
    fn turn_outcome_default_is_empty() {
        assert_eq!(
            TurnOutcome::default(),
            TurnOutcome { message: None, used_tokens: 0, permission: None, usage: Default::default() }
        );
    }
}
