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

pub use transport::{Transport, AcpCommand, PromptChannel, ResumeMode, TimeoutArg, PostureMap, CliProbeSpec, CliSpec, StreamSpec, StreamFormat};
pub use seat::{ExternalRootSpec, ModelParams, Seat, SeatCommands, SeatOverride};
pub use io::{parse_team, load_team, save_team, team_config_path, team_for_field, user_hadron_dir};
pub use migrate::{migrate_to_catalogue, seat_override_delta, orphan_overrides, legacy_id_renames, rename_legacy_ids, id_follows_convention};

pub(crate) use seat::is_false;

/// Strategy used by the merge gate to land a quark's branch onto main.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    #[default]
    FastForward,
    Squash,
    GitHubPr,
}

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
    /// Repo policy for [`crate::Projection::nucleus_index_budget_bytes`] — how big
    /// `.hadron/nucleus/index.md` may grow before it is over budget, in KiB. `Option`
    /// and tolerant of a hand-edited team.json the same way `max_exchanges` is;
    /// absent (or `0`) falls back to the shipped default
    /// (`nucleus_status::BUDGET_BYTES`, 32 KiB). Not a strict enum: the Settings UI
    /// offers a fixed ladder (16/32/64/128), but a hand-edited file with any other
    /// positive value is honoured, not rejected.
    #[serde(default)]
    pub nucleus_index_budget_kb: Option<usize>,
    /// Configured merge strategy for landing quark branches onto base.
    #[serde(default)]
    pub merge_strategy: Option<MergeStrategy>,
    /// Turn watchdog silence limit in seconds (default 1800s / 30m).
    #[serde(default)]
    pub turn_deadline_secs: Option<u64>,
    /// Live activity stale threshold in seconds (default 120s).
    #[serde(default)]
    pub stale_after_secs: Option<i64>,
    /// Whether to automatically prune worktrees on branch merge/abandonment (default true).
    #[serde(default)]
    pub git_auto_prune_worktrees: Option<bool>,
    /// Custom Git author name override for commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_author_name: Option<String>,
    /// Custom Git author email override for commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_author_email: Option<String>,
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

    /// The configured merge strategy for landing quark branches, defaulting to `FastForward`.
    pub fn merge_strategy(&self) -> MergeStrategy {
        self.merge_strategy.unwrap_or_default()
    }

    /// The configured turn silence deadline in seconds, defaulting to 1800 (30m).
    pub fn turn_deadline_secs(&self) -> u64 {
        self.turn_deadline_secs.unwrap_or(30 * 60)
    }

    /// The configured live activity stale threshold in seconds, defaulting to 120s.
    pub fn stale_after_secs(&self) -> i64 {
        self.stale_after_secs.unwrap_or(120)
    }

    /// Whether git worktrees should be automatically pruned on merge/abandonment.
    pub fn git_auto_prune_worktrees(&self) -> bool {
        self.git_auto_prune_worktrees.unwrap_or(true)
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
        if let Some(limit) = ov.energy_limit {
            seat.energy_limit = Some(limit);
        }
        if let Some(deny_skills) = ov.deny_skills.clone() {
            seat.deny_skills = deny_skills;
        }
        if let Some(model_params) = ov.model_params.clone() {
            seat.model_params = model_params;
        }
        seen.insert(ov.id.clone());
        quarks.push(seat);
    }
    Team {
        quarks,
        roster: Vec::new(),
        max_exchanges: repo.max_exchanges.or(global.max_exchanges),
        nucleus_index_budget_kb: repo.nucleus_index_budget_kb.or(global.nucleus_index_budget_kb),
        merge_strategy: repo.merge_strategy.or(global.merge_strategy),
        turn_deadline_secs: repo.turn_deadline_secs.or(global.turn_deadline_secs),
        stale_after_secs: repo.stale_after_secs.or(global.stale_after_secs),
        git_auto_prune_worktrees: repo.git_auto_prune_worktrees.or(global.git_auto_prune_worktrees),
        git_author_name: repo.git_author_name.clone().or_else(|| global.git_author_name.clone()),
        git_author_email: repo.git_author_email.clone().or_else(|| global.git_author_email.clone()),
    }
}
