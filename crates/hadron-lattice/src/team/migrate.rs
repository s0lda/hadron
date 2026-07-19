use crate::QuarkId;

use super::seat::SeatOverride;
use super::transport::Transport;
use super::{Seat, Team};

/// Split a repo team's legacy full seats out into the global catalogue: each seat's
/// definition is upserted into `global`, and `repo.quarks` is replaced by role/state
/// [`SeatOverride`]s in `repo.roster`. Idempotent — a repo with no legacy seats is
/// left untouched.
///
/// The invariant that makes this safe on a **live** setup: `resolve_team(repo, global)`
/// afterwards is seat-for-seat identical (order included) to `resolve_team(repo, global)`
/// before — the override carries each seat's own `flavor` + `enabled` over its own def —
/// so a running daemon reconciles the split to a no-op re-seat rather than a rebuild.
/// (Proven by `migrate_to_catalogue_is_identity_under_resolve`.)
pub fn migrate_to_catalogue(repo: &mut Team, global: &mut Team) {
    for seat in std::mem::take(&mut repo.quarks) {
        let ov = SeatOverride {
            flavor: Some(seat.flavor.clone()),
            enabled: Some(seat.enabled),
            ..SeatOverride::role(seat.id.clone())
        };
        // Definition → catalogue (upsert by id).
        if let Some(existing) = global.quarks.iter_mut().find(|s| s.id == seat.id) {
            *existing = seat;
        } else {
            global.quarks.push(seat);
        }
        // Role/state → repo override (dedup: a legacy seat and an override for the same
        // id must never coexist after migration).
        if !repo.roster.iter().any(|o| o.id == ov.id) {
            repo.roster.push(ov);
        }
    }
}

/// Express a user's edit of a catalogue-adopted quark as a per-repo **delta** from the
/// catalogue default. `def` is the shared default (from the global catalogue), `desired`
/// is the seat the user wants *here*, and `prev` is any existing role/participation
/// override for this id (preserved). Each definition knob is carried only when it differs
/// from the default — so a knob left at the default inherits it (and stays in step if the
/// default later changes), while the catalogue and every other repo are untouched.
///
/// This is the inverse of the definition-layering in [`resolve_team`]: for any `def`, a
/// repo carrying `seat_override_delta(id, def, desired, prev)` resolves that id back to
/// `desired`. Proven by `a_settings_edit_becomes_a_delta_that_resolves_back_to_the_edit`.
pub fn seat_override_delta(
    id: QuarkId,
    def: &Seat,
    desired: &Seat,
    prev: Option<&SeatOverride>,
) -> SeatOverride {
    SeatOverride {
        flavor: prev.and_then(|o| o.flavor.clone()),
        enabled: prev.and_then(|o| o.enabled),
        model: (desired.model != def.model).then(|| desired.model.clone()),
        effort: (desired.effort != def.effort).then(|| desired.effort.clone()),
        mode_config: (desired.mode_config != def.mode_config).then(|| desired.mode_config.clone()),
        display_name: (desired.display_name != def.display_name)
            .then(|| desired.display_name.clone()),
        roles: (desired.roles != def.roles).then(|| desired.roles.clone()),
        exclusive: (desired.exclusive != def.exclusive).then_some(desired.exclusive),
        ..SeatOverride::role(id)
    }
}

/// The override ids in `repo.roster` that name no legacy seat and no catalogue seat,
/// so [`resolve_team`] drops them. The daemon logs these — a repo pointing at a quark
/// the catalogue no longer defines is a stale reference worth a warning, not a silent
/// disappearance.
pub fn orphan_overrides(repo: &Team, global: &Team) -> Vec<QuarkId> {
    repo.roster
        .iter()
        .filter(|ov| repo.get(&ov.id).is_none() && global.get(&ov.id).is_none())
        .map(|ov| ov.id.clone())
        .collect()
}

/// The one-shot legacy id renames, in one place so every consumer (the team.json pass
/// below and the chamber's ChamberPrefs key move) reads the SAME map. Only the two
/// built-ins that predate the `<transport>-<vendor>` convention; every other id is left
/// alone, so a user's custom id is never surprise-renamed.
pub fn legacy_id_renames() -> &'static [(&'static str, &'static str)] {
    &[("agy", "cli-agy"), ("opus", "cli-claude")]
}

/// Apply [`legacy_id_renames`] to a team in place: both full-seat ids and roster override
/// ids (a roster entry references a catalogue id, so it must move in lockstep). Idempotent
/// — an already-renamed id is not in the map's left column, so a second run changes nothing.
pub fn rename_legacy_ids(team: &mut Team) {
    let rename = |id: &mut QuarkId| {
        if let Some((_, new)) = legacy_id_renames().iter().find(|(old, _)| *old == id.as_str()) {
            *id = QuarkId::new(*new);
        }
    };
    for seat in &mut team.quarks {
        rename(&mut seat.id);
    }
    for ov in &mut team.roster {
        rename(&mut ov.id);
    }
}

/// Soft convention check: does `id` start with its transport prefix (`cli-`, `acp-`, `sdk-`)?
/// Advisory only — used to default new-seat ids and to warn, never to reject (custom ids like
/// `cli-agy-pro` stay legal).
pub fn id_follows_convention(id: &str, transport: Transport) -> bool {
    id.starts_with(&format!("{}-", transport.code()))
}
