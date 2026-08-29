use super::*;
use crate::Flavor;
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

/// A team.json written before this field existed (or hand-edited without it) must
/// still parse — `nucleus_index_budget_kb` defaults to `None`, never a hard failure.
#[test]
fn parse_team_tolerates_a_missing_nucleus_index_budget_kb() {
    let team = parse_team("{\"quarks\":[]}").unwrap();
    assert_eq!(team.nucleus_index_budget_kb, None);
}

/// A hand-edited team.json may set any positive KiB value — not a strict enum, and
/// not limited to the Settings UI's 16/32/64/128 ladder.
#[test]
fn parse_team_reads_a_hand_edited_nucleus_index_budget_kb() {
    let team = parse_team("{\"quarks\":[],\"nucleus_index_budget_kb\":48}").unwrap();
    assert_eq!(team.nucleus_index_budget_kb, Some(48));
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
        nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
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
    save_team(&path, &Team { quarks: vec![seat("a", "claude", "m", Flavor::Worker)], roster: vec![], max_exchanges: None, nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default() }).unwrap();

    let two = Team {
        quarks: vec![
            seat("a", "claude", "m", Flavor::Worker),
            seat("b", "agy", "g", Flavor::Worker),
        ],
        roster: vec![],
        max_exchanges: None,
        nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
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

/// `roles`/`exclusive` round-trip through JSON like every other seat field.
#[test]
fn seat_roles_and_exclusive_serde_round_trip() {
    let mut s = seat("architect", "claude", "opus", Flavor::Worker);
    s.roles = vec!["architect".into(), "security".into()];
    s.exclusive = true;
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"roles\":[\"architect\",\"security\"]"), "{json}");
    assert!(json.contains("\"exclusive\":true"), "{json}");
    let back: Seat = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

/// A seat with neither key — every `team.json` written before role-routing existed —
/// must decode to empty roles / not-exclusive, and re-serializing it must not grow
/// those keys back into the file.
#[test]
fn legacy_seat_has_no_roles_and_is_not_exclusive() {
    let json = r#"{"id":"opus","provider":"claude","model":"opus-4.8","flavor":"orchestrator"}"#;
    let s: Seat = serde_json::from_str(json).unwrap();
    assert!(s.roles.is_empty());
    assert!(!s.exclusive);
    let out = serde_json::to_string(&s).unwrap();
    assert!(!out.contains("roles"), "empty roles must not grow the file: {out}");
    assert!(!out.contains("exclusive"), "false exclusive must not grow the file: {out}");
}

/// A role or exclusivity change is a different agent — same as a model/vendor
/// change — so the re-seat planner must rebuild rather than silently keep routing
/// the old scope.
#[test]
fn same_agent_rebuilds_on_role_or_exclusive_change() {
    let base = Seat::cli(QuarkId::new("x"), "claude", "opus", Flavor::Worker);

    let mut roles_changed = base.clone();
    roles_changed.roles = vec!["architect".into()];
    assert!(!base.same_agent(&roles_changed), "a role change must not look like the same agent");

    let mut exclusive_changed = base.clone();
    exclusive_changed.exclusive = true;
    assert!(!base.same_agent(&exclusive_changed), "an exclusivity change must not look like the same agent");
}

/// `Seat.commands` round-trips through JSON: allow/deny patterns come back
/// exactly as written.
#[test]
fn seat_commands_serde_round_trips() {
    let mut s = seat("architect", "claude", "opus", Flavor::Worker);
    s.commands = SeatCommands {
        allowed: vec!["git *".into()],
        not_allowed: vec!["rm -rf *".into()],
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: Seat = serde_json::from_str(&json).unwrap();
    assert_eq!(back.commands, s.commands);
}

/// A seat with no `commands` config — every `team.json` written before this
/// field existed — must decode to an empty `SeatCommands`, and re-serializing
/// it must not grow a `"commands"` key into the file.
#[test]
fn absent_commands_is_empty_and_omitted() {
    let s = seat("architect", "claude", "opus", Flavor::Worker);
    assert!(s.commands.is_empty());
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("\"commands\""), "empty commands must not grow the file: {json}");
}

/// A `commands` change is a different agent — same as a role/exclusive change —
/// so the re-seat planner must rebuild rather than silently keep the old
/// allow/deny list live.
#[test]
fn same_agent_rebuilds_on_commands_change() {
    let base = Seat::cli(QuarkId::new("x"), "claude", "opus", Flavor::Worker);
    let mut changed = base.clone();
    changed.commands.not_allowed.push("rm -rf *".into());
    assert!(!base.same_agent(&changed), "a commands change must not look like the same agent");
}

/// `Seat.secret_env` round-trips through JSON: declared names come back exactly
/// as written. Only the NAMES live here — never values.
#[test]
fn seat_secret_env_serde_round_trips() {
    let mut s = seat("agy", "agy", "gemini-3-pro", Flavor::Worker);
    s.secret_env = vec!["GEMINI_API_KEY".into()];
    let json = serde_json::to_string(&s).unwrap();
    let back: Seat = serde_json::from_str(&json).unwrap();
    assert_eq!(back.secret_env, s.secret_env);
}

/// A seat with no `secret_env` config — every `team.json` written before this
/// field existed — must decode to an empty `Vec`, and re-serializing it must
/// not grow a `"secret_env"` key into the file.
#[test]
fn absent_secret_env_is_empty_and_omitted() {
    let s = seat("architect", "claude", "opus", Flavor::Worker);
    assert!(s.secret_env.is_empty());
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("\"secret_env\""), "empty secret_env must not grow the file: {json}");
}

/// A `secret_env` change is a different agent — the subprocess env changes —
/// so the re-seat planner must rebuild rather than silently keep the old env
/// live.
#[test]
fn same_agent_rebuilds_on_secret_env_change() {
    let base = Seat::cli(QuarkId::new("x"), "claude", "opus", Flavor::Worker);
    let mut changed = base.clone();
    changed.secret_env.push("GEMINI_API_KEY".into());
    assert!(!base.same_agent(&changed), "a secret_env change must not look like the same agent");
}

#[test]
fn seat_model_params_serde_round_trips() {
    let mut s = seat("local-ollama", "http", "llama3", Flavor::Worker);
    s.model_params = ModelParams {
        temperature: Some(0.1),
        top_p: Some(0.95),
        max_tokens: Some(4096),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"temperature\":0.1"), "{json}");
    assert!(json.contains("\"top_p\":0.95"), "{json}");
    assert!(json.contains("\"max_tokens\":4096"), "{json}");
    let back: Seat = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn seat_without_model_params_parses_default_and_omits_key_on_serialize() {
    let json = r#"{"id":"local-ollama","provider":"http","model":"llama3","flavor":"worker"}"#;
    let s: Seat = serde_json::from_str(json).unwrap();
    assert_eq!(s.model_params, ModelParams::default());
    let out = serde_json::to_string(&s).unwrap();
    assert!(!out.contains("model_params"), "empty model_params must not grow file: {out}");
}

#[test]
fn same_agent_rebuilds_on_model_params_change() {
    let base = Seat::cli(QuarkId::new("x"), "claude", "opus", Flavor::Worker);
    let mut changed = base.clone();
    changed.model_params.temperature = Some(0.1);
    assert!(!base.same_agent(&changed), "model_params change must force rebuild");
}

#[test]
fn seat_supports_model_params_capability() {
    let mut http_seat = Seat::cli(QuarkId::new("h"), "http", "llama3", Flavor::Worker);
    http_seat.transport = Transport::Http;
    assert!(http_seat.supports_model_params(), "HTTP transport must support model params");

    let mut acp_seat = Seat::cli(QuarkId::new("a"), "acp-claude", "opus", Flavor::Worker);
    acp_seat.transport = Transport::Acp;
    assert!(acp_seat.supports_model_params(), "ACP transport must support model params");

    let mut cli_seat = Seat::cli(QuarkId::new("c"), "claude", "opus", Flavor::Worker);
    cli_seat.transport = Transport::Cli;
    assert!(!cli_seat.supports_model_params(), "CLI seat without parameter flags must not support model params");

    let mut custom_cli_seat = Seat::cli(QuarkId::new("custom"), "mycli", "model", Flavor::Worker);
    custom_cli_seat.transport = Transport::Cli;
    let mut spec = CliSpec::generic("mycli".into(), vec![]);
    spec.temperature_flag = Some("--temperature".into());
    custom_cli_seat.cli = Some(spec);
    assert!(custom_cli_seat.supports_model_params(), "CLI seat with temperature_flag must support model params");
}

#[test]
fn resolve_env_pulls_values_from_the_store() {
    use crate::secrets::{MemoryStore, SecretStore};
    let mut s = seat("agy", "agy", "gemini-3-pro", Flavor::Worker);
    s.secret_env = vec!["GEMINI_API_KEY".into()];
    let store = MemoryStore::new();
    store.set(&s.id, "GEMINI_API_KEY", "k").unwrap();

    assert_eq!(s.resolve_env(&store), vec![("GEMINI_API_KEY".to_string(), "k".to_string())]);
}

#[test]
fn resolve_env_skips_absent_names() {
    use crate::secrets::{MemoryStore, SecretStore};
    let mut s = seat("agy", "agy", "gemini-3-pro", Flavor::Worker);
    s.secret_env = vec!["A".into(), "B".into()];
    let store = MemoryStore::new();
    store.set(&s.id, "A", "a-value").unwrap();

    assert_eq!(s.resolve_env(&store), vec![("A".to_string(), "a-value".to_string())]);
}

#[test]
fn resolve_env_empty_when_no_secret_env() {
    use crate::secrets::MemoryStore;
    let s = seat("agy", "agy", "gemini-3-pro", Flavor::Worker);
    let store = MemoryStore::new();

    assert!(s.resolve_env(&store).is_empty());
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
        nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
    };
    let json = serde_json::to_string(&team).unwrap();
    let back: Team = serde_json::from_str(&json).unwrap();
    assert_eq!(team, back);
}

#[test]
fn team_merge_strategy_serde_round_trip() {
    let team = Team {
        merge_strategy: Some(MergeStrategy::Squash),
        ..Default::default()
    };
    let json = serde_json::to_string(&team).unwrap();
    assert!(json.contains(r#""merge_strategy":"squash""#), "{json}");
    let parsed: Team = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.merge_strategy(), MergeStrategy::Squash);
}

#[test]
fn team_merge_strategy_defaults_to_fast_forward() {
    let team: Team = serde_json::from_str("{}").unwrap();
    assert_eq!(team.merge_strategy(), MergeStrategy::FastForward);
}


#[test]
fn lookup_finds_a_seat_by_id() {
    let team = Team { quarks: vec![seat("agy", "agy", "gemini-3-pro", Flavor::Worker)], roster: vec![], max_exchanges: None, nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default() };
    let s = team.get(&QuarkId::new("agy")).unwrap();
    assert_eq!(s.vendor, "agy");
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
    // A silent parse failure would degrade to an empty team (which now seats nobody), so
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

#[test]
fn legacy_provider_key_parses_into_vendor_stripped_of_transport_prefix() {
    // A team.json written before this change: ACP seat carries the smeared "acp-claude",
    // CLI seat carries the bare vendor "agy".
    let json = r#"{"quarks":[
        {"id":"acp-claude","provider":"acp-claude","model":"opus","flavor":"worker","transport":"acp"},
        {"id":"agy","provider":"agy","model":"flash","flavor":"orchestrator","transport":"cli"}
    ]}"#;
    let team = parse_team(json).expect("legacy team parses");
    assert_eq!(team.quarks[0].vendor, "claude", "acp- prefix stripped to pure vendor");
    assert_eq!(team.quarks[1].vendor, "agy", "bare vendor left as-is");
}

#[test]
fn rename_legacy_ids_applies_the_map_to_quarks_and_roster_and_is_idempotent() {
    let mut team = Team {
        quarks: vec![
            Seat::cli(QuarkId::new("agy"), "agy", "flash", Flavor::Orchestrator),
            Seat::cli(QuarkId::new("opus"), "claude", "opus", Flavor::Worker),
        ],
        roster: vec![SeatOverride::role(QuarkId::new("agy"))],
        max_exchanges: None,
        nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
    };
    rename_legacy_ids(&mut team);
    assert_eq!(team.quarks[0].id.as_str(), "cli-agy");
    assert_eq!(team.quarks[1].id.as_str(), "cli-claude");
    assert_eq!(team.roster[0].id.as_str(), "cli-agy", "roster ids move too");

    let snapshot = team.clone();
    rename_legacy_ids(&mut team); // second run is a no-op
    assert_eq!(team, snapshot, "idempotent: nothing already-renamed changes");
}

#[test]
fn acp_ids_already_follow_convention_and_are_untouched() {
    let mut team = Team {
        quarks: vec![Seat {
            transport: Transport::Acp,
            ..Seat::cli(QuarkId::new("acp-claude"), "claude", "opus", Flavor::Worker)
        }],
        roster: vec![],
        max_exchanges: None,
        nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
    };
    rename_legacy_ids(&mut team);
    assert_eq!(team.quarks[0].id.as_str(), "acp-claude", "not in the map, unchanged");
    assert!(id_follows_convention("acp-claude", Transport::Acp));
    assert!(!id_follows_convention("agy", Transport::Cli));
}

#[test]
fn transport_code_is_the_short_wire_word() {
    assert_eq!(Transport::Cli.code(), "cli");
    assert_eq!(Transport::Acp.code(), "acp");
    assert_eq!(Transport::Sdk.code(), "sdk");
}

#[test]
fn conventional_id_prefixes_a_pure_vendor_with_the_transport_code() {
    assert_eq!(Transport::Acp.conventional_id("claude"), "acp-claude");
    assert_eq!(Transport::Cli.conventional_id("agy"), "cli-agy");
    assert_eq!(Transport::Sdk.conventional_id("agy"), "sdk-agy");
    // And the id it builds always satisfies the convention it's checked against.
    assert!(id_follows_convention(&Transport::Acp.conventional_id("claude"), Transport::Acp));
}

#[test]
fn a_pre_migration_team_resolves_to_the_same_seats_as_its_migrated_form() {
    // Legacy shape: smeared `provider`, legacy ids.
    let legacy = r#"{"quarks":[
        {"id":"agy","provider":"agy","model":"flash","flavor":"orchestrator","transport":"cli"},
        {"id":"acp-claude","provider":"acp-claude","model":"opus","flavor":"worker","transport":"acp"}
    ]}"#;
    let mut before = parse_team(legacy).unwrap();

    // Migrated shape: pure vendor + renamed cli- id, same behaviour.
    let migrated = r#"{"quarks":[
        {"id":"cli-agy","vendor":"agy","model":"flash","flavor":"orchestrator","transport":"cli"},
        {"id":"acp-claude","vendor":"claude","model":"opus","flavor":"worker","transport":"acp"}
    ]}"#;
    let after = parse_team(migrated).unwrap();

    // Vendor + transport + model + flavor must match seat-for-seat after the id-rename.
    rename_legacy_ids(&mut before);
    let empty = Team::default();
    let rb = resolve_team(&before, &empty);
    let ra = resolve_team(&after, &empty);
    let key = |t: &Team| t.quarks.iter()
        .map(|s| (s.id.0.clone(), s.vendor.clone(), s.transport, s.model.clone(), s.flavor.clone()))
        .collect::<Vec<_>>();
    assert_eq!(key(&rb), key(&ra), "legacy and migrated forms resolve identically");
}

#[cfg(test)]
mod resolve_tests {
    use super::super::*;
    use crate::Flavor;
    use tempfile::tempdir;

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
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        // A catalogue with *different* defs for the same ids must not leak in.
        let global = Team {
            quarks: vec![seat("opus", "claude", "SONNET-NOT-THIS", Flavor::Worker)],
            roster: vec![],
            max_exchanges: Some(999),
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let resolved = resolve_team(&repo, &global);
        assert_eq!(resolved.quarks, repo.quarks, "legacy seats kept verbatim");
        assert!(resolved.roster.is_empty());
        assert_eq!(resolved.max_exchanges, Some(7), "repo policy is authoritative");
    }

    /// Same policy shape as `max_exchanges` — `resolve_team` carries the REPO's
    /// nucleus index budget, never the catalogue's.
    #[test]
    fn resolve_team_carries_the_repos_nucleus_index_budget_not_the_catalogues() {
        let repo = Team {
            quarks: vec![seat("opus", "claude", "opus", Flavor::Orchestrator)],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: Some(64),
            merge_strategy: None, ..Default::default()
        };
        let global = Team {
            quarks: vec![],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: Some(128),
            merge_strategy: None, ..Default::default()
        };
        let resolved = resolve_team(&repo, &global);
        assert_eq!(resolved.nucleus_index_budget_kb, Some(64), "repo policy is authoritative");
    }

    /// An override pulls its definition from the catalogue and applies the per-repo
    /// role/state on top.
    #[test]
    fn an_override_resolves_its_definition_from_the_catalogue() {
        let global = Team {
            quarks: vec![seat("acp-claude", "acp-claude", "opus", Flavor::Worker)],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let repo = Team {
            quarks: vec![],
            roster: vec![SeatOverride {
                flavor: Some(Flavor::Orchestrator),
                enabled: Some(false),
                ..SeatOverride::role(QuarkId::new("acp-claude"))
            }],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let resolved = resolve_team(&repo, &global);
        assert_eq!(resolved.quarks.len(), 1);
        let s = &resolved.quarks[0];
        assert_eq!(s.vendor, "acp-claude", "definition comes from the catalogue");
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
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let repo = Team {
            quarks: vec![],
            roster: vec![SeatOverride::role(QuarkId::new("q"))],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let s = &resolve_team(&repo, &global).quarks[0];
        assert_eq!(s.flavor, Flavor::Orchestrator, "inherits catalogue role");
        assert!(!s.enabled, "inherits catalogue state");
    }

    /// A repo override MAY set `roles`/`exclusive`; absent means inherit the
    /// catalogue's, mirroring every other definition-delta field.
    #[test]
    fn resolve_team_applies_role_and_exclusive_overrides() {
        let global = Team {
            quarks: vec![Seat {
                roles: vec!["worker".into()],
                ..seat("q", "acp-claude", "opus", Flavor::Worker)
            }],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        // Absent override fields inherit the catalogue's roles/exclusive.
        let inherit = Team { roster: vec![SeatOverride::role(QuarkId::new("q"))], ..Team::default() };
        let s = &resolve_team(&inherit, &global).quarks[0];
        assert_eq!(s.roles, vec!["worker".to_string()], "inherits catalogue roles");
        assert!(!s.exclusive, "inherits catalogue exclusive (false)");

        // An explicit override lands on the resolved seat.
        let overridden = Team {
            roster: vec![SeatOverride {
                roles: Some(vec!["architect".into(), "security".into()]),
                exclusive: Some(true),
                ..SeatOverride::role(QuarkId::new("q"))
            }],
            ..Team::default()
        };
        let s = &resolve_team(&overridden, &global).quarks[0];
        assert_eq!(s.roles, vec!["architect".to_string(), "security".to_string()], "override lands");
        assert!(s.exclusive, "override lands");
    }

    #[test]
    fn resolve_team_applies_deny_skills_override() {
        let global = Team {
            quarks: vec![Seat {
                deny_skills: vec!["writing-plans".into()],
                ..seat("q", "acp-claude", "opus", Flavor::Worker)
            }],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        // Absent override fields inherit the catalogue's deny_skills.
        let inherit = Team { roster: vec![SeatOverride::role(QuarkId::new("q"))], ..Team::default() };
        let s = &resolve_team(&inherit, &global).quarks[0];
        assert_eq!(s.deny_skills, vec!["writing-plans".to_string()], "inherits catalogue deny_skills");

        // An explicit override lands on the resolved seat.
        let overridden = Team {
            roster: vec![SeatOverride {
                deny_skills: Some(vec!["executing-plans".into()]),
                ..SeatOverride::role(QuarkId::new("q"))
            }],
            ..Team::default()
        };
        let s = &resolve_team(&overridden, &global).quarks[0];
        assert_eq!(s.deny_skills, vec!["executing-plans".to_string()], "override lands");
    }

    /// A repo override MAY set `commands`; the resolved seat carries the override's
    /// allow/deny lists rather than the catalogue's (empty) default.
    #[test]
    fn resolve_team_applies_commands_override() {
        let global = Team {
            quarks: vec![seat("q", "acp-claude", "opus", Flavor::Worker)],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let repo = Team {
            roster: vec![SeatOverride {
                commands: Some(SeatCommands {
                    not_allowed: vec!["curl *".into()],
                    ..Default::default()
                }),
                ..SeatOverride::role(QuarkId::new("q"))
            }],
            ..Team::default()
        };
        let s = &resolve_team(&repo, &global).quarks[0];
        assert_eq!(s.commands.not_allowed, vec!["curl *".to_string()]);
    }

    /// Absent `commands` on the override inherits the catalogue's `commands`.
    #[test]
    fn resolve_team_inherits_commands_when_override_absent() {
        let global = Team {
            quarks: vec![Seat {
                commands: SeatCommands { allowed: vec!["git *".into()], not_allowed: vec![] },
                ..seat("q", "acp-claude", "opus", Flavor::Worker)
            }],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let repo = Team {
            roster: vec![SeatOverride::role(QuarkId::new("q"))],
            ..Team::default()
        };
        let s = &resolve_team(&repo, &global).quarks[0];
        assert_eq!(s.commands.allowed, vec!["git *".to_string()], "inherits catalogue commands");
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
                enabled: Some(true),
                ..SeatOverride::role(QuarkId::new("ghost"))
            }],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
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
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let repo = Team {
            quarks: vec![seat("dup", "claude", "LEGACY", Flavor::Orchestrator)],
            roster: vec![SeatOverride {
                flavor: Some(Flavor::Worker),
                enabled: Some(false),
                ..SeatOverride::role(QuarkId::new("dup"))
            }],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        let resolved = resolve_team(&repo, &global);
        assert_eq!(resolved.quarks.len(), 1, "seated once, not twice");
        assert_eq!(resolved.quarks[0].model, "LEGACY", "legacy seat wins");
        assert_eq!(resolved.quarks[0].flavor, Flavor::Orchestrator);
    }

    /// **Jake's exact scenario.** One catalogue default (`acp-claude` = Opus). A fresh
    /// repo that only adopts it (no model delta) resolves to Opus; a second repo that
    /// pins `model: Some("sonnet")` resolves to Sonnet — and the **catalogue is untouched
    /// by either**, so the two repos never clobber each other's model.
    #[test]
    fn a_model_override_diverges_per_repo_without_touching_the_catalogue() {
        let global = Team {
            quarks: vec![seat("acp-claude", "acp-claude", "opus", Flavor::Worker)],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        // Repo A: adopt only — inherits the catalogue default.
        let repo_a = Team {
            roster: vec![SeatOverride {
                enabled: Some(true),
                ..SeatOverride::role(QuarkId::new("acp-claude"))
            }],
            ..Team::default()
        };
        assert_eq!(resolve_team(&repo_a, &global).quarks[0].model, "opus", "A inherits default");

        // Repo B: same id, pinned to Sonnet here.
        let repo_b = Team {
            roster: vec![SeatOverride {
                enabled: Some(true),
                model: Some("sonnet".into()),
                ..SeatOverride::role(QuarkId::new("acp-claude"))
            }],
            ..Team::default()
        };
        assert_eq!(resolve_team(&repo_b, &global).quarks[0].model, "sonnet", "B overrides");
        assert_eq!(global.quarks[0].model, "opus", "the shared catalogue default is unchanged");
    }

    /// Each definition delta is applied **independently** — `resolve_team` runs four
    /// separate `if let Some` arms, and forgetting one would be a silent inherit with no
    /// compile error, so effort/mode/name each override while the others still inherit.
    #[test]
    fn each_definition_field_overrides_independently() {
        let global = Team {
            quarks: vec![Seat {
                effort: Some("high".into()),
                mode_config: Some("architect".into()),
                display_name: Some("Cat Default".into()),
                ..seat("q", "acp-claude", "opus", Flavor::Worker)
            }],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };

        // effort-only override; mode + name inherit.
        let eff = Team {
            roster: vec![SeatOverride {
                effort: Some(Some("low".into())),
                ..SeatOverride::role(QuarkId::new("q"))
            }],
            ..Team::default()
        };
        let s = &resolve_team(&eff, &global).quarks[0];
        assert_eq!(s.effort.as_deref(), Some("low"), "effort overridden");
        assert_eq!(s.mode_config.as_deref(), Some("architect"), "mode inherited");
        assert_eq!(s.display_name.as_deref(), Some("Cat Default"), "name inherited");

        // name-only override; effort + mode inherit.
        let nm = Team {
            roster: vec![SeatOverride {
                display_name: Some(Some("NnN Cat".into())),
                ..SeatOverride::role(QuarkId::new("q"))
            }],
            ..Team::default()
        };
        let s = &resolve_team(&nm, &global).quarks[0];
        assert_eq!(s.display_name.as_deref(), Some("NnN Cat"), "name overridden");
        assert_eq!(s.effort.as_deref(), Some("high"), "effort inherited");
    }

    /// The reason `effort`/`mode`/`name` are `Option<Option<String>>`: a repo must be
    /// able to **clear** an inherited value, not just set it. `Some(None)` clears here;
    /// a single `Option` could not tell this apart from "inherit" and would silently keep
    /// running the catalogue's `high`.
    #[test]
    fn a_cleared_knob_overrides_an_inherited_default_to_none() {
        let global = Team {
            quarks: vec![Seat {
                effort: Some("high".into()),
                ..seat("q", "acp-claude", "opus", Flavor::Worker)
            }],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        // Absent effort → inherits "high".
        let inherit = Team {
            roster: vec![SeatOverride::role(QuarkId::new("q"))],
            ..Team::default()
        };
        assert_eq!(resolve_team(&inherit, &global).quarks[0].effort.as_deref(), Some("high"));
        // Some(None) → explicitly cleared here, distinct from inherit.
        let cleared = Team {
            roster: vec![SeatOverride {
                effort: Some(None),
                ..SeatOverride::role(QuarkId::new("q"))
            }],
            ..Team::default()
        };
        assert_eq!(
            resolve_team(&cleared, &global).quarks[0].effort, None,
            "cleared beats the inherited default",
        );
    }

    /// The three-state knob must round-trip through JSON: absent = inherit, `null` =
    /// cleared, value = set. A regression here (e.g. serde emitting `null` for an absent
    /// field) would turn every inherit into a clear on the next load.
    #[test]
    fn override_knob_tristate_round_trips_through_json() {
        let ov = SeatOverride {
            model: Some("sonnet".into()),
            effort: Some(None),                       // cleared
            mode_config: Some(Some("ask".into())),    // set
            // display_name left absent → inherit
            ..SeatOverride::role(QuarkId::new("acp-claude"))
        };
        let json = serde_json::to_string(&ov).unwrap();
        assert!(json.contains("\"effort\":null"), "cleared serializes as null: {json}");
        assert!(!json.contains("display_name"), "an inherited field is omitted: {json}");
        let back: SeatOverride = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ov, "tri-state survives the round trip");
    }

    /// A `save_team` → `load_team` cycle must reproduce a byte-for-byte-equal `Team`
    /// across the shapes the chamber actually holds: a legacy ACP seat (command +
    /// effort + explicit enabled) *and* roster overrides carrying the tri-state knobs.
    /// The chamber polls these files every tick and reprojects only on `loaded != held`
    /// — a non-idempotent round trip would make that always true and repaint forever.
    #[test]
    fn save_load_round_trips_seats_and_overrides() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("team.json");
        let team = Team {
            quarks: vec![
                seat("opus", "claude", "opus", Flavor::Orchestrator),
                Seat {
                    effort: Some("high".into()),
                    mode_config: Some("ask".into()),
                    enabled: false,
                    command: Some(AcpCommand {
                        program: "npx".into(),
                        args: vec!["-y".into(), "codex-acp".into()],
                    }),
                    transport: Transport::Acp,
                    // Pure vendor, not the smeared "acp-codex": every real construction site
                    // now writes a normalized vendor (see `Seat::normalize_vendor` and its
                    // call in the chamber's ACP wizard), so a *held* seat and one freshly
                    // *loaded* from the same bytes must already agree without parse-time
                    // stripping doing any work here.
                    ..seat("acp-codex", "codex", "gpt-5.6-terra", Flavor::Worker)
                },
            ],
            roster: vec![
                SeatOverride {
                    enabled: Some(true),
                    model: Some("sonnet".into()),
                    effort: Some(None), // cleared
                    ..SeatOverride::role(QuarkId::new("acp-claude"))
                },
                SeatOverride::role(QuarkId::new("agy")),
            ],
            max_exchanges: Some(12),
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        save_team(&path, &team).unwrap();
        assert_eq!(load_team(&path), team, "the full team must round-trip idempotently");
    }

    /// The Settings-commit path, proven headlessly: editing an adopted quark
    /// (model→Sonnet, clear an inherited effort, rename) produces a delta that carries
    /// **only** what differs and preserves participation — and resolving that delta
    /// reproduces the edit while the shared catalogue default stays Opus/high. This is
    /// the correctness the chamber's `commit_settings_inputs` rests on.
    #[test]
    fn a_settings_edit_becomes_a_delta_that_resolves_back_to_the_edit() {
        let def = Seat {
            effort: Some("high".into()),
            ..seat("acp-claude", "acp-claude", "opus", Flavor::Worker)
        };
        let global = Team {
            quarks: vec![def.clone()],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
        };
        // The quark is already adopted here (enabled), and the user edits three fields.
        let prev = SeatOverride {
            enabled: Some(true),
            ..SeatOverride::role(QuarkId::new("acp-claude"))
        };
        let desired = Seat {
            model: "sonnet".into(),
            effort: None, // cleared
            display_name: Some("NnN Cat".into()),
            ..def.clone()
        };
        let ov = seat_override_delta(QuarkId::new("acp-claude"), &def, &desired, Some(&prev));

        // Delta carries only the differences, and keeps the quark adopted.
        assert_eq!(ov.model.as_deref(), Some("sonnet"));
        assert_eq!(ov.effort, Some(None), "cleared here, distinct from inherit");
        assert_eq!(ov.display_name, Some(Some("NnN Cat".into())));
        assert_eq!(ov.mode_config, None, "unedited knob inherits");
        assert_eq!(ov.enabled, Some(true), "participation preserved");

        // Resolving the delta reproduces the edit; the catalogue default is untouched.
        let repo = Team { roster: vec![ov], ..Team::default() };
        let s = &resolve_team(&repo, &global).quarks[0];
        assert_eq!(s.model, "sonnet");
        assert_eq!(s.effort, None);
        assert_eq!(s.display_name.as_deref(), Some("NnN Cat"));
        assert!(s.enabled);
        assert_eq!(global.quarks[0].model, "opus", "shared catalogue default unchanged");
        assert_eq!(global.quarks[0].effort.as_deref(), Some("high"), "default effort unchanged");
    }

    /// The "differs from default → Some" branch for `roles`/`exclusive` specifically:
    /// every other delta test leaves `desired.roles`/`desired.exclusive` equal to the
    /// catalogue default, so only the `None` (inherit) arm of those two fields ever
    /// ran. This edits both away from the default and checks the delta carries the
    /// edit AND that resolving it reproduces `desired` — the same round-trip property
    /// `a_settings_edit_becomes_a_delta_that_resolves_back_to_the_edit` proves for the
    /// other knobs.
    #[test]
    fn seat_override_delta_carries_changed_roles_and_exclusive() {
        let def = seat("acp-claude", "acp-claude", "opus", Flavor::Worker); // roles: [], exclusive: false
        let global = Team { quarks: vec![def.clone()], roster: vec![], max_exchanges: None, nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default() };

        let desired = Seat {
            roles: vec!["security".into()],
            exclusive: true,
            ..def.clone()
        };
        let ov = seat_override_delta(QuarkId::new("acp-claude"), &def, &desired, None);

        assert_eq!(ov.roles, Some(vec!["security".to_string()]), "role edit is carried");
        assert_eq!(ov.exclusive, Some(true), "exclusivity edit is carried");

        // Resolving the delta reproduces `desired`; the catalogue default is untouched.
        let repo = Team { roster: vec![ov], ..Team::default() };
        let s = &resolve_team(&repo, &global).quarks[0];
        assert_eq!(s.roles, vec!["security".to_string()]);
        assert!(s.exclusive);
        assert!(global.quarks[0].roles.is_empty(), "shared catalogue default unchanged");
        assert!(!global.quarks[0].exclusive, "shared catalogue default unchanged");
    }

    #[test]
    fn seat_override_delta_carries_changed_deny_skills() {
        let def = seat("acp-claude", "acp-claude", "opus", Flavor::Worker); // deny_skills: []
        let global = Team { quarks: vec![def.clone()], roster: vec![], max_exchanges: None, nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default() };

        let desired = Seat {
            deny_skills: vec!["writing-plans".into()],
            ..def.clone()
        };
        let ov = seat_override_delta(QuarkId::new("acp-claude"), &def, &desired, None);

        assert_eq!(ov.deny_skills, Some(vec!["writing-plans".to_string()]), "deny_skills edit is carried");

        // Resolving the delta reproduces `desired`; the catalogue default is untouched.
        let repo = Team { roster: vec![ov], ..Team::default() };
        let s = &resolve_team(&repo, &global).quarks[0];
        assert_eq!(s.deny_skills, vec!["writing-plans".to_string()]);
        assert!(global.quarks[0].deny_skills.is_empty(), "shared catalogue default unchanged");
    }

    /// An edit that changes nothing back to the catalogue default produces an all-inherit
    /// delta — so "reset to default in this repo" genuinely drops the override rather than
    /// pinning a copy of the default that would not track a later catalogue change.
    #[test]
    fn an_edit_matching_the_default_produces_an_all_inherit_delta() {
        let def = Seat {
            effort: Some("high".into()),
            ..seat("q", "acp-claude", "opus", Flavor::Worker)
        };
        let ov = seat_override_delta(QuarkId::new("q"), &def, &def, None);
        assert_eq!(ov.model, None);
        assert_eq!(ov.effort, None);
        assert_eq!(ov.mode_config, None);
        assert_eq!(ov.display_name, None);
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
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
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
            nucleus_index_budget_kb: None, merge_strategy: None, ..Default::default()
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
mod cli_spec_tests {
    use super::super::*;
    use crate::Mode;

    #[test]
    fn cli_spec_serde_round_trips() {
        let spec = CliSpec {
            program: "mycli".into(),
            args: vec!["--flag".into()],
            prompt: PromptChannel::Arg { flag: Some("--print".into()) },
            model_flag: Some("--model".into()),
            temperature_flag: Some("--temperature".into()),
            top_p_flag: Some("--top-p".into()),
            max_tokens_flag: Some("--max-tokens".into()),
            model_probe: Some(CliProbeSpec { args: vec!["models".into()] }),
            resume: ResumeMode::Continue { flag: "--continue".into() },
            timeout: Some(TimeoutArg { flag: "--timeout".into(), value: "10m".into() }),
            posture: PostureMap {
                ask: vec!["--ask".into()],
                write: vec!["--write".into()],
                auto: vec!["--auto".into()],
                bypass: vec!["--bypass".into()],
            },
            argv_guard: true,
            stream: None,
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: CliSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, back, "a full CliSpec must round-trip through JSON byte-for-byte");
    }

    /// `CliSpec::agy()` must mirror `crates/hadron-gluon/src/adapter/agy.rs` exactly —
    /// this is the SSOT check that stops the two from drifting apart.
    #[test]
    fn agy_preset_matches_todays_agy_flags() {
        let spec = CliSpec::agy();
        assert_eq!(spec.program, "agy");
        assert_eq!(spec.prompt, PromptChannel::Arg { flag: Some("--print".into()) });
        assert_eq!(spec.model_flag, Some("--model".into()));
        assert_eq!(spec.resume, ResumeMode::Continue { flag: "--continue".into() });
        assert_eq!(
            spec.timeout,
            Some(TimeoutArg { flag: "--print-timeout".into(), value: "29m".into() })
        );
        assert!(spec.argv_guard, "agy needs the E2BIG argv guard");
        assert_eq!(spec.posture.ask, vec!["--mode".to_string(), "plan".to_string()]);
        assert_eq!(spec.posture.write, vec!["--mode".to_string(), "accept-edits".to_string()]);
        assert_eq!(spec.posture.auto, vec!["--mode".to_string(), "accept-edits".to_string()]);
        assert_eq!(spec.posture.bypass, vec!["--dangerously-skip-permissions".to_string()]);
        assert_eq!(
            spec.stream,
            Some(StreamSpec {
                format: StreamFormat::AgyStreamJson,
                flags: vec!["--output-format".to_string(), "stream-json".to_string()],
            })
        );
    }

    #[test]
    fn preset_resolves_agy_claude_copilot_and_none_for_unknown() {
        assert_eq!(CliSpec::preset("agy"), Some(CliSpec::agy()));
        assert_eq!(CliSpec::preset("claude"), Some(CliSpec::claude()));
        assert_eq!(CliSpec::preset("copilot"), Some(CliSpec::copilot()));
        assert_eq!(CliSpec::preset("nonexistent-vendor"), None);
    }

    #[test]
    fn claude_preset_carries_mediation_flags() {
        let spec = CliSpec::claude();
        assert_eq!(spec.program, "claude");
        assert_eq!(spec.prompt, PromptChannel::Stdin);
        assert_eq!(spec.model_flag, Some("--model".into()));
        let expected_posture = vec![
            "--mcp-config".to_string(),
            "<hadron-forge-mcp>".to_string(),
            "--disallowedTools".to_string(),
            "Edit".to_string(),
            "Write".to_string(),
            "MultiEdit".to_string(),
            "NotebookEdit".to_string(),
        ];
        assert_eq!(spec.posture.ask, expected_posture);
        assert_eq!(spec.posture.write, expected_posture);
        assert_eq!(spec.posture.auto, expected_posture);
        assert_eq!(spec.posture.bypass, expected_posture);
    }

    #[test]
    fn copilot_preset_carries_mediation_flags() {
        let spec = CliSpec::copilot();
        assert_eq!(spec.program, "copilot");
        assert_eq!(spec.prompt, PromptChannel::Stdin);
        assert_eq!(spec.model_flag, Some("--model".into()));
        let expected_posture = vec![
            "--additional-mcp-config".to_string(),
            "<hadron-forge-mcp>".to_string(),
            "--disallowedTools".to_string(),
            "Edit".to_string(),
            "Write".to_string(),
            "MultiEdit".to_string(),
            "NotebookEdit".to_string(),
        ];
        assert_eq!(spec.posture.ask, expected_posture);
        assert_eq!(spec.posture.write, expected_posture);
        assert_eq!(spec.posture.auto, expected_posture);
        assert_eq!(spec.posture.bypass, expected_posture);
    }

    #[test]
    fn generic_spec_is_stdin_raw() {
        let spec = CliSpec::generic("mycli".into(), vec!["--flag".into()]);
        assert_eq!(spec.program, "mycli");
        assert_eq!(spec.args, vec!["--flag".to_string()]);
        assert_eq!(spec.prompt, PromptChannel::Stdin);
        assert_eq!(spec.model_flag, None);
        assert_eq!(spec.resume, ResumeMode::None);
        assert_eq!(spec.timeout, None);
        assert_eq!(spec.posture, PostureMap::default());
        assert!(!spec.argv_guard);

        // Negative test documenting boundary: generic injects NO posture args for any mode
        assert!(spec.posture.for_mode(crate::Mode::Ask).is_empty());
        assert!(spec.posture.for_mode(crate::Mode::Write).is_empty());
        assert!(spec.posture.for_mode(crate::Mode::Auto).is_empty());
        assert!(spec.posture.for_mode(crate::Mode::Bypass).is_empty());
    }

    #[test]
    fn posture_map_for_mode_selects_the_right_arm() {
        let posture = PostureMap {
            ask: vec!["ask".into()],
            write: vec!["write".into()],
            auto: vec!["auto".into()],
            bypass: vec!["bypass".into()],
        };
        assert_eq!(posture.for_mode(Mode::Ask), &["ask".to_string()]);
        assert_eq!(posture.for_mode(Mode::Write), &["write".to_string()]);
        assert_eq!(posture.for_mode(Mode::Auto), &["auto".to_string()]);
        assert_eq!(posture.for_mode(Mode::Bypass), &["bypass".to_string()]);
    }

    /// A minimal custom-CLI spec needs only `program` + `prompt`; everything else
    /// must default so a bare `{"program":"mycli","prompt":"stdin"}` parses.
    #[test]
    fn minimal_json_needs_only_program_and_prompt() {
        let json = r#"{"program":"mycli","prompt":"stdin"}"#;
        let spec: CliSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.program, "mycli");
        assert_eq!(spec.prompt, PromptChannel::Stdin);
        assert!(spec.args.is_empty());
        assert_eq!(spec.model_flag, None);
        assert_eq!(spec.model_probe, None);
        assert_eq!(spec.resume, ResumeMode::None);
        assert_eq!(spec.timeout, None);
        assert_eq!(spec.posture, PostureMap::default());
        assert!(!spec.argv_guard);
    }

    // The budget RESOLVER is not here. `Team` carries the raw `nucleus_index_budget_kb`
    // and nothing else; turning it into bytes (including the `Some(0)` → default rule a
    // naive `unwrap_or(32) * 1024` gets wrong) has one home,
    // `hadron_gluon::nucleus_status::resolve_budget_bytes`, and is tested there.
}

#[cfg(test)]
mod enabled_tests {
    use super::super::*;
    use crate::Flavor;

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
            |s: &mut Seat| s.vendor = "agy".into(),
            |s: &mut Seat| s.flavor = Flavor::Orchestrator,
            |s: &mut Seat| s.transport = Transport::Acp,
            |s: &mut Seat| s.command = Some(AcpCommand { program: "other".into(), args: vec![] }),
            |s: &mut Seat| s.cli = Some(CliSpec::agy()),
        ] {
            let mut changed = base.clone();
            mutate(&mut changed);
            assert!(!base.same_agent(&changed), "a real change must NOT look like the same agent");
        }
    }
}

#[test]
fn external_roots_serde_round_trip_and_default_to_none() {
    // A seat written before the field existed reads as "no external access at all",
    // which is the off state — there is no separate `Off` rung to get wrong.
    let legacy = r#"{"id":"a","vendor":"claude","model":"opus","flavor":"worker"}"#;
    let legacy: Seat = serde_json::from_str(legacy).unwrap();
    assert!(legacy.external_roots.is_empty());
    assert!(
        !serde_json::to_string(&legacy).unwrap().contains("external_roots"),
        "an empty allowlist must not grow a key in team.json"
    );

    let mut s = seat("a", "claude", "opus", Flavor::Worker);
    s.external_roots = vec![
        ExternalRootSpec { path: "/home/x/.hadron/sessions".into(), writable: false },
        ExternalRootSpec { path: "/home/x/dev/other".into(), writable: true },
    ];
    let back: Seat = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.external_roots, s.external_roots);
    assert!(!back.external_roots[0].writable);
    assert!(back.external_roots[1].writable);
}

#[test]
fn same_agent_rebuilds_on_an_external_root_change() {
    let a = seat("a", "claude", "opus", Flavor::Worker);
    let mut b = a.clone();
    b.external_roots = vec![ExternalRootSpec { path: "/tmp".into(), writable: true }];
    assert!(!a.same_agent(&b), "granting a root must re-seat, not silently apply later");
}
