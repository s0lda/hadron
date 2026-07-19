//! The team roster config: the seats the human has added. A **seat** is one
//! quark instance = an id bound to a provider (backing CLI) running a model.
//! Stored as `team.json` and read by both the daemon (to instantiate adapters)
//! and the chamber (to make each roster row legible: `id · provider · model`).
//!
//! Pure and offline: this only parses the config. Spawning adapters from it is
//! the daemon's job; annotating the roster is the chamber's.

use serde::{Deserialize, Serialize};

use crate::QuarkId;

mod transport;
mod seat;
mod io;
mod migrate;
#[cfg(test)]
mod tests;

pub use transport::{Transport, AcpCommand, PromptChannel, ResumeMode, TimeoutArg, PostureMap, CliSpec};
pub use seat::{Seat, SeatCommands, SeatOverride};
pub use io::{parse_team, load_team, save_team, team_config_path, team_for_field, user_hadron_dir};
pub use migrate::{migrate_to_catalogue, seat_override_delta, orphan_overrides, legacy_id_renames, rename_legacy_ids, id_follows_convention};

pub(crate) use seat::is_false;

/// The full team: every seat the human has added.
///
/// Two ways to name a seat, and they coexist:
/// - [`Team::quarks`] — full self-contained [`Seat`] definitions. The original,
///   and still authoritative: a team.json that only uses this array behaves
///   byte-for-byte as it always did.
/// - [`Team::roster`] — role/state-only [`SeatOverride`]s that point at the global
///   catalogue for their definition. This is what a per-repo team.json carries
///   once the quark *definitions* live globally.
///
/// [`resolve_team`] folds the two (plus the catalogue) into a plain `Vec<Seat>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    #[serde(default)]
    pub quarks: Vec<Seat>,
    /// Per-repo role/state overrides that resolve against the global catalogue.
    /// Skipped when empty so a legacy team.json (and a catalogue file) never grow
    /// an empty `"roster": []` key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roster: Vec<SeatOverride>,
    #[serde(default)]
    pub max_exchanges: Option<usize>,
}

impl Team {
    /// Look up a seat by quark id.
    pub fn get(&self, id: &QuarkId) -> Option<&Seat> {
        self.quarks.iter().find(|s| &s.id == id)
    }

    /// Whether the team has any seats.
    pub fn is_empty(&self) -> bool {
        self.quarks.is_empty()
    }
}

/// Fold a repo team together with the global catalogue into the concrete team the
/// daemon seats and the chamber annotates: a plain `Team` whose `quarks` are fully
/// resolved [`Seat`]s and whose `roster` is empty. The result has exactly the shape
/// (`Vec<Seat>`) the reseat planner and adapters already handle, so **nothing
/// downstream of this function changes**.
///
/// **Backward compatible by construction.** A repo team that uses only the legacy
/// `quarks` array (full seats, empty `roster`) resolves to *itself* — every
/// existing `team.json` behaves byte-for-byte as before, whatever the catalogue holds.
///
/// The rules:
/// - Legacy full seats in `repo.quarks` are kept verbatim (self-contained, no
///   catalogue lookup) and take precedence: if an id appears in both a legacy seat
///   and an override, the legacy seat wins and the override is ignored.
/// - Each `repo.roster` override names a catalogue seat by id, clones its full
///   definition, and applies the role/state overrides where present.
/// - An override naming an id the catalogue does **not** define is **dropped**: a
///   role/state with no definition is not a seatable quark. Because a not-defined
///   (or not-adopted) quark can never become a [`Seat`] here, it can never reach the
///   daemon — that is the structural guarantee that a "gray-dot" available quark is
///   never booted. See [`orphan_overrides`] to surface the dropped ids for a warning.
/// - `max_exchanges` stays a **repo/team policy**, not a catalogue value: the repo's
///   setting is authoritative (absent → `None` → the daemon's default), so a repo
///   file's exchange cap is unchanged by the catalogue it now points at.
pub fn resolve_team(repo: &Team, global: &Team) -> Team {
    let mut quarks: Vec<Seat> = Vec::with_capacity(repo.quarks.len() + repo.roster.len());
    let mut seen: std::collections::HashSet<QuarkId> = std::collections::HashSet::new();
    // Legacy full seats first — self-contained, highest precedence.
    for seat in &repo.quarks {
        if seen.insert(seat.id.clone()) {
            quarks.push(seat.clone());
        }
    }
    // Overrides resolve their definition from the catalogue.
    for ov in &repo.roster {
        if seen.contains(&ov.id) {
            continue; // a legacy seat with this id already won
        }
        let Some(base) = global.get(&ov.id) else {
            continue; // orphan override: no definition to seat (see orphan_overrides)
        };
        let mut seat = base.clone();
        if let Some(flavor) = ov.flavor.clone() {
            seat.flavor = flavor;
        }
        if let Some(enabled) = ov.enabled {
            seat.enabled = enabled;
        }
        // Per-repo definition deltas, layered over the catalogue default. Absent =
        // inherit; for the already-optional knobs, `Some(None)` = cleared here.
        if let Some(model) = ov.model.clone() {
            seat.model = model;
        }
        if let Some(effort) = ov.effort.clone() {
            seat.effort = effort;
        }
        if let Some(mode_config) = ov.mode_config.clone() {
            seat.mode_config = mode_config;
        }
        if let Some(display_name) = ov.display_name.clone() {
            seat.display_name = display_name;
        }
        if let Some(roles) = ov.roles.clone() {
            seat.roles = roles;
        }
        if let Some(exclusive) = ov.exclusive {
            seat.exclusive = exclusive;
        }
        if let Some(commands) = ov.commands.clone() {
            seat.commands = commands;
        }
        seen.insert(ov.id.clone());
        quarks.push(seat);
    }
    Team {
        quarks,
        roster: Vec::new(),
        max_exchanges: repo.max_exchanges,
    }
}
