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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TurnOutcome {
    pub message: Option<String>,
    #[serde(default)]
    pub used_tokens: u32,
    #[serde(default)]
    pub permission: Option<PermissionAsk>,
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
            mode: Mode::Bypass,
        };
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
            TurnOutcome { message: None, used_tokens: 0, permission: None }
        );
    }
}
