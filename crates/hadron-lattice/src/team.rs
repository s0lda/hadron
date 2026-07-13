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
    /// The backing CLI/vendor, e.g. "claude", "agy".
    pub provider: String,
    /// The model this seat runs, e.g. "opus-4.8", "gemini-3-pro".
    pub model: String,
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
        let Seat { id, provider, model, flavor, transport, command, enabled: _ } = self;
        id == &other.id
            && provider == &other.provider
            && model == &other.model
            && flavor == &other.flavor
            && transport == &other.transport
            && command == &other.command
    }

    /// A CLI seat — the shape every seat had before ACP. Keeps construction sites
    /// (and tests) from having to spell out two ACP fields they do not care about.
    pub fn cli(id: QuarkId, provider: impl Into<String>, model: impl Into<String>, flavor: Flavor) -> Seat {
        Seat {
            id,
            provider: provider.into(),
            model: model.into(),
            flavor,
            transport: Transport::Cli,
            command: None,
            enabled: true,
        }
    }
}

/// The full team: every seat the human has added.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    #[serde(default)]
    pub quarks: Vec<Seat>,
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
        save_team(&path, &Team { quarks: vec![seat("a", "claude", "m", Flavor::Worker)] }).unwrap();

        let two = Team {
            quarks: vec![
                seat("a", "claude", "m", Flavor::Worker),
                seat("b", "agy", "g", Flavor::Worker),
            ],
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
        };
        let json = serde_json::to_string(&team).unwrap();
        let back: Team = serde_json::from_str(&json).unwrap();
        assert_eq!(team, back);
    }

    #[test]
    fn lookup_finds_a_seat_by_id() {
        let team = Team { quarks: vec![seat("agy", "agy", "gemini-3-pro", Flavor::Worker)] };
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
