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
    /// Short human blurb from the catalogue; empty for best-effort presets (the
    /// wizard falls back to showing the command line).
    pub description: String,
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
    /// The "Custom CLI" form: a generic `Transport::Cli` seat built from a hand-typed
    /// vendor/program/args/model + prompt-channel choice, rather than a probed ACP
    /// preset. Unlike `Connecting`, there is nothing to boot-and-probe here — the
    /// wizard's own `custom_cli_*` input fields on `Chamber` hold the live form state
    /// (see `mod.rs`), so this variant carries no payload of its own.
    CustomCli,
}

/// Which channel the custom-CLI wizard's prompt-delivery toggle currently selects. Drives
/// whether `cli_seat_from` is called with `PromptChannel::Stdin` or
/// `PromptChannel::Arg { flag }` (the flag text comes from `custom_cli_flag`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum CliChannelChoice {
    #[default]
    Stdin,
    Arg,
}

/// Build a `Transport::Cli` [`Seat`] from the custom-CLI wizard's fields. Pure and
/// gpui-free — extracted out of the `on_click` closure specifically so it is
/// unit-testable, per the same reasoning the ACP save path already documents inline
/// (`conventional_id` for the id, `Flavor::Worker` default, `enabled: true`). Mirrors
/// that path's shape but for `Transport::Cli` + a generic [`hadron_lattice::CliSpec`]
/// instead of an `AcpCommand`.
///
/// Unlike the ACP path's `vendor` (a pre-vetted preset key, so its `normalize_vendor()`
/// call is a documented no-op), `vendor` here is raw human-typed text — so any stray
/// `cli-`/`acp-`/`sdk-` prefix is stripped **before** the id is derived, not after. Doing
/// it after (as a `Seat::normalize_vendor()` call on the built seat) would desync `id`
/// from `vendor`: e.g. vendor `"cli-ollama"` would derive `id = "cli-cli-ollama"`, then
/// normalize the *vendor* alone to `"ollama"`, leaving the id still doubled-up.
pub(super) fn cli_seat_from(
    vendor: &str,
    program: &str,
    args: Vec<String>,
    channel: hadron_lattice::PromptChannel,
    model: &str,
) -> hadron_lattice::Seat {
    let mut cli = hadron_lattice::CliSpec::generic(program.to_string(), args);
    cli.prompt = channel;

    let mut seat = hadron_lattice::Seat {
        // Placeholder — replaced below once `vendor` is normalized. `Seat::normalize_vendor`
        // only touches `self.vendor`, so it has to run before the id can be derived from it.
        id: hadron_lattice::QuarkId::new(""),
        display_name: None,
        vendor: vendor.to_string(),
        model: model.to_string(),
        flavor: hadron_lattice::Flavor::Worker, // default flavor, same as the ACP path
        transport: hadron_lattice::Transport::Cli,
        command: None,
        cli: Some(cli),
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
    seat.normalize_vendor();
    // SSOT: the same `<transport>-<vendor>` builder the ACP save path uses, just off
    // `Transport::Cli` instead of `Transport::Acp` — and now off the normalized vendor,
    // so `id` and `vendor` always agree.
    seat.id = hadron_lattice::QuarkId::new(&hadron_lattice::Transport::Cli.conventional_id(&seat.vendor));
    seat
}

/// Whether a hand-typed custom-CLI `vendor` derives an id `hadron_gluon`'s
/// `validate_quark_id` accepts. That function is the SSOT for "safe" (it also gates
/// every other seat-creation path — `build`/`build_seat`/`build_seat_watched`), and
/// this wizard is the FIRST UI surface that feeds freely-typed text into a `QuarkId` —
/// which becomes a worktree DIRECTORY name, a git BRANCH ref segment, and a live-file
/// name (see `validate_quark_id`'s doc comment). Reuses `cli_seat_from` itself (rather
/// than re-deriving the normalize+conventional_id steps here) so this check can never
/// drift from what Save actually produces.
///
/// Also requires the NORMALIZED vendor to be non-empty. A bare transport prefix (e.g.
/// `"cli-"`) or an all-prefix vendor normalizes to `""` via `Seat::normalize_vendor`,
/// which then derives `id = "cli-"` — non-empty and all safe characters, so
/// `validate_quark_id` alone would accept it even though the vendor itself is nothing.
pub(super) fn custom_cli_vendor_is_valid(vendor: &str) -> bool {
    let seat = cli_seat_from(vendor, "placeholder", Vec::new(), hadron_lattice::PromptChannel::Stdin, "");
    !seat.vendor.is_empty() && hadron_gluon::adapter::registry::validate_quark_id(&seat.id).is_ok()
}

/// Parse the Settings "Max exchanges" field into the value it commits onto
/// `Team::max_exchanges` — a **team/repo-wide** policy (not per-quark), the cap on
/// quark↔quark exchanges before the daemon's backstop stops the swarm (`Engine`'s
/// `exchanges >= max_exchanges` check in `hadron-gluon`).
///
/// - Blank (or whitespace-only) → `None`, which clears any repo override and falls back
///   to the daemon's own built-in default (`hadron-gluon.rs`'s `team.max_exchanges.unwrap_or(..)`).
///   This is **not** "unlimited" — every exchange loop is still bounded by that default —
///   so the UI hint must say "daemon default", never "unlimited". This function
///   deliberately does not hardcode that default's numeral: it lives in exactly one
///   place (the daemon bin), and duplicating it here would be a second source of truth
///   that silently drifts if that default ever changes (SSOT, Standard Model rule 3).
/// - `"0"` → `None` too, not `Some(0)`. `Some(0)` would trip the backstop's
///   `exchanges >= max_exchanges` check before a single exchange runs, silently
///   freezing the swarm — a footgun no one clearing the field down to zero would
///   intend. A human who wants a hard stop should type a small positive number
///   instead, not zero.
/// - Any other unparsable text (non-numeric, negative, overflow) is ignored → `None`,
///   same as blank: a garbled edit never wins over the safe default.
/// - A positive integer parses straight through as `Some(n)`.
pub(super) fn parse_max_exchanges(raw: &str) -> Option<usize> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<usize>() {
        Ok(0) => None,
        Ok(n) => Some(n),
        Err(_) => None,
    }
}

/// The custom-CLI wizard's channel-toggle → [`PromptChannel`] mapping. The one bit of
/// this form that isn't a straight field copy: `Arg` with a blank flag field means "the
/// prompt rides as a bare positional argument" (`flag: None`), not "flag unset by
/// mistake". Extracted out of the `on_click` closure (alongside `cli_seat_from`) so this
/// conversion is unit-testable too, not just eyeballed in the wizard.
pub(super) fn prompt_channel_from(choice: CliChannelChoice, flag: &str) -> hadron_lattice::PromptChannel {
    match choice {
        CliChannelChoice::Stdin => hadron_lattice::PromptChannel::Stdin,
        CliChannelChoice::Arg => {
            let flag = flag.trim();
            hadron_lattice::PromptChannel::Arg { flag: (!flag.is_empty()).then(|| flag.to_string()) }
        }
    }
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

/// Default env-var name the per-quark API-key field offers when a seat has not
/// declared any `secret_env` yet (the common case: Antigravity's `GEMINI_API_KEY`).
pub(super) const DEFAULT_SECRET_VAR: &str = "GEMINI_API_KEY";

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

    /// The value-level derivation the custom-CLI wizard's "Save" button calls: a
    /// stdin-prompt seat, the common case (Ollama, a local script, etc.).
    #[test]
    fn cli_seat_from_builds_a_stdin_cli_transport_seat() {
        let seat = cli_seat_from(
            "ollama",
            "ollama",
            vec!["run".to_string(), "llama3".to_string()],
            hadron_lattice::PromptChannel::Stdin,
            "llama3",
        );

        assert_eq!(seat.transport, hadron_lattice::Transport::Cli);
        assert_eq!(seat.id.as_str(), "cli-ollama", "SSOT id via Transport::conventional_id");
        assert!(seat.command.is_none(), "command is the ACP field — a CLI seat leaves it None");
        assert_eq!(seat.vendor, "ollama");
        assert_eq!(seat.model, "llama3");
        assert_eq!(seat.flavor, hadron_lattice::Flavor::Worker);
        assert!(seat.enabled);
        assert!(seat.effort.is_none());
        assert!(seat.mode_config.is_none());

        let cli = seat.cli.expect("cli spec must be Some for a custom-CLI seat");
        assert_eq!(cli.program, "ollama");
        assert_eq!(cli.args, vec!["run".to_string(), "llama3".to_string()]);
        assert_eq!(cli.prompt, hadron_lattice::PromptChannel::Stdin);

        // Advisory check the ACP path also runs before saving — must hold for a
        // freshly-derived id.
        assert!(hadron_lattice::id_follows_convention(seat.id.as_str(), seat.transport));
    }

    /// The other prompt channel: the flag-argument choice, and a bare program with no
    /// static args — proves the flag (not just Stdin) round-trips into the `cli` spec.
    #[test]
    fn cli_seat_from_arg_channel_carries_the_flag() {
        let seat = cli_seat_from(
            "myclitool",
            "/usr/local/bin/myclitool",
            vec![],
            hadron_lattice::PromptChannel::Arg { flag: Some("--prompt".to_string()) },
            "",
        );

        assert_eq!(seat.id.as_str(), "cli-myclitool");
        let cli = seat.cli.expect("cli spec must be Some");
        assert_eq!(cli.program, "/usr/local/bin/myclitool");
        assert!(cli.args.is_empty());
        assert_eq!(
            cli.prompt,
            hadron_lattice::PromptChannel::Arg { flag: Some("--prompt".to_string()) }
        );
    }

    /// The bug the doc comment on `cli_seat_from` calls out: a hand-typed vendor that
    /// already carries a transport prefix must not desync `id` from `vendor` — the
    /// normalize has to happen before the id is derived, not after.
    #[test]
    fn cli_seat_from_normalizes_a_stray_transport_prefix_before_deriving_the_id() {
        let seat = cli_seat_from(
            "cli-ollama",
            "ollama",
            vec![],
            hadron_lattice::PromptChannel::Stdin,
            "",
        );

        assert_eq!(seat.vendor, "ollama", "normalize_vendor must strip the stray prefix");
        assert_eq!(seat.id.as_str(), "cli-ollama", "id derived from the NORMALIZED vendor");
        assert!(hadron_lattice::id_follows_convention(seat.id.as_str(), seat.transport));
    }

    /// The bug this whole review round closes: a vendor with a slash would derive an id
    /// like `"cli-foo/bar"`, which becomes a nested (broken) worktree directory and a
    /// malformed git branch ref segment. Must be rejected before Save is ever wired up.
    #[test]
    fn custom_cli_vendor_is_valid_rejects_a_slash() {
        assert!(!custom_cli_vendor_is_valid("foo/bar"));
    }

    /// A space would derive a whitespace-containing id — already rejected by
    /// `validate_quark_id`'s original check, but proven here too since this helper is
    /// what the wizard actually calls.
    #[test]
    fn custom_cli_vendor_is_valid_rejects_a_space() {
        assert!(!custom_cli_vendor_is_valid("my tool"));
    }

    /// A colon: invalid in a git ref, and a path separator on Windows.
    #[test]
    fn custom_cli_vendor_is_valid_rejects_a_colon() {
        assert!(!custom_cli_vendor_is_valid("my:tool"));
    }

    /// The common, unremarkable case must still work — this is a gate, not a trap.
    #[test]
    fn custom_cli_vendor_is_valid_accepts_a_normal_vendor() {
        assert!(custom_cli_vendor_is_valid("ollama"));
        assert!(custom_cli_vendor_is_valid("cli_tool.v2"));
    }

    /// A human who types a bare transport prefix (or nothing at all) as the vendor gets
    /// `Seat::normalize_vendor` stripping it down to `""` — `cli_seat_from` would then
    /// derive `id = "cli-"` (non-empty, all-safe-characters, so `validate_quark_id` alone
    /// waves it through) with an EMPTY vendor. Must still be rejected: an empty vendor is
    /// not a usable label, however "safe" the resulting id string looks.
    #[test]
    fn custom_cli_vendor_is_valid_rejects_a_bare_transport_prefix() {
        assert!(!custom_cli_vendor_is_valid("cli-"), "normalizes to an empty vendor");
        assert!(!custom_cli_vendor_is_valid(""), "empty vendor outright");
    }

    /// `prompt_channel_from`: the Stdin choice ignores whatever the flag field holds —
    /// it isn't read in that branch at all.
    #[test]
    fn prompt_channel_from_stdin_choice_ignores_the_flag_field() {
        assert_eq!(
            prompt_channel_from(CliChannelChoice::Stdin, "--ignored"),
            hadron_lattice::PromptChannel::Stdin
        );
    }

    /// The blank-flag case: a human who picks "Argv flag" but leaves the flag box empty
    /// gets a positional argument, not a broken `--` flag.
    #[test]
    fn prompt_channel_from_arg_choice_blank_flag_is_positional() {
        assert_eq!(
            prompt_channel_from(CliChannelChoice::Arg, "   "),
            hadron_lattice::PromptChannel::Arg { flag: None }
        );
    }

    #[test]
    fn prompt_channel_from_arg_choice_carries_a_nonblank_flag() {
        assert_eq!(
            prompt_channel_from(CliChannelChoice::Arg, "--prompt"),
            hadron_lattice::PromptChannel::Arg { flag: Some("--prompt".to_string()) }
        );
    }

    // -- parse_max_exchanges: the Settings "Max exchanges" field's value-level parse --

    #[test]
    fn parse_max_exchanges_blank_is_none() {
        assert_eq!(parse_max_exchanges(""), None);
        assert_eq!(parse_max_exchanges("   "), None);
    }

    #[test]
    fn parse_max_exchanges_zero_is_none_not_some_zero() {
        // Some(0) would trip the engine's `exchanges >= max_exchanges` backstop before a
        // single exchange runs — a footgun. Zero clears the override instead, same as blank.
        assert_eq!(parse_max_exchanges("0"), None);
    }

    #[test]
    fn parse_max_exchanges_positive_integer_round_trips() {
        assert_eq!(parse_max_exchanges("50"), Some(50));
        assert_eq!(parse_max_exchanges("1"), Some(1));
    }

    #[test]
    fn parse_max_exchanges_trims_surrounding_whitespace() {
        assert_eq!(parse_max_exchanges("  12  "), Some(12));
    }

    #[test]
    fn parse_max_exchanges_invalid_text_is_none() {
        assert_eq!(parse_max_exchanges("abc"), None);
        assert_eq!(parse_max_exchanges("-5"), None, "negative — usize can't parse it");
        assert_eq!(parse_max_exchanges("3.5"), None);
    }
}
