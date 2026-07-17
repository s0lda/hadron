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

/// Backs the ACP model **dropdown** in a quark's Settings. The chamber re-probes the
/// agent each time an ACP quark is opened (see `start_acp_model_probe`) and parks the
/// result here, keyed by quark `id` so a probe that lands after the human has moved on
/// can't populate another quark's list.
pub(super) struct AcpModelProbe {
    pub id: String,
    pub state: AcpModelState,
}

pub(super) enum AcpModelState {
    /// Booting the agent to read back the models it advertises on `session/new`.
    Probing,
    /// The agent's offered models (wire value + label) and its current/default pick.
    Ready {
        models: Vec<hadron_gluon::adapter::acp::AcpModel>,
        current: String,
    },
    /// The agent could not be probed, or advertises no model picker at all. The string
    /// is a short human note; the dropdown still offers "Default" (the agent's choice).
    Unavailable(String),
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
            transport: seat.transport.code().to_string(),
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

/// One-shot: rename legacy ids to the `<transport>-<vendor>` convention across the repo
/// team, the global catalogue, and the chamber's per-quark identity — all off the single
/// `legacy_id_renames` map. Idempotent; safe to call every launch.
///
/// Writes the catalogue (global) first, then the repo, matching the write order
/// [`migrate_repo_to_catalogue`] uses — so the two migrations are consistent for the next
/// maintainer to reason about. This does NOT make the pair atomic: on an overrides-only
/// repo, a repo-file override references a catalogue seat *by id*, so renaming the two
/// files in either order leaves a one-poll-tick window where the id in one file doesn't
/// match the id in the other yet, and `resolve_team` drops the override as an orphan
/// (logged, not fatal) until the second write lands. That window is inherent to a
/// cross-file id-rename of an overrides-only repo, not something a write-order choice can
/// close; it self-heals on the very next resolve, and this only runs at launch, before any
/// daemon is polling the files — so in practice nothing observes the gap.
pub(super) fn migrate_legacy_ids(
    repo_path: &std::path::Path,
    global_path: &std::path::Path,
    prefs: &mut ChamberPrefs,
) {
    for path in [global_path, repo_path] {
        let mut team = load_team(path);
        let before = team.clone();
        hadron_lattice::rename_legacy_ids(&mut team);
        if team != before {
            if let Err(e) = hadron_lattice::save_team(path, &team) {
                eprintln!("chamber: legacy id-rename failed to write {}: {e}", path.display());
            }
        }
    }
    prefs.rename_quark_ids(hadron_lattice::legacy_id_renames());
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Flavor, QuarkId, Seat};
    use tempfile::tempdir;

    /// The bug this task fixes: `configured_providers` used to feed `ConfiguredQuark`'s
    /// `transport` field the seat's *vendor* (a Task 1 stopgap), so the Settings "Transport:"
    /// label showed "claude"/"agy" instead of "acp"/"cli". Pins the real transport code per
    /// seat, distinct from — and not equal to — its vendor, across all three transports.
    #[test]
    fn configured_providers_reports_the_real_transport_not_the_vendor() {
        let team = Team {
            quarks: vec![
                Seat::cli(QuarkId::new("cli-agy"), "agy", "gemini-3-pro", Flavor::Worker),
                Seat {
                    transport: hadron_lattice::Transport::Acp,
                    ..Seat::cli(QuarkId::new("acp-claude"), "claude", "opus-4.8", Flavor::Orchestrator)
                },
                Seat {
                    transport: hadron_lattice::Transport::Sdk,
                    ..Seat::cli(QuarkId::new("sdk-codex"), "codex", "gpt-5", Flavor::Worker)
                },
            ],
            roster: vec![],
            max_exchanges: None,
        };

        let providers = configured_providers(&team);
        assert_eq!(providers.len(), 3);

        let cli = providers.iter().find(|p| p.id == "cli-agy").unwrap();
        assert_eq!(cli.transport, "cli", "the transport code, not the vendor \"agy\"");
        let acp = providers.iter().find(|p| p.id == "acp-claude").unwrap();
        assert_eq!(acp.transport, "acp", "the transport code, not the vendor \"claude\"");
        let sdk = providers.iter().find(|p| p.id == "sdk-codex").unwrap();
        assert_eq!(sdk.transport, "sdk", "the transport code, not the vendor \"codex\"");

        for (p, model) in [(cli, "gemini-3-pro"), (acp, "opus-4.8"), (sdk, "gpt-5")] {
            match &p.state {
                ProviderState::Ready { model: m } => assert_eq!(m, model),
                _ => panic!("expected ProviderState::Ready"),
            }
        }
    }

    /// The composition `migrate_legacy_ids` exists for: a legacy id in BOTH team files
    /// moves in lockstep with the ChamberPrefs identity keyed on it, so a rename never
    /// resets a quark's colour/name/avatar in one file while leaving it stranded in
    /// another. `rename_legacy_ids` and `ChamberPrefs::rename_quark_ids` are unit-tested
    /// individually (lattice, config.rs); this proves the launch-time glue that runs
    /// them together against real files.
    #[test]
    fn migrate_legacy_ids_moves_team_files_and_prefs_together() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo_team.json");
        let global_path = dir.path().join("global_team.json");

        let mut repo = Team::default();
        repo.quarks.push(Seat::cli(QuarkId::new("agy"), "agy", "gemini-3-pro", Flavor::Worker));
        hadron_lattice::save_team(&repo_path, &repo).unwrap();

        let mut global = Team::default();
        global.quarks.push(Seat::cli(QuarkId::new("opus"), "claude", "opus", Flavor::Orchestrator));
        hadron_lattice::save_team(&global_path, &global).unwrap();

        let mut prefs = ChamberPrefs::default();
        prefs.quarks.insert("agy".to_string(), Identity::default());

        migrate_legacy_ids(&repo_path, &global_path, &mut prefs);

        let repo_after = load_team(&repo_path);
        assert!(repo_after.get(&QuarkId::new("cli-agy")).is_some(), "repo seat renamed");
        assert!(repo_after.get(&QuarkId::new("agy")).is_none(), "old repo id gone");

        let global_after = load_team(&global_path);
        assert!(global_after.get(&QuarkId::new("cli-claude")).is_some(), "catalogue seat renamed");

        assert!(prefs.quarks.contains_key("cli-agy"), "identity followed the rename");
        assert!(!prefs.quarks.contains_key("agy"));

        // Idempotent: a second pass over the already-renamed files/prefs is a no-op.
        migrate_legacy_ids(&repo_path, &global_path, &mut prefs);
        assert!(prefs.quarks.contains_key("cli-agy"));
        assert!(load_team(&repo_path).get(&QuarkId::new("cli-agy")).is_some());
    }
}
