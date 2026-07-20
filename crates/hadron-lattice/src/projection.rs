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
    /// **What the swarm has learned**, carried across turns and sessions — the memory
    /// INDEX, and the only part of memory that is loaded every turn.
    ///
    /// The difference between a quark that repeats yesterday's mistake and one that
    /// doesn't is not the model — it is whether anything persisted. A quark with no
    /// memory re-derives the same wrong conclusion every session, forever. It is one
    /// file for the whole swarm, not one per quark: a lesson agy paid for is a lesson
    /// opus should not have to pay for again.
    ///
    /// It is an INDEX: one short line per lesson, each optionally naming a note that
    /// holds the long version. The index is always in the prompt, so it must stay
    /// small; the notes are read on demand, by the quark, only when the line it just
    /// read turns out to matter.
    #[serde(default)]
    pub memory: String,
    /// Whether the index was cut to fit the budget. A memory silently dropped for size
    /// is the failure mode we keep killing everywhere else: the quark cannot tell
    /// "never learned" from "not shown".
    #[serde(default)]
    pub memory_truncated: bool,
    /// Where to WRITE the index. Non-optional and always populated: telling a quark to
    /// "remember this" without telling it the path is an instruction it cannot obey,
    /// and it will either invent a path or silently do nothing.
    #[serde(default)]
    pub memory_path: std::path::PathBuf,
    /// Where the long-form notes live. The index points into this directory; the quark
    /// opens a note with its own tools when it needs the detail.
    #[serde(default)]
    pub memory_notes_dir: std::path::PathBuf,
    /// Who exists, their flavor and energy — enables orchestration.
    pub roster: Vec<QuarkCard>,
    /// What other quarks are doing right now, if they are currently mid-turn.
    #[serde(default)]
    pub live_activities: Vec<crate::live::Activity>,
    /// Recent relevant events. v1: a dumb recent window.
    pub field_window: Vec<Event>,
    /// Whether older events were dropped to fit the window's byte budget. The
    /// truncation used to be silent, so a quark whose earlier instruction had
    /// aged out could not tell the difference between "the human never said it"
    /// and "I can no longer see it" — and confidently acted on the partial
    /// picture. Same class as the merge-gate fiction: say what is missing.
    #[serde(default)]
    pub field_truncated: bool,
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
    /// Matched role body if this turn matched a role (spec §3.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_body: Option<String>,
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
    pub permission: Option<PermissionAsk>,
    /// Spend + context + quota the provider reported for this turn. Empty when the
    /// provider reports nothing (a mock) or has no quota concept (claude reports
    /// context only).
    ///
    /// There is deliberately **no `used_tokens: u32`** here any more. An adapter
    /// cannot report a bare number, because a bare number meant a different thing for
    /// every transport and we printed all of them in the same column. It reports
    /// components on [`crate::TokenSpend`]; the engine calls
    /// [`crate::TokenSpend::fresh`] — the single totaller — to get a figure.
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
                display_name: None,
                flavor: Flavor::Worker,
                energy: EnergyState::Available,
                provider: String::new(),
                model: String::new(),
                roles: vec![],
                exclusive: false,
                commands: crate::team::SeatCommands::default(),
                energy_limit: None,
                deny_skills: vec![],
            }],
            field_window: vec![Event::new(
                Actor::Human,
                Some(QuarkId::new("claude")),
                Kind::Message { body: "go".into() },
            )],
            field_truncated: false,
            memory: String::new(),
            memory_path: std::path::PathBuf::new(),
            memory_truncated: false,
            memory_notes_dir: std::path::PathBuf::new(),
            live_activities: vec![],
            git_diff: String::new(),
            isolated: true,
            cwd: std::path::PathBuf::from("/tmp/wt"),
            mode: Mode::Bypass,
            role_body: None,
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
            TurnOutcome { message: None, permission: None, usage: Default::default() }
        );
    }
}
