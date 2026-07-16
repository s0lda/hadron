//! The team roster config: the seats the human has added. A **seat** is one
//! quark instance = an id bound to a provider (backing CLI) running a model.
//! Stored as `team.json` and read by both the daemon (to instantiate adapters)
//! and the chamber (to make each roster row legible: `id · provider · model`).
//!
//! Pure and offline: this only parses the config. Spawning adapters from it is
//! the daemon's job; annotating the roster is the chamber's.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Flavor, QuarkId};

/// How the gluon *talks to* a seat's agent. Two transports, one seam:
///
/// - [`Transport::Cli`] — the original: one `claude`/`agy` subprocess per turn,
///   whole prompt in, stdout out. Every seat written before ACP existed uses it,
///   which is exactly why it is the [`Default`]: an existing `team.json` that has
///   never heard of a `transport` key keeps resolving to the CLI adapters,
///   byte-for-byte.
/// - [`Transport::Acp`] — JSON-RPC over stdio to an [Agent Client Protocol] agent.
///   The seat names an ACP agent binary (or takes one of the known shorthands);
///   the gluon speaks the protocol's *client* side.
///
/// This is a **config** switch, deliberately, not an inference from `provider`:
/// `claude` the CLI and `claude` over an ACP adapter are the same vendor reached
/// two different ways, and a seat must say which one it means.
///
/// [Agent Client Protocol]: https://agentclientprotocol.com/
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// One-shot CLI subprocess per turn. The default, forever, for `agy`.
    #[default]
    Cli,
    /// A resident ACP agent spoken to over JSON-RPC on stdio.
    Acp,
}

/// How to boot an ACP agent: the program and its args. Comes
/// straight out of `team.json`, so reaching a *new* ACP agent is a config change
/// rather than a code change — which is the entire point of standing on a
/// protocol instead of a vendor's CLI.
///
/// Only read when [`Seat::transport`] is [`Transport::Acp`]. When absent, the
/// gluon resolves a default command from the seat's `provider` (e.g.
/// `acp-claude` → `npx -y @agentclientprotocol/claude-agent-acp@latest`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpCommand {
    /// The executable to spawn, e.g. `npx` or an absolute path to an agent binary.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// One seat: an identity bound to a provider (CLI/vendor) and a model. Same
/// provider with a different model is a different seat (independent trust).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub id: QuarkId,
    /// The human-readable name of the quark, e.g. "Google Girl".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The backing CLI/vendor, e.g. "claude", "agy".
    pub provider: String,
    /// The model this seat runs, e.g. "opus-4.8", "gemini-3-pro".
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_config: Option<String>,
    pub flavor: Flavor,
    /// How the gluon reaches this seat's agent. Absent → [`Transport::Cli`], so
    /// every `team.json` written before ACP existed keeps its exact behaviour.
    #[serde(default)]
    pub transport: Transport,
    /// The ACP agent to boot. Ignored unless `transport` is [`Transport::Acp`];
    /// absent there means "resolve the command from `provider`".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<AcpCommand>,
    /// Whether this seat **participates**. A disabled quark keeps its seat, its
    /// identity, and — crucially — its live instance: it is simply never excited.
    ///
    /// Defaults to `true`, so every `team.json` written before this field existed
    /// keeps its exact behaviour without being rewritten.
    ///
    /// This is participation, NOT existence. Disabling is not unseating: an ACP seat
    /// holds a resident subprocess and a conversation, and tearing that down to flip a
    /// boolean would throw away the session for nothing. See [`Seat::same_agent`],
    /// which is what stops the re-seat planner from mistaking a toggle for a rebuild.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

/// `true`. Serde needs a function, and an absent `enabled` must mean *on* — a seat
/// that predates this field was never disabled.
fn enabled_by_default() -> bool {
    true
}

impl Seat {
    /// Are these two seats **the same agent**, ignoring whether it is switched on?
    ///
    /// This is deliberately *not* `==`. The re-seat planner treats any difference
    /// between the running seat and the desired one as "this is a different agent, tear
    /// it down and build the new one" — which is right for a model change, a provider
    /// change, or a re-pointed ACP binary. It is exactly **wrong** for `enabled`:
    /// flipping a boolean would rebuild a resident ACP quark and silently discard its
    /// conversation.
    ///
    /// So `enabled` is the one field carved out of the identity comparison, and a
    /// change to it produces a *toggle* rather than a *replace*. Every other field
    /// still forces a rebuild, by construction: this destructures the struct, so adding
    /// a field to `Seat` without deciding which side of this line it falls on will not
    /// compile.
    pub fn same_agent(&self, other: &Seat) -> bool {
        let Seat { id, display_name: _, provider, model, flavor, transport, command, enabled: _, effort, mode_config } = self;
        id == &other.id
            && provider == &other.provider
            && model == &other.model
            && flavor == &other.flavor
            && transport == &other.transport
            && command == &other.command
            && effort == &other.effort
            && mode_config == &other.mode_config
    }

    /// A CLI seat — the shape every seat had before ACP. Keeps construction sites
    /// (and tests) from having to spell out two ACP fields they do not care about.
    pub fn cli(id: QuarkId, provider: impl Into<String>, model: impl Into<String>, flavor: Flavor) -> Seat {
        Seat {
            id,
            display_name: None,
            provider: provider.into(),
            model: model.into(),
            flavor,
            transport: Transport::Cli,
            command: None,
            enabled: true,
            effort: None,
            mode_config: None,
        }
    }
}

/// A per-repo override of one catalogue seat: which *role* it plays here and
/// whether it participates — **without** re-stating the seat's definition. The
/// definition (provider/model/command/transport/effort) lives once in the global
/// catalogue (`~/.hadron/team.json`); a repo names a quark by `id` and says only
/// "here it is the orchestrator" or "here it is off". Both override fields are
/// optional: an absent one inherits the catalogue's value.
///
/// This is what "different orchestrator per repo" costs: a two-field row, not a
/// duplicated seat. The full [`Seat`] is still supported in [`Team::quarks`] for
/// backward compatibility (and for a self-contained team that wants no catalogue),
/// so nothing that predates this type has to change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatOverride {
    pub id: QuarkId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flavor: Option<Flavor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
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
        seen.insert(ov.id.clone());
        quarks.push(seat);
    }
    Team {
        quarks,
        roster: Vec::new(),
        max_exchanges: repo.max_exchanges,
    }
}

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
            id: seat.id.clone(),
            flavor: Some(seat.flavor.clone()),
            enabled: Some(seat.enabled),
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

/// The user's home directory, cross-platform: `$HOME` on Unix, `%USERPROFILE%`
/// on Windows. `None` if neither is set.
fn home_dir() -> Option<PathBuf> {
    for var in ["HOME", "USERPROFILE"] {
        if let Some(v) = std::env::var_os(var) {
            if !v.is_empty() {
                return Some(PathBuf::from(v));
            }
        }
    }
    None
}

/// The user-level Hadron directory: `~/.hadron` (i.e. `<home>/.hadron`), the same
/// dot-folder convention as a project's `.hadron/`. Cross-platform. `None` if the
/// home directory can't be resolved. All global Hadron state (chamber prefs, the
/// default team) lives here.
pub fn user_hadron_dir() -> Option<PathBuf> {
    Some(home_dir()?.join(".hadron"))
}

/// The canonical global `team.json` location: `~/.hadron/team.json`. Both the
/// daemon (to seat quarks when no project team is found) and the chamber (to
/// annotate the roster) resolve the same file here.
pub fn team_config_path() -> Option<PathBuf> {
    Some(user_hadron_dir()?.join("team.json"))
}

/// Resolve which `team.json` describes the team working a given field: the
/// project-level `team.json` sitting next to the field (the `.hadron/` convention)
/// if present, else the global `~/.hadron/team.json`. Both the daemon (to seat)
/// and the chamber (to annotate the roster) must resolve the SAME team, so they
/// share this — otherwise the chamber shows legibility for a team the daemon
/// never seated.
pub fn team_for_field(field_path: &Path) -> Option<PathBuf> {
    if let Some(sibling) = field_path.parent().map(|d| d.join("team.json")) {
        if sibling.exists() {
            return Some(sibling);
        }
    }
    // Fall back to walking up the directory tree looking for a project `.hadron/team.json`.
    let mut current = field_path.parent();
    while let Some(dir) = current {
        let repo_config = dir.join(".hadron").join("team.json");
        if repo_config.exists() {
            return Some(repo_config);
        }
        current = dir.parent();
    }
    team_config_path()
}

/// Parse a team from JSON text, **keeping the error**.
///
/// The one parser. The daemon re-reads `team.json` while the swarm is live and must
/// tell "the human seated nobody" apart from "I could not parse this" — a distinction
/// [`load_team`] cannot make, since it maps both to an empty team. If a malformed read
/// answered "empty team", a `team.json` caught mid-write would silently unseat the
/// entire swarm.
///
/// It takes text rather than a path on purpose: the daemon detects change by comparing
/// the file's bytes, and must parse *those* bytes. Re-reading the path to parse it
/// would be a second read of a file that may have changed in between.
pub fn parse_team(text: &str) -> std::io::Result<Team> {
    serde_json::from_str(text).map_err(std::io::Error::other)
}

/// Load a team from an explicit path. Missing or malformed → an empty team, so
/// a fresh install (or a viewer with no config) degrades to "no annotations"
/// rather than an error.
///
/// The lossy wrapper over [`parse_team`]: one parser, two error policies.
pub fn load_team(path: &Path) -> Team {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_team(&text).unwrap_or_default(),
        Err(_) => Team::default(),
    }
}

/// Save a team to an explicit path, creating the directory if it does not exist.
/// The chamber calls this when the human seats a provider in Settings.
///
/// The write is **atomic**: the JSON goes to a temp file in the *same directory*
/// (rename is only atomic within one filesystem) and is then renamed over the
/// target. A plain `fs::write` truncates first, so a reader — and the daemon now
/// polls this file to re-seat the live swarm — can catch it empty or half-written.
/// [`try_load_team`] would reject that torn read anyway; this stops it happening at
/// all. Both layers stay: the parse guard is what makes a *crashed* save safe, and
/// the rename is what makes a *concurrent* save safe.
pub fn save_team(path: &Path, team: &Team) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(team).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seat(id: &str, provider: &str, model: &str, flavor: Flavor) -> Seat {
        Seat::cli(QuarkId::new(id), provider, model, flavor)
    }

    /// `parse_team` keeps the error where `load_team` swallows it. The daemon re-reads
    /// this file while the swarm is live: it MUST be able to tell "the human seated
    /// nobody" (apply it) from "I cannot parse this" (keep the running roster).
    #[test]
    fn parse_team_distinguishes_malformed_from_empty() {
        assert!(parse_team("{ not json").is_err(), "malformed must be an error");
        assert_eq!(
            parse_team("{\"quarks\":[]}").unwrap(),
            Team::default(),
            "an explicitly empty team is valid, and is NOT an error"
        );
    }

    /// The lossy loader still degrades a malformed file to an empty team — the old
    /// behaviour, which the chamber and a fresh install rely on. Both policies exist on
    /// purpose; this pins the one that must not change.
    #[test]
    fn load_team_still_degrades_malformed_to_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("team.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_team(&path), Team::default());
    }

    /// The save is atomic: it must leave no temp file behind, and the target must
    /// contain the whole document. (A `fs::write` truncates in place, so a concurrent
    /// reader — the daemon now polls this file — could catch it empty.)
    #[test]
    fn save_team_is_atomic_and_leaves_no_litter() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("team.json");
        let team = Team {
            quarks: vec![seat("opus", "claude", "opus", Flavor::Orchestrator)],
            roster: vec![],
            max_exchanges: None,
        };
        save_team(&path, &team).unwrap();

        assert_eq!(load_team(&path), team, "the saved team must round-trip");
        let litter: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "team.json")
            .collect();
        assert!(litter.is_empty(), "the temp file was left behind: {litter:?}");
    }

    /// Overwriting an existing team must also be atomic — the rename replaces the file
    /// in one step rather than truncating the one a reader may be holding.
    #[test]
    fn save_team_overwrites_an_existing_file_in_one_step() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("team.json");
        save_team(&path, &Team { quarks: vec![seat("a", "claude", "m", Flavor::Worker)], roster: vec![], max_exchanges: None }).unwrap();

        let two = Team {
            quarks: vec![
                seat("a", "claude", "m", Flavor::Worker),
                seat("b", "agy", "g", Flavor::Worker),
            ],
            roster: vec![],
            max_exchanges: None,
        };
        save_team(&path, &two).unwrap();
        assert_eq!(load_team(&path), two);
    }

    /// THE forward-compat guarantee of the ACP work: a `team.json` written before
    /// the transport seam existed — no `transport` key anywhere — still parses, and
    /// every seat in it still resolves to the CLI transport. If this ever flips,
    /// every existing team silently changes how it is driven.
    #[test]
    fn a_seat_without_a_transport_key_is_still_cli() {
        let old = r#"{"quarks":[
            {"id":"opus","provider":"claude","model":"opus-4.8","flavor":"orchestrator"},
            {"id":"agy","provider":"agy","model":"gemini-3-pro","flavor":"worker"}
        ]}"#;
        let team: Team = serde_json::from_str(old).unwrap();
        assert_eq!(team.quarks.len(), 2);
        assert!(team.quarks.iter().all(|s| s.transport == Transport::Cli));
        assert!(team.quarks.iter().all(|s| s.command.is_none()));
    }

    #[test]
    fn an_acp_seat_parses_its_command() {
        let cfg = r#"{"quarks":[{
            "id":"acp","provider":"acp-claude","model":"opus-4.8","flavor":"worker",
            "transport":"acp",
            "command":{"program":"npx","args":["-y","@agentclientprotocol/claude-agent-acp@latest"]}
        }]}"#;
        let team: Team = serde_json::from_str(cfg).unwrap();
        let s = &team.quarks[0];
        assert_eq!(s.transport, Transport::Acp);
        let cmd = s.command.as_ref().unwrap();
        assert_eq!(cmd.program, "npx");
        assert_eq!(cmd.args[1], "@agentclientprotocol/claude-agent-acp@latest");
    }

    /// A CLI seat must not start *emitting* ACP keys either: a round-trip through
    /// serde has to leave an old team.json looking like an old team.json, or the
    /// chamber rewrites the human's config with fields they never asked for.
    #[test]
    fn a_cli_seat_serializes_without_an_acp_command() {
        let json = serde_json::to_string(&seat("agy", "agy", "g", Flavor::Worker)).unwrap();
        assert!(!json.contains("command"), "no empty ACP command: {json}");
        // `transport` does serialize (it is a plain enum with a default), and it
        // round-trips to the same seat either way.
        let back: Seat = serde_json::from_str(&json).unwrap();
        assert_eq!(back.transport, Transport::Cli);
    }

    #[test]
    fn team_round_trips() {
        let team = Team {
            quarks: vec![
                seat("opus", "claude", "opus-4.8", Flavor::Orchestrator),
                seat("agy", "agy", "gemini-3-pro", Flavor::Worker),
            ],
            roster: vec![],
            max_exchanges: None,
        };
        let json = serde_json::to_string(&team).unwrap();
        let back: Team = serde_json::from_str(&json).unwrap();
        assert_eq!(team, back);
    }

    #[test]
    fn lookup_finds_a_seat_by_id() {
        let team = Team { quarks: vec![seat("agy", "agy", "gemini-3-pro", Flavor::Worker)], roster: vec![], max_exchanges: None };
        let s = team.get(&QuarkId::new("agy")).unwrap();
        assert_eq!(s.provider, "agy");
        assert_eq!(s.model, "gemini-3-pro");
        assert!(team.get(&QuarkId::new("nope")).is_none());
    }

    #[test]
    fn missing_or_malformed_file_is_an_empty_team() {
        let dir = tempdir().unwrap();
        assert!(load_team(&dir.path().join("nope.json")).is_empty());
        let bad = dir.path().join("team.json");
        std::fs::write(&bad, "{ not json").unwrap();
        assert!(load_team(&bad).is_empty());
    }

    #[test]
    fn team_for_field_prefers_the_sibling_then_global() {
        let dir = tempdir().unwrap();
        let hadron = dir.path().join(".hadron");
        std::fs::create_dir_all(&hadron).unwrap();
        let field = hadron.join("field.jsonl");
        // No sibling team.json yet → falls back to the global path (env-dependent,
        // but never the sibling).
        let sibling = hadron.join("team.json");
        assert_ne!(team_for_field(&field), Some(sibling.clone()));
        // Once a sibling exists, it wins.
        std::fs::write(&sibling, "{}").unwrap();
        assert_eq!(team_for_field(&field), Some(sibling));
    }

    #[test]
    fn global_paths_live_under_user_hadron_dir() {
        // Whatever the home resolves to in this env, ~/.hadron is the root and
        // the global team.json sits directly inside it.
        if let Some(dir) = user_hadron_dir() {
            assert!(dir.ends_with(".hadron"), "user dir is <home>/.hadron");
            assert_eq!(team_config_path(), Some(dir.join("team.json")));
        }
    }

    #[test]
    fn tolerates_unknown_keys_like_the_template_note() {
        // The shipped team.example.json carries a leading "_note" comment key.
        // A silent parse failure would degrade to an empty team (mock quarks), so
        // pin that the extra key is ignored and the quarks still load.
        let with_note = r#"{
            "_note": "provider = backing CLI; agy model is a display name",
            "quarks": [
                { "id": "opus", "provider": "claude", "model": "opus", "flavor": "orchestrator" },
                { "id": "agy",  "provider": "agy",    "model": "Gemini 3.1 Pro (High)", "flavor": "worker" }
            ]
        }"#;
        let team: Team = serde_json::from_str(with_note).unwrap();
        assert_eq!(team.quarks.len(), 2);
        assert_eq!(team.get(&QuarkId::new("agy")).unwrap().model, "Gemini 3.1 Pro (High)");
    }

    #[test]
    fn loads_a_written_team() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("team.json");
        std::fs::write(
            &path,
            r#"{"quarks":[{"id":"opus","provider":"claude","model":"opus-4.8","flavor":"orchestrator"}]}"#,
        )
        .unwrap();
        let team = load_team(&path);
        assert_eq!(team.quarks.len(), 1);
        assert_eq!(team.get(&QuarkId::new("opus")).unwrap().model, "opus-4.8");
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    fn seat(id: &str, provider: &str, model: &str, flavor: Flavor) -> Seat {
        Seat::cli(QuarkId::new(id), provider, model, flavor)
    }

    /// THE backward-compat guarantee: a repo team that uses only the legacy `quarks`
    /// array (no overrides) resolves to itself, whatever the catalogue holds. Every
    /// existing team.json keeps its exact behaviour.
    #[test]
    fn a_legacy_only_team_resolves_to_itself() {
        let repo = Team {
            quarks: vec![
                seat("opus", "claude", "opus", Flavor::Orchestrator),
                seat("agy", "agy", "gemini", Flavor::Worker),
            ],
            roster: vec![],
            max_exchanges: Some(7),
        };
        // A catalogue with *different* defs for the same ids must not leak in.
        let global = Team {
            quarks: vec![seat("opus", "claude", "SONNET-NOT-THIS", Flavor::Worker)],
            roster: vec![],
            max_exchanges: Some(999),
        };
        let resolved = resolve_team(&repo, &global);
        assert_eq!(resolved.quarks, repo.quarks, "legacy seats kept verbatim");
        assert!(resolved.roster.is_empty());
        assert_eq!(resolved.max_exchanges, Some(7), "repo policy is authoritative");
    }

    /// An override pulls its definition from the catalogue and applies the per-repo
    /// role/state on top.
    #[test]
    fn an_override_resolves_its_definition_from_the_catalogue() {
        let global = Team {
            quarks: vec![seat("acp-claude", "acp-claude", "opus", Flavor::Worker)],
            roster: vec![],
            max_exchanges: None,
        };
        let repo = Team {
            quarks: vec![],
            roster: vec![SeatOverride {
                id: QuarkId::new("acp-claude"),
                flavor: Some(Flavor::Orchestrator),
                enabled: Some(false),
            }],
            max_exchanges: None,
        };
        let resolved = resolve_team(&repo, &global);
        assert_eq!(resolved.quarks.len(), 1);
        let s = &resolved.quarks[0];
        assert_eq!(s.provider, "acp-claude", "definition comes from the catalogue");
        assert_eq!(s.model, "opus");
        assert_eq!(s.flavor, Flavor::Orchestrator, "repo overrides the role");
        assert!(!s.enabled, "repo overrides the state");
    }

    /// Absent override fields inherit the catalogue's values.
    #[test]
    fn an_override_inherits_catalogue_values_when_unset() {
        let global = Team {
            quarks: vec![Seat {
                enabled: false,
                ..seat("q", "claude", "opus", Flavor::Orchestrator)
            }],
            roster: vec![],
            max_exchanges: None,
        };
        let repo = Team {
            quarks: vec![],
            roster: vec![SeatOverride { id: QuarkId::new("q"), flavor: None, enabled: None }],
            max_exchanges: None,
        };
        let s = &resolve_team(&repo, &global).quarks[0];
        assert_eq!(s.flavor, Flavor::Orchestrator, "inherits catalogue role");
        assert!(!s.enabled, "inherits catalogue state");
    }

    /// An override naming an id the catalogue does not define is dropped — a
    /// role/state with no definition is not a seatable quark, so it can never reach
    /// the daemon. `orphan_overrides` surfaces it for a warning.
    #[test]
    fn an_orphan_override_is_dropped_and_reported() {
        let global = Team::default();
        let repo = Team {
            quarks: vec![],
            roster: vec![SeatOverride {
                id: QuarkId::new("ghost"),
                flavor: None,
                enabled: Some(true),
            }],
            max_exchanges: None,
        };
        assert!(resolve_team(&repo, &global).quarks.is_empty(), "orphan is not seated");
        assert_eq!(orphan_overrides(&repo, &global), vec![QuarkId::new("ghost")]);
    }

    /// A legacy full seat wins over an override with the same id (self-contained,
    /// highest precedence), and the id is not seated twice.
    #[test]
    fn a_legacy_seat_wins_over_an_override_of_the_same_id() {
        let global = Team {
            quarks: vec![seat("dup", "acp-claude", "CATALOGUE", Flavor::Worker)],
            roster: vec![],
            max_exchanges: None,
        };
        let repo = Team {
            quarks: vec![seat("dup", "claude", "LEGACY", Flavor::Orchestrator)],
            roster: vec![SeatOverride {
                id: QuarkId::new("dup"),
                flavor: Some(Flavor::Worker),
                enabled: Some(false),
            }],
            max_exchanges: None,
        };
        let resolved = resolve_team(&repo, &global);
        assert_eq!(resolved.quarks.len(), 1, "seated once, not twice");
        assert_eq!(resolved.quarks[0].model, "LEGACY", "legacy seat wins");
        assert_eq!(resolved.quarks[0].flavor, Flavor::Orchestrator);
    }

    /// The migration that runs once on Jake's LIVE setup. This is the single most
    /// dangerous path in the split — it rewrites both his daemon-polled catalogue and
    /// his repo file — so the property it rests on is asserted here, not just in prose:
    /// **`resolve_team` is seat-for-seat identical (order included) before and after**,
    /// which is exactly what lets the running daemon reconcile the split to a no-op
    /// re-seat instead of tearing down live ACP sessions.
    #[test]
    fn migrate_to_catalogue_is_identity_under_resolve() {
        // Jake's actual four seats, incl. the disabled orchestrator (`enabled:false`).
        let mut repo = Team {
            quarks: vec![
                Seat { enabled: false, ..seat("opus", "claude", "opus", Flavor::Orchestrator) },
                seat("agy", "agy", "gemini", Flavor::Worker),
                seat("acp-claude", "acp-claude", "opus", Flavor::Worker),
                seat("acp-agy", "acp-agy", "gemini", Flavor::Worker),
            ],
            roster: vec![],
            max_exchanges: Some(12),
        };
        let mut global = Team::default();

        let before = resolve_team(&repo, &global);
        migrate_to_catalogue(&mut repo, &mut global);

        // Defs moved to the catalogue; repo is now overrides-only.
        assert_eq!(global.quarks.len(), 4, "every def landed in the catalogue");
        assert!(repo.quarks.is_empty(), "no legacy seats left in the repo file");
        assert_eq!(repo.roster.len(), 4, "each seat became one override");

        // THE property the no-op-reseat claim depends on.
        let after = resolve_team(&repo, &global);
        assert_eq!(
            before.quarks, after.quarks,
            "resolved roster (order + every field, incl. the disabled opus) is unchanged",
        );
        assert_eq!(before.max_exchanges, after.max_exchanges, "repo policy survives");
    }

    /// Running the migration a second time changes nothing — the daemon may launch the
    /// chamber more than once, and a repo with no legacy seats must be left untouched
    /// (no duplicate overrides, no re-clobbered catalogue).
    #[test]
    fn migrate_to_catalogue_is_idempotent() {
        let mut repo = Team {
            quarks: vec![seat("opus", "claude", "opus", Flavor::Orchestrator)],
            roster: vec![],
            max_exchanges: None,
        };
        let mut global = Team::default();
        migrate_to_catalogue(&mut repo, &mut global);

        let repo_once = repo.clone();
        let global_once = global.clone();
        migrate_to_catalogue(&mut repo, &mut global); // second pass
        assert_eq!(repo, repo_once, "second migration adds no override");
        assert_eq!(global, global_once, "second migration re-clobbers no def");
    }
}

#[cfg(test)]
mod enabled_tests {
    use super::*;

    /// Jake's live `team.json` has no `enabled` key anywhere — it was written before the
    /// field existed. Every one of those seats must read as **on**. If this ever
    /// defaults to `false`, a Hadron upgrade silently switches the whole swarm off.
    #[test]
    fn a_team_json_written_before_enabled_existed_reads_as_all_on() {
        // Copied from the shape the wizard actually writes.
        let legacy = r#"{"quarks":[
            {"id":"opus","provider":"claude","model":"opus","flavor":"orchestrator","transport":"cli"},
            {"id":"acp-claude","provider":"acp-claude","model":"claude","flavor":"worker","transport":"acp",
             "command":{"program":"npx","args":["-y","@agentclientprotocol/claude-agent-acp@latest"]}}
        ]}"#;
        let team: Team = serde_json::from_str(legacy).unwrap();
        assert_eq!(team.quarks.len(), 2);
        for seat in &team.quarks {
            assert!(seat.enabled, "{} came back DISABLED from a file that never mentioned it", seat.id.as_str());
        }
    }

    #[test]
    fn an_explicitly_disabled_seat_round_trips() {
        let mut seat = Seat::cli(QuarkId::new("agy"), "agy", "gemini", Flavor::Worker);
        seat.enabled = false;
        let back: Seat = serde_json::from_str(&serde_json::to_string(&seat).unwrap()).unwrap();
        assert_eq!(back, seat);
        assert!(!back.enabled);
    }

    /// `same_agent` is the identity used by the re-seat planner. It must ignore `enabled`
    /// and NOTHING else — if it ever ignored `model`, changing the model in Settings would
    /// leave the old model answering.
    #[test]
    fn same_agent_ignores_enabled_and_only_enabled() {
        let base = Seat::cli(QuarkId::new("x"), "claude", "opus", Flavor::Worker);

        let mut off = base.clone();
        off.enabled = false;
        assert!(base.same_agent(&off), "the switch is not the identity");
        assert_ne!(base, off, "but they are still different seats");

        for mutate in [
            (|s: &mut Seat| s.model = "sonnet".into()) as fn(&mut Seat),
            |s: &mut Seat| s.provider = "agy".into(),
            |s: &mut Seat| s.flavor = Flavor::Orchestrator,
            |s: &mut Seat| s.transport = Transport::Acp,
            |s: &mut Seat| s.command = Some(AcpCommand { program: "other".into(), args: vec![] }),
        ] {
            let mut changed = base.clone();
            mutate(&mut changed);
            assert!(!base.same_agent(&changed), "a real change must NOT look like the same agent");
        }
    }
}
