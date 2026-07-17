//! Provider/agent wizard + settings value types: the descriptors and state enums for
//! connecting a quark to a backing provider ([`AgentDescriptor`], [`AuthMethod`],
//! [`ProviderState`], [`ConfiguredQuark`], [`WizardState`]), the [`SettingsTarget`] the
//! Settings overlay edits, plus the provider-roster and one-shot catalogue-migration
//! helpers. Value + file-I/O work — no `Chamber`.

use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct AgentDescriptor {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct AuthMethod {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum ProviderState {
    NotConnected,
    Connecting,
    NeedsAuth(Vec<AuthMethod>),
    Ready { model: String },
    Failed(String),
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct ConfiguredQuark {
    pub id: String,
    pub transport: String,
    pub state: ProviderState,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum WizardState {
    None,
    PickPreset,
    Connecting(AgentDescriptor, ProviderState),
}

/// Which identity the Settings overlay is currently editing.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum SettingsTarget {
    Providers,
    Human,
    Quark(String),
}

impl SettingsTarget {
    /// The actor key used for identity resolution / prefs lookup.
    pub(super) fn key(&self) -> &str {
        match self {
            SettingsTarget::Providers => "providers",
            SettingsTarget::Human => "human",
            SettingsTarget::Quark(id) => id,
        }
    }
}

/// The Configured Providers rows for a (resolved) team: every adopted quark, id +
/// backing provider + model. Not-adopted catalogue quarks are intentionally absent —
/// they appear as greyed roster rows, not as configured providers.
pub(super) fn configured_providers(team: &Team) -> Vec<ConfiguredQuark> {
    team.quarks
        .iter()
        .map(|seat| ConfiguredQuark {
            id: seat.id.0.clone(),
            transport: seat.provider.clone(),
            state: ProviderState::Ready {
                model: seat.model.clone(),
            },
        })
        .collect()
}

/// Move any legacy full seats in the repo file into the global catalogue and rewrite
/// the repo file as role/state overrides. A no-op when the repo has no legacy seats, so
/// it is safe to call on every launch. The transformation itself is the tested pure
/// [`migrate_to_catalogue`] (in `lattice`, verified byte-identical under `resolve_team`),
/// so a running daemon reconciles this to an empty re-seat rather than a disruptive
/// rebuild; this wrapper only does the file I/O and a one-time safety backup.
pub(super) fn migrate_repo_to_catalogue(repo_path: &std::path::Path, global_path: &std::path::Path) {
    let mut repo = load_team(repo_path);
    if repo.quarks.is_empty() {
        return; // already split (or empty) — nothing to migrate
    }
    // This rewrite is irreversible and touches a live file. Before touching either
    // file, keep a one-time snapshot of the self-contained repo team, so the original
    // seat *definitions* survive even if the global catalogue is later lost or reset
    // (an overrides-only repo with a missing catalogue resolves to an empty swarm).
    let backup = repo_path.with_extension("json.premigration");
    if !backup.exists() {
        if let Err(e) = std::fs::copy(repo_path, &backup) {
            eprintln!("chamber: migration aborted — could not back up repo team: {e}");
            return; // never rewrite the only copy of the defs without a backup
        }
    }
    let mut global = load_team(global_path);
    let moved = repo.quarks.len();
    hadron_lattice::migrate_to_catalogue(&mut repo, &mut global);
    // Catalogue is written FIRST: a daemon that polls mid-migration must never see the
    // new overrides-only repo against the OLD (def-less) catalogue, which would resolve
    // to orphans and drop the swarm. New-catalogue + old-repo only ever resolves to the
    // unchanged roster.
    if let Err(e) = hadron_lattice::save_team(global_path, &global) {
        eprintln!("chamber: migration failed to write catalogue: {e}");
        return; // do NOT rewrite the repo file if the catalogue write failed
    }
    if let Err(e) = hadron_lattice::save_team(repo_path, &repo) {
        eprintln!("chamber: migration failed to write repo team: {e}");
    } else {
        eprintln!(
            "chamber: migrated {moved} seat(s) into the global catalogue; repo now uses \
             overrides (backup at {})",
            backup.display()
        );
    }
}
