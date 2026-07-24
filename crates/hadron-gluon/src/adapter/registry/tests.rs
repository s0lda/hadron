use super::*;
use hadron_lattice::secrets::MemoryStore;
use hadron_lattice::{AcpCommand, CliSpec};

/// The Antigravity SDK needs a Gemini API key; OAuth/login agents need none.
/// The chamber shows the API-key field ONLY for a vendor with a declared var,
/// so an over-broad entry here would put the field under a quark that never
/// uses it (exactly the bug this guards).
#[test]
fn secret_env_for_declares_agy_only() {
    assert_eq!(QuarkKind::secret_env_for("agy", hadron_lattice::Transport::Acp), &["GEMINI_API_KEY"]);
    assert_eq!(QuarkKind::secret_env_for("agy", hadron_lattice::Transport::Sdk), &["GEMINI_API_KEY"]);
    assert!(QuarkKind::secret_env_for("agy", hadron_lattice::Transport::Cli).is_empty());
    assert!(QuarkKind::secret_env_for("claude", hadron_lattice::Transport::Acp).is_empty());
    assert!(QuarkKind::secret_env_for("codex", hadron_lattice::Transport::Acp).is_empty());
    assert!(QuarkKind::secret_env_for("gemini", hadron_lattice::Transport::Acp).is_empty());
    assert!(QuarkKind::secret_env_for("some-unknown-vendor", hadron_lattice::Transport::Acp).is_empty());
}

/// A fresh, empty `SecretStore` for tests that are not exercising secret
/// resolution itself (that is `cli.rs`'s `cli_invocation_carries_resolved_secret_env`)
/// — every seat here has an empty `secret_env`, so `resolve_env` always returns
/// `[]` regardless of what this store holds.
fn store() -> MemoryStore {
    MemoryStore::new()
}

/// A CLI seat, vendor `"agy"`, no explicit `cli` spec and no `command` — the
/// exact shape a live `cli-agy` seat has today. Must resolve to the built-in
/// agy preset, so the existing seat needs no config change.
#[test]
fn cli_agy_seat_resolves_to_the_agy_preset() {
    let s = seat("a", "agy");
    assert!(s.cli.is_none() && s.command.is_none(), "precondition: no explicit config");
    assert_eq!(QuarkKind::from_seat(&s).unwrap(), QuarkKind::Cli(CliSpec::agy()));
}

/// An explicit `cli` spec wins over the vendor preset, even when the vendor
/// itself has one — the human's explicit config always beats a guess.
#[test]
fn cli_seat_with_explicit_cli_spec_wins() {
    let mut s = seat("a", "agy");
    let custom = CliSpec::generic("mycli".to_string(), vec!["--flag".to_string()]);
    s.cli = Some(custom.clone());
    assert_eq!(QuarkKind::from_seat(&s).unwrap(), QuarkKind::Cli(custom));
}

/// A CLI seat on a vendor with no built-in preset and no `cli`/`command` of its
/// own has nothing to build — and must say so, naming the fix, rather than
/// silently failing later.
#[test]
fn cli_seat_unknown_vendor_no_spec_errors() {
    let s = seat("a", "mystery");
    let err = QuarkKind::from_seat(&s).unwrap_err().to_string();
    assert!(err.contains("mystery"), "must name the vendor: {err}");
    assert!(
        err.contains("cli") && err.contains("command"),
        "must name the fix (a `cli` spec or a `command`): {err}"
    );
}

/// A bare `command` (program + args), with no preset and no explicit `cli`
/// spec, builds the generic "pipe prompt in, read reply out" spec — the
/// escape hatch for a CLI Hadron was never specifically taught.
#[test]
fn cli_seat_bare_command_builds_generic() {
    let mut s = seat("a", "mystery");
    s.command = Some(AcpCommand { program: "mycli".into(), args: vec!["--foo".into()] });
    assert_eq!(
        QuarkKind::from_seat(&s).unwrap(),
        QuarkKind::Cli(CliSpec::generic("mycli".to_string(), vec!["--foo".to_string()]))
    );
}

#[test]
fn rejects_reserved_and_malformed_ids() {
    assert!(validate_quark_id(&QuarkId::new("human")).is_err());
    assert!(validate_quark_id(&QuarkId::new("gluon")).is_err());
    assert!(validate_quark_id(&QuarkId::new("")).is_err());
    assert!(validate_quark_id(&QuarkId::new("  ")).is_err());
    assert!(validate_quark_id(&QuarkId::new("two words")).is_err());
}

#[test]
fn accepts_normal_ids() {
    assert!(validate_quark_id(&QuarkId::new("claude")).is_ok());
    assert!(validate_quark_id(&QuarkId::new("agy")).is_ok());
    assert!(validate_quark_id(&QuarkId::new("worker-2")).is_ok());
}

/// An id becomes a worktree DIRECTORY name (`worktree.rs` joins it onto
/// `trees_dir`), a git BRANCH ref (`quark/<id>/...`), and a live-file name
/// (`hadron_lattice::live`) — so it must be path- and git-ref-safe, not merely
/// whitespace-free. `/` and `\\` would nest or break a path; `:` breaks a git ref
/// on some platforms and is a Windows path separator besides. This is the SSOT
/// character-set check every seat-creation path (including a freely-typed wizard
/// vendor) rides on.
#[test]
fn rejects_path_and_ref_unsafe_characters() {
    assert!(validate_quark_id(&QuarkId::new("cli-foo/bar")).is_err());
    assert!(validate_quark_id(&QuarkId::new("a\\b")).is_err());
    assert!(validate_quark_id(&QuarkId::new("a:b")).is_err());
    // The charset allowlist alone permits `.` — including runs of it and leading/
    // trailing placement — but a git ref component may not contain `..` (git treats
    // it as a revision range) and may not start or end with `.`. Caught the daemon
    // failing `git checkout -b quark/cli-a..b/<ulid>` AFTER the id had already
    // persisted; must be rejected here instead, before it ever gets that far.
    assert!(validate_quark_id(&QuarkId::new("cli-a..b")).is_err());
    assert!(validate_quark_id(&QuarkId::new("cli-.x")).is_err());
    assert!(validate_quark_id(&QuarkId::new("cli-x.")).is_err());
    // A single interior dot stays legal — only `..` and leading/trailing dots are
    // rejected.
    assert!(validate_quark_id(&QuarkId::new("cli_tool.v2")).is_ok());
}

/// The live ids in today's `team.json` — none of these may ever start failing.
#[test]
fn accepts_the_live_seat_ids() {
    for id in ["cli-agy", "cli-claude", "acp-claude", "acp-codex", "acp-agy"] {
        assert!(validate_quark_id(&QuarkId::new(id)).is_ok(), "{id} must stay valid");
    }
}

/// The safe set is `[A-Za-z0-9._-]` — dot and underscore are allowed (a custom-CLI
/// vendor like "cli_tool.v2" is a reasonable thing to type), everything outside
/// ASCII alphanumerics/`.`/`_`/`-` is not.
#[test]
fn accepts_dot_and_underscore_in_ids() {
    assert!(validate_quark_id(&QuarkId::new("cli_tool.v2")).is_ok());
}

#[test]
fn build_wires_the_right_adapter() {
    let agy = build(QuarkSpec {
        id: QuarkId::new("agy"),
        flavor: Flavor::Orchestrator,
        kind: QuarkKind::Cli(CliSpec::agy()),
        model: "opus-4.8".into(),
        effort: None,
        mode_config: None,
        display_name: None,
        roles: Vec::new(),
        exclusive: false,
        commands: SeatCommands::default(),
        env: RedactedEnv::default(),
        energy_limit: None,
        deny_skills: Vec::new(),
    })
    .unwrap();
    assert_eq!(agy.id(), QuarkId::new("agy"));
    assert_eq!(agy.flavor(), Flavor::Orchestrator);

    let generic = build(QuarkSpec {
        id: QuarkId::new("custom"),
        flavor: Flavor::Worker,
        kind: QuarkKind::Cli(CliSpec::generic("mycli".into(), vec![])),
        model: String::new(),
        effort: None,
        mode_config: None,
        display_name: None,
        roles: Vec::new(),
        exclusive: false,
        commands: SeatCommands::default(),
        env: RedactedEnv::default(),
        energy_limit: None,
        deny_skills: Vec::new(),
    })
    .unwrap();
    assert_eq!(generic.id(), QuarkId::new("custom"));
    assert_eq!(generic.flavor(), Flavor::Worker);
}

#[test]
fn build_rejects_reserved_id() {
    let err = build(QuarkSpec {
        id: QuarkId::new("gluon"),
        flavor: Flavor::Worker,
        kind: QuarkKind::Cli(CliSpec::agy()),
        model: String::new(),
        effort: None,
        mode_config: None,
        display_name: None,
        roles: Vec::new(),
        exclusive: false,
        commands: SeatCommands::default(),
        env: RedactedEnv::default(),
        energy_limit: None,
        deny_skills: Vec::new(),
    });
    assert!(err.is_err());
}

/// A CLI seat, the default shape.
fn seat(id: &str, provider: &str) -> Seat {
    Seat::cli(QuarkId::new(id), provider, "", Flavor::Worker)
}

/// An ACP seat with no boot command — it must come from the catalogue.
fn acp_seat(id: &str, provider: &str) -> Seat {
    Seat { transport: Transport::Acp, ..seat(id, provider) }
}

#[test]
fn build_seat_maps_provider_and_rejects_unknown() {
    use hadron_lattice::Seat;
    let seat = Seat::cli(QuarkId::new("opus"), "agy", "opus-4.8", Flavor::Orchestrator);
    let q = build_seat(&seat, &store()).unwrap();
    assert_eq!(q.id(), QuarkId::new("opus"));

    // A CLI seat on an uncatalogued vendor with no `cli` spec and no bare
    // `command` has nothing to build from — still an error.
    let bad = Seat::cli(QuarkId::new("x"), "chatgpt", "gpt-5", Flavor::Worker);
    assert!(build_seat(&bad, &store()).is_err());
}

/// **The transport seam.** The existing `agy` provider must still resolve to
/// the one-shot CLI, via its built-in preset — a `team.json` written before
/// ACP existed picks up no new behaviour at all. This is the "byte-for-byte"
/// guarantee, at the fork itself.
///
/// `claude`'s CLI preset is gone with `claude.rs` (Claude is ACP-only now, per
/// spec §5/§8) — the live `cli-claude` seat stays `enabled: false`, so this is
/// a deliberate, documented gap, not a regression.
#[test]
fn the_existing_agy_provider_still_resolves_to_the_cli_transport() {
    assert_eq!(QuarkKind::from_seat(&seat("b", "agy")).unwrap(), QuarkKind::Cli(CliSpec::agy()));
    // and a seat that carries no transport hint is still a CLI seat
    assert_eq!(seat("b", "agy").transport, Transport::Cli);
    assert!(seat("b", "agy").command.is_none());
}

/// `claude` needs no `program`: it defaults to the Claude ACP adapter, so
/// seating one is a one-line config change.
///
/// This also PROVES the latent gap this task closes: `acp_seat` builds a seat
/// with `vendor: "claude"` (the pure form Task 1's `normalize_vendor` produces
/// from a legacy `"acp-claude"`, and what a fresh wizard-written seat carries
/// today) and no `command`. Before the catalogue was re-keyed on pure vendor,
/// `for_vendor("claude")` would find nothing — the catalogue was still keyed
/// `"acp-claude"` — and this seat would fail to resolve its boot command.
#[test]
fn acp_claude_defaults_to_the_claude_adapter() {
    let seat = acp_seat("acp", "claude");
    assert!(seat.command.is_none(), "the gap this closes only exists when the seat names no command");
    let kind = QuarkKind::from_seat(&seat).unwrap();
    assert_eq!(kind, QuarkKind::Acp(AcpTarget::claude_adapter()));
    let QuarkKind::Acp(target) = kind else { unreachable!() };
    assert_eq!(
        target.command_line(),
        "npx -y @agentclientprotocol/claude-agent-acp@latest",
        "a command-less claude ACP seat must resolve via for_vendor(\"claude\") to the real boot command"
    );
}

/// The GPT seat. Nothing structural ever stopped one — the ACP transport takes
/// any agent that speaks the protocol — so seating Codex is a catalogue entry and
/// a `vendor` string, not an adapter. This pins the boot command so a typo in it
/// is a red test rather than a seat that silently boots nothing.
#[test]
fn a_codex_seat_boots_the_openai_acp_adapter() {
    let QuarkKind::Acp(t) = QuarkKind::from_seat(&acp_seat("gpt", "codex")).unwrap() else {
        panic!("expected an ACP transport");
    };
    assert_eq!(t.command_line(), "npx -y @agentclientprotocol/codex-acp@latest");
}

/// …but the seat may override it — which is how a pinned version or a local
/// checkout gets used.
#[test]
fn a_seat_can_override_the_acp_boot_command() {
    let mut s = acp_seat("acp", "claude");
    s.command = Some(AcpCommand { program: "node".into(), args: vec!["./my-adapter.js".into()] });
    let QuarkKind::Acp(t) = QuarkKind::from_seat(&s).unwrap() else {
        panic!("expected an ACP transport");
    };
    assert_eq!(t.command_line(), "node ./my-adapter.js");
}

/// An uncatalogued ACP vendor reaches an agent we have never heard of — and it
/// must SAY so when the seat forgot to name a command, rather than booting nothing.
/// (Uses a vendor absent from `ACP_AGENTS` — `goose` used to serve this role, but
/// `goose` is itself a catalogued best-effort preset, so it no longer proves the
/// "uncatalogued" case now that the catalogue keys on pure vendor.)
#[test]
fn an_uncatalogued_acp_seat_requires_a_command_and_says_so() {
    let mut s = acp_seat("unlisted", "no-such-agent");
    s.command = Some(AcpCommand { program: "no-such-agent".into(), args: vec!["acp".into()] });
    let QuarkKind::Acp(t) = QuarkKind::from_seat(&s).unwrap() else {
        panic!("expected an ACP transport");
    };
    assert_eq!(t.command_line(), "no-such-agent acp");

    let err = QuarkKind::from_seat(&acp_seat("nope", "no-such-agent")).unwrap_err().to_string();
    assert!(err.contains("no built-in boot command"), "must name the fix: {err}");
}

#[test]
fn sdk_transport_is_unsupported_and_not_seatable() {
    let mut seat = acp_seat("sdk-agy", "agy");
    seat.transport = Transport::Sdk;
    let err = QuarkKind::from_seat(&seat).expect_err("sdk must not resolve");
    assert!(
        err.to_string().contains("sdk") && err.to_string().contains("unsupported"),
        "error must name the unsupported transport, got: {err}"
    );
}

/// `AcpTarget::for_seat` is what the chamber probes with, so it must resolve a
/// seat's boot command the same way `from_seat` does: a CLI seat has none, an ACP
/// seat defaults to its vendor's catalogue command, and an explicit `command`
/// overrides. (An uncatalogued ACP seat with no command has no target — `None`.)
#[test]
fn for_seat_resolves_the_probe_target() {
    // CLI seat → no ACP target.
    assert_eq!(AcpTarget::for_seat(&seat("opus", "claude")), None);

    // ACP seat, default vendor command.
    assert_eq!(
        AcpTarget::for_seat(&acp_seat("gpt", "codex")),
        AcpTarget::for_vendor("codex"),
    );

    // ACP seat with an explicit command override wins over the catalogue default.
    let mut s = acp_seat("acp", "claude");
    s.command = Some(AcpCommand { program: "node".into(), args: vec!["./x.js".into()] });
    assert_eq!(AcpTarget::for_seat(&s).unwrap().command_line(), "node ./x.js");

    // Uncatalogued ACP vendor, no command → nothing to boot.
    assert_eq!(AcpTarget::for_seat(&acp_seat("nope", "no-such-agent")), None);
}

/// The catalogue is the SSOT for the provider list: every entry must resolve to
/// a bootable target, and the chamber renders THIS, not a list of its own.
#[test]
fn every_catalogued_acp_agent_resolves_to_its_boot_command() {
    assert!(!ACP_AGENTS.is_empty());
    for a in ACP_AGENTS {
        let target = AcpTarget::for_vendor(a.vendor)
            .unwrap_or_else(|| panic!("{} is in the catalogue but will not resolve", a.vendor));
        assert_eq!(target.program, a.program);
        assert_eq!(
            QuarkKind::from_seat(&acp_seat("q", a.vendor)).unwrap(),
            QuarkKind::Acp(target),
            "a catalogued ACP seat needs no command of its own"
        );
    }
    assert!(AcpTarget::for_vendor("no-such-agent").is_none());
}

/// An ACP seat builds a real quark, and building it spawns NOTHING — the agent
/// subprocess boots lazily on the first `excite`, exactly as the CLI path does.
/// (If this ever regressed, seating a team would fork an `npx` per ACP quark at
/// daemon start.)
#[test]
fn building_an_acp_seat_spawns_no_process() {
    let s = acp_seat("acp", "claude");
    let q = build_seat(&s, &store()).unwrap();
    assert_eq!(q.id(), QuarkId::new("acp"));
    assert_eq!(q.flavor(), Flavor::Worker);
}

#[test]
fn catalogue_is_keyed_on_pure_vendor() {
    // The ACP catalogue is *the ACP catalogue*, so transport is implied: it keys on the
    // pure vendor "claude", not the old smeared "acp-claude".
    assert!(AcpTarget::for_vendor("claude").is_some(), "claude resolves by pure vendor");
    assert!(AcpTarget::for_vendor("acp-claude").is_none(), "the old smeared key is gone");
}

/// Role routing's build-time seam: a seat's `roles`/`exclusive` must reach the
/// quark `build_seat` constructs, the same way `display_name` already does — not
/// via a daemon-side population step that does not exist.
#[test]
fn build_seat_carries_the_seats_roles_onto_the_quark() {
    let mut s = seat("security-quark", "agy");
    s.roles = vec!["security".to_string()];
    s.exclusive = true;

    let q = build_seat(&s, &store()).unwrap();
    assert_eq!(q.roles(), vec!["security".to_string()], "roles never reached the built quark");
    assert!(q.exclusive(), "exclusive never reached the built quark");
}

/// The site the brief flagged as easiest to miss: `build_seat_watched`'s ACP
/// fast-path constructs `AcpQuark` directly, bypassing `build()` entirely — so
/// `build_seat`'s coverage above proves nothing about it. This is also the
/// daemon's REAL production path for a resident quark (`seat_quarks`/
/// `apply_reseat` call `build_seat_watched`), so an untested gap here would mean
/// the live `@Claude` seat's roles never actually reach its roster card.
/// Building spawns nothing (see `building_an_acp_seat_spawns_no_process`), so
/// this is free and INERT.
#[test]
fn build_seat_watched_carries_roles_onto_an_acp_quark() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = acp_seat("acp-sec", "claude");
    s.roles = vec!["security".to_string()];
    s.exclusive = true;

    let q = build_seat_watched(&s, dir.path(), &store()).unwrap();
    assert_eq!(q.roles(), vec!["security".to_string()], "roles never reached the ACP quark");
    assert!(q.exclusive(), "exclusive never reached the ACP quark");
}

/// The command allow/deny carry path's build-time seam: a seat's `commands`
/// must reach the quark `build_seat` constructs, the same way `roles`/
/// `exclusive` already do (see `build_seat_carries_the_seats_roles_onto_the_quark`).
#[test]
fn built_quark_exposes_its_seat_commands() {
    let mut s = seat("cmd-quark", "agy");
    s.commands = SeatCommands { not_allowed: vec!["rm -rf *".into()], ..Default::default() };

    let q = build_seat(&s, &store()).unwrap();
    assert_eq!(
        q.commands().not_allowed,
        vec!["rm -rf *".to_string()],
        "commands never reached the built quark"
    );
}

/// **The leak-rules test, at the spec level.** `QuarkSpec` derives `Debug`, and
/// its `env` field carries the resolved secret between `build_seat` and the
/// adapter — so `QuarkSpec`'s `Debug` is just as much a leak vector as
/// `CliInvocation`'s. Only the var NAME may ever come back out of `{:?}`.
#[test]
fn resolved_env_is_not_in_debug_output() {
    let secret_value = "s3cr3t-payload-xyz789";
    let spec = QuarkSpec {
        id: QuarkId::new("agy"),
        flavor: Flavor::Worker,
        kind: QuarkKind::Cli(CliSpec::agy()),
        model: String::new(),
        effort: None,
        mode_config: None,
        display_name: None,
        roles: Vec::new(),
        exclusive: false,
        commands: SeatCommands::default(),
        env: RedactedEnv(vec![("GEMINI_API_KEY".to_string(), secret_value.to_string())]),
        energy_limit: None,
        deny_skills: Vec::new(),
    };
    let debug = format!("{spec:?}");
    assert!(!debug.contains(secret_value), "the secret VALUE leaked into Debug: {debug}");
    assert!(debug.contains("GEMINI_API_KEY"), "the var NAME should still be visible: {debug}");
}

#[test]
fn test_registry_loader_resolution_fixture() {
    let fixture = r#"{
        "version": "1.0.0",
        "agents": [
            {
                "id": "agent-npx",
                "name": "NPX Agent",
                "distribution": {
                    "npx": {
                        "package": "@test/agent-npx@1.0.0",
                        "args": ["--acp"]
                    }
                }
            },
            {
                "id": "agent-uvx",
                "name": "UVX Agent",
                "distribution": {
                    "uvx": {
                        "package": "agent-uvx==2.0.0",
                        "args": ["-x"]
                    }
                }
            },
            {
                "id": "agent-binary",
                "name": "Binary Agent",
                "distribution": {
                    "binary": {
                        "linux-x86_64": {
                            "archive": "https://example.com/agent.tar.gz",
                            "cmd": "./agent"
                        }
                    }
                }
            }
        ]
    }"#;

    let data = loader::parse_registry_json(fixture).expect("fixture should parse");
    assert_eq!(data.agents.len(), 3);

    // 1. npx resolution
    let target_npx =
        loader::resolve_from_registry_data(&data, "agent-npx").expect("npx agent should resolve");
    assert_eq!(target_npx.program, "npx");
    assert_eq!(target_npx.args, vec!["-y", "@test/agent-npx@1.0.0", "--acp"]);

    // 2. uvx resolution
    let target_uvx =
        loader::resolve_from_registry_data(&data, "agent-uvx").expect("uvx agent should resolve");
    assert_eq!(target_uvx.program, "uvx");
    assert_eq!(target_uvx.args, vec!["agent-uvx==2.0.0", "-x"]);

    // 3. binary resolution (typed error)
    let err_binary = loader::resolve_from_registry_data(&data, "agent-binary")
        .expect_err("binary agent must be rejected");
    assert_eq!(err_binary, loader::RegistryError::BinaryNotSupported);
}


/// The merge rule's point: a **proven** preset outranks the registry. We have driven a
/// real turn through `@latest`; the registry pins a version. Ours wins.
#[test]
fn a_proven_preset_outranks_the_registry() {
    let claude = QuarkKind::available_agents()
        .into_iter()
        .find(|e| e.vendor == "claude")
        .expect("claude is in the catalogue");
    assert!(claude.proven);
    let (program, args) = claude.command.expect("claude has a boot command");
    assert_eq!(program, "npx");
    assert!(
        args.iter().any(|a| a.ends_with("claude-agent-acp@latest")),
        "the proven preset's own args should survive the merge: {args:?}"
    );
}

/// An unproven preset is a bare binary name guessed from a package name — exactly the
/// "install the CLI first" wall. Where the registry knows better it wins: `fast-agent`
/// becomes a real `uvx` command instead of `fast-agent` on `PATH`.
#[test]
fn the_registry_replaces_an_unproven_bare_name_guess() {
    let entry = QuarkKind::available_agents()
        .into_iter()
        .find(|e| e.vendor == "fast-agent")
        .expect("fast-agent is in the catalogue");
    assert!(!entry.proven);
    let (program, _) = entry.command.expect("fast-agent has a boot command");
    assert_eq!(program, "uvx", "the bare-name guess should have been replaced");
}

/// A `binary` agent has no command Hadron will synthesise, so the merge must *clear* our
/// bare-name guess rather than leave one that cannot work. `None` is the honest answer,
/// and what the wizard greys out.
#[test]
fn a_binary_registry_agent_clears_the_guessed_command() {
    let goose = QuarkKind::available_agents()
        .into_iter()
        .find(|e| e.vendor == "goose")
        .expect("goose is in the catalogue");
    assert_eq!(goose.command, None, "a binary-distribution agent must offer no command");
}

/// Nothing the presets knew about may vanish in the merge.
#[test]
fn every_preset_vendor_survives_the_merge() {
    let merged = QuarkKind::available_agents();
    for (vendor, ..) in QuarkKind::available_presets() {
        assert!(merged.iter().any(|e| e.vendor == vendor), "{vendor} vanished from the catalogue");
    }
}

/// The bundled snapshot is why the catalogue is non-empty on a box with no fetched cache
/// and no Zed — without it the wizard falls back to the guesses this module replaces.
#[test]
fn the_bundled_snapshot_parses_and_carries_claude() {
    let data = super::loader::bundled_registry().expect("the bundled snapshot must parse");
    assert!(data.agents.len() > 30, "bundled registry has {} agents", data.agents.len());
    assert!(data.agents.iter().any(|a| a.id == "claude-acp"));
}
