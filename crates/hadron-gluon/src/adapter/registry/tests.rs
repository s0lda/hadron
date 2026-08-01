use super::*;
use hadron_lattice::secrets::{MemoryStore, SecretStore};
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
        external_roots: vec![],
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
        external_roots: vec![],
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
        external_roots: vec![],
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
        let built = QuarkKind::from_seat(&acp_seat("q", a.vendor));
        // Every preset but `agy` names a program on `PATH`, so a seat on it builds with
        // no command of its own. `agy` names a file — the vendored bridge interpreter —
        // and seating now REFUSES one that is not on disk, so on a machine that has
        // never provisioned the bridge that seat legitimately does not build. Asserting
        // the error rather than skipping it keeps both halves of the rule pinned.
        if std::path::Path::new(&target.program.replace(USER_HOME_TOKEN, "/x")).is_absolute() {
            match built {
                Ok(k) => assert_eq!(k, QuarkKind::Acp(target)),
                Err(e) => assert!(
                    e.to_string().contains("does not exist"),
                    "{} may only fail for an absent file, got: {e}",
                    a.vendor
                ),
            }
        } else {
            assert_eq!(
                built.unwrap(),
                QuarkKind::Acp(target),
                "a catalogued ACP seat needs no command of its own"
            );
        }
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

/// **The CLI half of `build_seat_watched`'s new branch, end to end.** A CLI seat
/// whose `cli.stream` is `Some` must come back `.watching()`-wired: a real turn
/// (a plain `sh` subprocess standing in for a streaming CLI — cheap, no network,
/// same reasoning as `runner.rs`'s ProcessRunner tests) publishes its draft into
/// `live_dir` mid-turn and leaves it clear once the turn ends, same as the ACP
/// case above. A `cli.stream: None` seat, by contrast, is untouched — it takes
/// the ordinary `build_seat` fallback and publishes nothing, ever.
#[tokio::test]
async fn build_seat_watched_wires_a_streaming_cli_seat_and_leaves_a_plain_one_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = seat("agy-stream", "agy");
    let mut cli = CliSpec::agy();
    cli.stream = Some(hadron_lattice::StreamSpec {
        args: vec![],
        format: hadron_lattice::StreamFormat::AgyStreamJson,
    });
    // Stand in for the real `agy` binary with a tiny shell script emitting the
    // agy stream-json shape — proves the wiring, not the vendor.
    cli.program = "sh".to_string();
    cli.args = vec![
        "-c".to_string(),
        "printf '%s\\n' \
         '{\"event\":\"step_update\",\"step_update\":{\"step_type\":\"agent_response\",\"text_delta\":\"hi\"}}' \
         '{\"event\":\"result\",\"result\":{\"response\":\"hi there\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}'"
            .to_string(),
    ];
    cli.prompt = hadron_lattice::PromptChannel::Stdin;
    cli.resume = hadron_lattice::ResumeMode::None;
    cli.timeout = None;
    cli.argv_guard = false;
    s.cli = Some(cli);

    let mut q = build_seat_watched(&s, dir.path(), &store()).unwrap();
    let t = hadron_lattice::Projection {
        isolated: true,
        task: "say hi".into(),
        invariants: String::new(),
        available_invariants: vec![],
        nucleus_digest: String::new(),
        live_activities: vec![],
        roster: vec![],
        field_window: vec![],
        field_truncated: false,
        nucleus_index: String::new(),
        nucleus_index_path: std::path::PathBuf::new(),
        nucleus_index_truncated: false,
        nucleus_index_budget_bytes: hadron_lattice::DEFAULT_NUCLEUS_INDEX_BUDGET_BYTES,
        nucleus_notes_dir: std::path::PathBuf::new(),
        git_diff: String::new(),
        cwd: std::env::temp_dir(),
        mode: hadron_lattice::Mode::default(),
        role_body: None,
        active_skill: None,
        named_specifically: true,
        has_forge_tools: false,
    };
    let outcome = q.excite(t).await.expect("streaming CLI turn");

    assert_eq!(outcome.message.as_deref(), Some("hi there"), "reply is the stream's parsed final text");
    assert_eq!(outcome.usage.spend.input, Some(1));
    assert_eq!(
        hadron_lattice::live::read(dir.path(), &QuarkId::new("agy-stream"), chrono::Utc::now()),
        None,
        "a finished streaming CLI turn must leave no activity behind, same as ACP"
    );
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
        external_roots: vec![],
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

/// A `binary` agent has no command Hadron will **download**, but the row still
/// documents the agent's own ACP argv — so the merge keeps our program (the CLI on
/// `PATH`) and takes the publisher's args.
///
/// This REPLACES an earlier assertion that the merge must clear the guess to `None`.
/// That was half right: a bare `goose` is genuinely broken, because an agent launched
/// with no ACP flag starts its interactive TUI and never answers `initialize` (the
/// `copilot` preset carries the same finding). But `None` greyed the row out as
/// "Needs a manual command" with no way in the wizard to supply one — eight agents
/// unusable to a human who had installed them. `goose acp` is what the publisher
/// documents and is neither a guess nor a download.
#[test]
fn a_binary_registry_agent_contributes_its_acp_args_not_a_download() {
    let goose = QuarkKind::available_agents()
        .into_iter()
        .find(|e| e.vendor == "goose")
        .expect("goose is in the catalogue");
    let (program, args) = goose.command.expect("goose keeps a bootable command");
    assert_eq!(program, "goose", "the program stays OURS — we never run the archive's cmd");
    assert_eq!(args, vec!["acp".to_string()], "the ACP subcommand comes from the publisher");
}

/// Nothing the presets knew about may vanish in the merge.
#[test]
fn every_preset_vendor_survives_the_merge() {
    let merged = QuarkKind::available_agents();
    for a in ACP_AGENTS {
        assert!(merged.iter().any(|e| e.vendor == a.vendor), "{} vanished from the catalogue", a.vendor);
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

/// **A catalogue boot command may never be cwd-relative.** The ACP transport spawns it
/// with `Command::new(program)` and never sets `current_dir`, so a relative path is
/// resolved against whatever directory the human launched the chamber from. Launching
/// from `target/release` made the `agy` seat's interpreter miss and every turn died with
/// a bare `No such file or directory (os error 2)`. A path in this repo is written
/// `{repo}`-anchored; a path under the user's Hadron directory (materialized assets, not
/// checkout files) is written `{hadron}`-anchored; anything else must be a bare program
/// name found on `PATH`.
#[test]
fn no_preset_boot_command_is_cwd_relative() {
    for a in ACP_AGENTS {
        for part in std::iter::once(a.program).chain(a.args.iter().copied()) {
            if !part.contains('/') {
                continue; // a bare program name, resolved via PATH
            }
            if part.starts_with('@') {
                continue; // an npm scoped package (`@scope/pkg`), never opened as a path
            }
            assert!(
                part.starts_with(REPO_ROOT_TOKEN) || part.starts_with(USER_HOME_TOKEN) || part.starts_with('/'),
                "{}'s boot command names the relative path {part:?} — anchor it with \
                 {REPO_ROOT_TOKEN} or {USER_HOME_TOKEN} or it resolves against the spawning process's cwd",
                a.vendor
            );
        }
    }
}

/// `resolved` substitutes in the program AND every arg (the `agy` bridge puts its script
/// in an arg, so a program-only substitution would fix half the command), leaves the
/// resolved secret env untouched, and yields absolute paths.
#[test]
fn resolved_expands_the_repo_token_across_program_and_args() {
    let t = AcpTarget {
        program: format!("{REPO_ROOT_TOKEN}/scripts/venv/bin/python"),
        args: vec!["--flag".to_string(), format!("{REPO_ROOT_TOKEN}/scripts/agy_acp.py")],
        env: vec![("GEMINI_API_KEY".to_string(), "sekrit".to_string())],
    };
    assert!(t.needs_repo_root());

    let r = t.resolved().expect("the test binary lives inside this repo's target/");
    assert!(!r.command_line().contains(REPO_ROOT_TOKEN), "left a token in {:?}", r.command_line());
    assert!(std::path::Path::new(r.program()).is_absolute(), "{:?} is not absolute", r.program());
    assert!(r.program().ends_with("/scripts/venv/bin/python"));
    assert_eq!(r.args()[0], "--flag", "a non-path arg must pass through untouched");
    assert!(std::path::Path::new(&r.args()[1]).is_absolute());
    assert_eq!(r.env(), t.env.as_slice(), "resolution must not disturb the resolved secret env");
}

/// `{hadron}` needs no git checkout at all — unlike `{repo}`, it resolves against
/// [`hadron_lattice::user_hadron_dir`], so a target naming only that token must resolve
/// from anywhere, including a directory with no repo above it whatsoever.
#[test]
fn resolved_expands_the_hadron_token_across_program_and_args() {
    let t = AcpTarget {
        program: format!("{USER_HOME_TOKEN}/bridges/agy/venv/bin/python"),
        args: vec!["--flag".to_string(), format!("{USER_HOME_TOKEN}/bridges/agy/agy_acp.py")],
        env: vec![("GEMINI_API_KEY".to_string(), "sekrit".to_string())],
    };
    assert!(t.needs_home_root());
    assert!(!t.needs_repo_root());

    let r = t
        .resolved_from(std::path::Path::new("/"))
        .expect("{hadron} needs no checkout, so this must resolve even from /");
    assert!(!r.command_line().contains(USER_HOME_TOKEN), "left a token in {:?}", r.command_line());
    assert!(std::path::Path::new(r.program()).is_absolute(), "{:?} is not absolute", r.program());
    assert!(r.program().ends_with("/bridges/agy/venv/bin/python"));
    assert_eq!(r.args()[0], "--flag", "a non-path arg must pass through untouched");
    assert!(std::path::Path::new(&r.args()[1]).is_absolute());
    assert_eq!(r.env(), t.env.as_slice(), "resolution must not disturb the resolved secret env");
}

/// The carve-out the plan calls out by name (`registry/mod.rs`'s `is_repo_relative`): a
/// part naming `{hadron}` must never be misclassified as "relative to the checkout" —
/// it is anchored to the user's home directory, which needs no checkout at all. Without
/// this carve-out a `{hadron}/…` program would be refused by the seat-time guard
/// (`0b8e9c05`) even though it never needed a source checkout.
#[test]
fn a_hadron_anchored_part_is_not_repo_relative() {
    assert!(!is_repo_relative(&format!("{USER_HOME_TOKEN}/bridges/agy/agy_acp.py")));
}

/// A command with no token is spawned exactly as written — `npx`, `gemini`, a seat's own
/// `command` — so resolution must be a no-op there and must NOT need a git repo at all.
#[test]
fn resolved_is_a_no_op_without_the_token() {
    let t = AcpTarget {
        program: "npx".to_string(),
        args: vec!["-y".to_string(), "@agentclientprotocol/claude-agent-acp@latest".to_string()],
        env: Vec::new(),
    };
    assert!(!t.needs_repo_root());
    assert_eq!(t.resolved().unwrap().command_line(), t.command_line());
    assert!(t.resolved().unwrap().env().is_empty());
}

/// **The live bug.** `~/.hadron/team.json` carried the `acp-agy` seat's own `command`
/// with both parts written relative to the checkout — no `{repo}` token, because nothing
/// in the Settings UI or in hand-editing teaches anyone about one. `for_seat` returns a
/// stored command verbatim, so the token-only check passed it straight to `spawn`, which
/// resolved it against the daemon's cwd: `target/release` under a release chamber, and
/// every `acp-agy` turn died with a bare `No such file or directory (os error 2)`.
/// Anchoring is therefore driven by the path's SHAPE, not by the presence of a token.
#[test]
fn a_seat_command_written_relative_is_anchored_to_the_repo_root() {
    let seat = Seat {
        command: Some(hadron_lattice::AcpCommand {
            program: "crates/hadron-gluon/scripts/venv/bin/python".to_string(),
            args: vec!["crates/hadron-gluon/scripts/agy_acp.py".to_string()],
        }),
        ..acp_seat("acp-agy", "agy")
    };

    let t = AcpTarget::for_seat(&seat).expect("an ACP seat with a command");
    assert!(!t.needs_repo_root(), "the stored command names no token — that was the trap");

    let r = t.resolved().expect("the test binary lives inside this repo");
    assert!(std::path::Path::new(r.program()).is_absolute(), "{:?} is still relative", r.program());
    assert!(r.program().ends_with("/crates/hadron-gluon/scripts/venv/bin/python"));
    assert!(std::path::Path::new(&r.args()[0]).exists(), "{:?} does not exist", r.args()[0]);
}

/// The other half of the shape rule: an arg with a slash is not automatically a path.
/// `npx -y @agentclientprotocol/claude-agent-acp@latest` is how every Claude seat boots,
/// and anchoring that package spec to the checkout would break all of them.
#[test]
fn an_npm_package_spec_is_never_anchored() {
    let r = AcpTarget::claude_adapter().resolved().expect("resolvable");
    assert!(
        r.args().iter().any(|a| a.contains("@agentclientprotocol/")),
        "the package spec was rewritten: {:?}",
        r.args()
    );
    assert!(!r.command_line().contains("/crates/"), "anchored a package spec: {:?}", r.command_line());
}

/// The one preset this all exists for: it must resolve to the real interpreter and the
/// real script — with **no checkout at all**, unlike the old `{repo}`-anchored preset.
/// Guards the whole chain — token, substitution, and the paths landing where the
/// preset says they are.
///
/// The script is [`materialize_script`](crate::adapter::bridge::materialize_script)d
/// here so its existence is always checked, not skipped. (Nothing on the SEATING path
/// materializes it — the chamber's Settings page is the only caller; seating merely
/// refuses a seat whose interpreter is absent.) The **venv is never provisioned here**
/// (that needs a live
/// `pip install` — forbidden in a unit test), so a machine that has never run the
/// bootstrap legitimately has none: checked when it is there, skipped with a reason
/// when it is not.
#[test]
fn the_agy_preset_resolves_to_files_that_exist() {
    let t = AcpTarget::for_vendor("agy").expect("agy is in the catalogue");
    let r = t.resolved().expect("{hadron} needs no git checkout at all to resolve");

    crate::adapter::bridge::materialize_script().expect("materializing the bridge script");
    assert!(std::path::Path::new(&r.args()[0]).exists(), "no bridge script at {:?}", r.args()[0]);

    let python = std::path::Path::new(r.program());
    let venv = python.parent().and_then(|p| p.parent());
    match venv {
        Some(v) if v.exists() => assert!(python.exists(), "venv {v:?} exists but has no {python:?}"),
        _ => eprintln!("skipped: no agy venv at {venv:?} — run the bootstrap to cover this"),
    }
}

/// The Antigravity bridge preset boots `{repo}/scripts/venv/bin/python` — a path that
/// exists only in a checkout, and `scripts/venv` is gitignored so it is not even in a
/// clone. From an installed binary (`~/.cargo/bin`) there is no checkout root at all
/// and `resolved()` hard-Errs. Erring is right for a seat that really needs a repo
/// file; offering the preset on a build where it CANNOT work is not. The error must
/// at minimum say so in words the human can act on.
#[test]
fn a_repo_anchored_preset_explains_itself_when_there_is_no_checkout() {
    let target = AcpTarget {
        program: format!("{REPO_ROOT_TOKEN}/scripts/venv/bin/python"),
        args: vec![format!("{REPO_ROOT_TOKEN}/scripts/agy_acp.py")],
        env: Default::default(),
    };
    // Simulate the installed case by resolving from a directory that is not a checkout.
    let err = target
        .resolved_from(std::path::Path::new("/"))
        .expect_err("no checkout root exists above /");
    let msg = err.to_string();
    assert!(
        msg.contains("only works from a source checkout"),
        "the error must name the cause a human can act on, got: {msg}"
    );
}

#[test]
fn a_relative_program_without_token_explains_itself_when_there_is_no_checkout() {
    let target = AcpTarget {
        program: "crates/hadron-gluon/scripts/venv/bin/python".to_string(),
        args: vec!["crates/hadron-gluon/scripts/agy_acp.py".to_string()],
        env: Default::default(),
    };
    let err = target
        .resolved_from(std::path::Path::new("/"))
        .expect_err("no checkout root exists above /");
    let msg = err.to_string();
    assert!(
        msg.contains("only works from a source checkout"),
        "the error must explain that relative paths require a source checkout, got: {msg}"
    );
}



/// The wizard's own copy of the rule above. A preset that cannot boot in THIS
/// installation must reach the human as an unclickable row with a reason, not as a
/// clickable row that seats a quark which then dies at turn time, minutes later, in
/// the field — where nobody connects it to the button they pressed.
///
/// `command: None` is the existing "listed but greyed out" signal, so this asserts the
/// reason lands too, and that entries which DO resolve are left alone.
#[test]
fn a_preset_that_cannot_resolve_here_is_listed_but_not_seatable() {
    let mut entries = vec![
        CatalogueEntry {
            vendor: "agy".into(),
            name: "Antigravity".into(),
            description: "Google Antigravity (Gemini), via the bundled ACP bridge".into(),
            command: Some((
                format!("{REPO_ROOT_TOKEN}/scripts/venv/bin/python"),
                vec![format!("{REPO_ROOT_TOKEN}/scripts/agy_acp.py")],
            )),
            proven: false,
        },
        CatalogueEntry {
            vendor: "claude".into(),
            name: "Claude Code (ACP)".into(),
            description: "Anthropic Claude Code, over ACP".into(),
            command: Some(("npx".into(), vec!["-y".into(), "@scope/pkg".into()])),
            proven: true,
        },
    ];
    // The installed case: nothing repo-anchored resolves.
    QuarkKind::mark_unseatable(&mut entries, |t| t.resolved_from(std::path::Path::new("/")).is_ok());

    assert!(entries[0].command.is_none(), "an unresolvable preset must not be seatable");
    assert!(
        entries[0].description.contains("source checkout"),
        "the greyed row must say why, got: {}",
        entries[0].description
    );
    assert!(entries[1].command.is_some(), "an npx preset needs no checkout and must survive");
    assert_eq!(entries[1].description, "Anthropic Claude Code, over ACP");
}

/// The negative control for the guard above: from a real checkout the agy row is
/// seatable, so `mark_unseatable` cannot be quietly greying out everything.
#[test]
fn the_agy_preset_is_still_seatable_from_a_checkout() {
    let agy = QuarkKind::available_agents()
        .into_iter()
        .find(|e| e.vendor == "agy")
        .expect("agy is in the catalogue");
    assert!(agy.command.is_some(), "the test binary lives in a checkout, so agy must resolve");
}

/// An ACP seat whose boot command cannot resolve must be refused where the seat is
/// BUILT, not where it takes a turn: the seating loops in `cli.rs` report and skip a
/// seat that fails to build, so one line of stderr replaces an errored turn per
/// dispatch, forever. (Jake's global `team.json` seats `acp-agy` for every project;
/// in a project with no checkout it errored on every single turn.)
#[test]
fn an_acp_seat_that_resolves_still_builds() {
    let s = acp_seat("acp-claude", "claude");
    assert!(matches!(QuarkKind::from_seat(&s), Ok(QuarkKind::Acp(_))));
}

/// Resolving proves a boot command is well-FORMED, not that anything is on disk.
/// `{hadron}` always resolves, so without this the `agy` seat would seat on a build
/// whose bridge venv has never been provisioned and then die with a bare ENOENT once
/// per dispatch — the failure the `{repo}` guard exists to stop, reached by the other
/// token. One skipped seat and one legible line instead.
#[test]
fn an_acp_seat_whose_program_is_absolute_and_absent_is_refused_at_seating() {
    let mut s = acp_seat("acp-ghost", "mystery");
    s.command = Some(AcpCommand {
        program: "/nonexistent/hadron/bridges/agy/venv/bin/python".into(),
        args: vec!["/nonexistent/agy_acp.py".into()],
    });
    let err = QuarkKind::from_seat(&s).unwrap_err().to_string();
    assert!(err.contains("/nonexistent/hadron/bridges/agy/venv/bin/python"), "must name it: {err}");
    assert!(err.contains("does not exist"), "must say what is wrong: {err}");

    // A program with no separator is PATH-resolved by `execve` and must NOT be stat'd.
    let mut s = acp_seat("acp-npx", "mystery");
    s.command = Some(AcpCommand { program: "npx".into(), args: vec!["@scope/pkg".into()] });
    assert!(matches!(QuarkKind::from_seat(&s), Ok(QuarkKind::Acp(_))));
}

/// **The Copilot-listed-twice guard.** `available_agents` merges the published
/// registry into the preset list keyed on vendor, and the old key was "the id with
/// `-acp` stripped" — a guess that matched `claude-acp` and `pi-acp` and missed
/// `github-copilot-cli`, `factory-droid`, `crow-cli`, `minion-code`, `mistral-vibe`,
/// `qwen-code` and `auggie`. Each of those rendered as a SECOND wizard row for an
/// agent already listed. `REGISTRY_ALIASES` states the mapping instead of guessing
/// it; this test fails the next time a registry refresh introduces a mismatch.
///
/// Compares NORMALISED names (our presets suffix " (ACP)", the publisher does not),
/// because the duplicate is visible to a human as two rows reading the same product —
/// distinct vendor keys are exactly what the bug had.
#[test]
fn no_two_catalogue_rows_name_the_same_agent() {
    let normalise = |s: &str| {
        s.to_lowercase()
            .replace("(acp)", "")
            .replace(" cli", "")
            .replace('-', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let entries = QuarkKind::available_agents();
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for e in &entries {
        let key = normalise(&e.name);
        if let Some(first) = seen.insert(key.clone(), e.vendor.clone()) {
            panic!(
                "two catalogue rows both read as {key:?}: vendors {first:?} and {:?} — \
                 add the registry id to REGISTRY_ALIASES",
                e.vendor
            );
        }
    }
}

/// **The "Needs a manual command" guard.** A registry row whose only distribution is
/// `binary` resolves to `None` (Hadron does not download and execute a third-party
/// archive). The merge used to assign that `None` straight over a preset's working
/// command, so eight agents that ship a bare-CLI preset here — `goose`, `cursor`,
/// `opencode`, `kimi`, `junie`, `poolside`, `stakpak`, `vtcode` — were listed as
/// unclickable in the add-quark wizard even though we knew how to boot them.
#[test]
fn a_binary_only_registry_row_never_erases_a_preset_command() {
    let entries = QuarkKind::available_agents();
    for vendor in ["goose", "opencode", "vtcode", "kimi"] {
        let entry = entries
            .iter()
            .find(|e| e.vendor == vendor)
            .unwrap_or_else(|| panic!("{vendor} is in the preset catalogue"));
        assert!(
            entry.command.is_some(),
            "{vendor} has a preset boot command; a binary-only registry row must not blank it"
        );
    }
}

/// The alias table is the SSOT for id→vendor, and `resolve_from_registry_data` must
/// read it too — it did not, so `for_vendor("copilot")` found nothing while the
/// wizard was busy listing Copilot twice.
#[test]
fn a_vendor_resolves_through_its_registry_alias() {
    assert_eq!(vendor_for("github-copilot-cli"), "copilot");
    assert_eq!(vendor_for("claude-acp"), "claude");
    assert_eq!(vendor_for("goose"), "goose");

    let registry = loader::bundled_registry().expect("the snapshot is compiled in");
    let target = loader::resolve_from_registry_data(&registry, "copilot")
        .expect("copilot resolves through its alias");
    assert_eq!(target.program, "npx");
    assert!(target.args.iter().any(|a| a.contains("@github/copilot")), "{:?}", target.args);
}

// -- Transport::Http: the cloud OpenAI-compatible vendor's api_key wiring --

/// `attach_http_api_key` is what `build()`'s `Http` arm calls to turn a resolved
/// `secret_env` value into `HttpTarget::api_key` — the seam that makes the wizard's
/// saved key actually reach the `Authorization: Bearer` header at turn time.
#[test]
fn attach_http_api_key_uses_the_first_resolved_secret() {
    let target = HttpTarget {
        vendor: crate::adapter::local::HttpVendor::OpenAiCompatible,
        base_url: "https://openrouter.ai/api/v1".to_string(),
        api_key: None,
    };
    let resolved = attach_http_api_key(target, &[("API_KEY".to_string(), "sk-live-123".to_string())]);
    assert_eq!(resolved.api_key, Some("sk-live-123".to_string()));
}

/// The keyless case — Ollama/LM Studio seats declare no `secret_env`, so `env` is
/// empty and `api_key` must stay `None` rather than picking up something stray.
#[test]
fn attach_http_api_key_is_a_no_op_with_no_resolved_secrets() {
    let target =
        HttpTarget { vendor: crate::adapter::local::HttpVendor::Ollama, base_url: "http://localhost:11434".to_string(), api_key: None };
    let resolved = attach_http_api_key(target, &[]);
    assert_eq!(resolved.api_key, None);
}

/// End-to-end through `build_seat`: a `Transport::Http` seat on the cloud vendor,
/// with its declared `secret_env` var resolved via the store — the same path the
/// chamber-saved seat and the daemon both go through. `build_seat` returning `Ok`
/// (rather than the "vendor Hadron does not know how to reach" error) proves
/// `HttpVendor::parse("openai-compatible")` and the whole seat→`QuarkKind::Http`→
/// `LocalQuark` chain compose for the new vendor, not just the local module alone.
#[test]
fn build_seat_resolves_the_cloud_vendor_and_wires_its_secret() {
    let mut s = Seat {
        transport: Transport::Http,
        vendor: "openai-compatible".to_string(),
        secret_env: vec!["API_KEY".to_string()],
        ..seat("http-openai-compatible", "openai-compatible")
    };
    s.http_base_url = Some("https://openrouter.ai/api/v1".to_string());

    let store = store();
    store.set(&s.id, "API_KEY", "sk-live-456").unwrap();

    let q = build_seat(&s, &store).unwrap();
    assert_eq!(q.id(), QuarkId::new("http-openai-compatible"));
}
