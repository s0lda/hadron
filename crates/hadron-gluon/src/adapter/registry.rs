use hadron_lattice::{Flavor, QuarkId, Seat, Transport};

use crate::adapter::acp::AcpQuark;
use crate::adapter::agy::AgyQuark;
use crate::adapter::claude::ClaudeQuark;
use crate::adapter::runner::ProcessRunner;
use crate::quark::Quark;

/// Which **transport** backs a configured quark — the one-shot CLI, or a resident
/// ACP agent.
///
/// This is the transport seam. `Claude` and `Agy` are the existing one-shot CLI
/// path and are unchanged: same argv, same stdin, same `ProcessRunner`. `Acp` is a
/// resident JSON-RPC-over-stdio session. A `team.json` that names no transport gets
/// the CLI, so the default is "exactly what happened before".
///
/// No longer `Copy`: `Acp` carries its boot command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuarkKind {
    /// One-shot CLI: `claude -p --output-format json`, prompt on stdin.
    Claude,
    /// One-shot CLI: `agy --print <prompt>`, prompt on argv under a byte cap.
    Agy,
    /// A resident ACP agent subprocess.
    Acp(AcpTarget),
}

/// How to boot an ACP agent: the program, its args, and its env. This comes
/// straight from `team.json`, so reaching a **new** ACP-speaking provider is a
/// config change rather than a code change — which is the entire point of putting
/// a protocol here instead of another vendor adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpTarget {
    pub program: String,
    pub args: Vec<String>,
}

/// One ACP agent Hadron knows how to boot without being told how.
///
/// This is the **catalogue**, and it is the single source of truth for it: the
/// registry resolves a seat's boot command from here, and the chamber's provider
/// list is a *view* of it. A UI that hardcodes its own list of providers is a UI
/// that drifts from what the daemon can actually reach — which is exactly what the
/// Settings mock did.
///
/// `proven` is not decoration: it says whether we have driven a real turn through
/// this agent, or merely written down its command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcpAgentSpec {
    /// The `provider` string a seat carries, e.g. `"acp-claude"`.
    pub provider: &'static str,
    /// What a human sees in the provider list.
    pub name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    /// Whether Hadron has completed a live ACP round-trip against this agent.
    pub proven: bool,
}

/// Every ACP agent with a built-in boot command. A seat may still override the
/// command, and a seat on an unlisted provider must supply one.
pub const ACP_AGENTS: &[AcpAgentSpec] = &[
    AcpAgentSpec {
        provider: "acp-claude",
        name: "Claude Code (ACP)",
        program: "npx",
        args: &["-y", "@agentclientprotocol/claude-agent-acp@latest"],
        // 82339b5: a real turn, a real reply, real token counts.
        proven: true,
    },
    AcpAgentSpec {
        provider: "acp-gemini",
        name: "Gemini CLI (ACP)",
        program: "gemini",
        args: &["--experimental-acp"],
        // Command line written down from the agent's own docs, never driven here.
        proven: false,
    },
    AcpAgentSpec {
        provider: "acp-agy",
        name: "Antigravity (SDK)",
        program: "python3",
        args: &["crates/hadron-gluon/scripts/agy_acp.py"],
        proven: false,
    },
];

impl AcpTarget {
    /// The Claude ACP adapter — a Node process wrapping the Claude Agent SDK and
    /// speaking ACP. This is the default boot command for the `"acp-claude"`
    /// provider, and it is what the live round-trip test drives.
    pub fn claude_adapter() -> AcpTarget {
        AcpTarget::for_provider("acp-claude").expect("acp-claude is in the catalogue")
    }

    /// The built-in boot command for a catalogued provider, or `None` for a
    /// provider Hadron has never heard of (which must name its own command).
    pub fn for_provider(provider: &str) -> Option<AcpTarget> {
        ACP_AGENTS.iter().find(|a| a.provider == provider).map(|a| AcpTarget {
            program: a.program.to_string(),
            args: a.args.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// The shell-ish command line, for `AcpAgent::from_str` and for diagnostics.
    pub fn command_line(&self) -> String {
        std::iter::once(self.program.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl QuarkKind {
    /// The provider list the chamber renders: `(provider, display name, program, args)`.
    ///
    /// A *view* of [`ACP_AGENTS`], not a list of its own — so the wizard can only
    /// offer an agent the daemon can actually boot. The previous version of this
    /// function kept its own literals and offered `agy acp`, which does not speak
    /// ACP at all: a provider list that drifts from the registry is a list of
    /// promises nothing keeps.
    pub fn available_presets() -> Vec<(&'static str, &'static str, &'static str, Vec<&'static str>)> {
        ACP_AGENTS.iter().map(|a| (a.provider, a.name, a.program, a.args.to_vec())).collect()
    }

    /// Map a `Seat.provider` string to a **CLI** transport. ACP providers are not
    /// resolvable from the provider string alone — they need the seat's boot
    /// `command` — so they resolve in [`QuarkKind::from_seat`].
    pub fn from_provider(provider: &str) -> anyhow::Result<QuarkKind> {
        match provider {
            "claude" => Ok(QuarkKind::Claude),
            "agy" => Ok(QuarkKind::Agy),
            other => anyhow::bail!(
                "unknown provider {other:?} (expected \"claude\", \"agy\", \"acp-claude\" or \"acp\")"
            ),
        }
    }

    /// Resolve a seat's transport. `Transport::Cli` keeps resolving exactly as
    /// before, off the provider string alone. `Transport::Acp` reads the seat's
    /// boot `command`, falling back to the provider's built-in default when the
    /// seat names none — so `acp-claude` needs no command, and an agent we have
    /// never heard of needs one.
    pub fn from_seat(seat: &Seat) -> anyhow::Result<QuarkKind> {
        match seat.transport {
            Transport::Cli => QuarkKind::from_provider(&seat.provider),
            Transport::Acp => {
                let target = match (&seat.command, seat.provider.as_str()) {
                    (Some(cmd), _) => AcpTarget {
                        program: cmd.program.clone(),
                        args: cmd.args.clone(),
                    },
                    (None, provider) => AcpTarget::for_provider(provider).ok_or_else(|| {
                        anyhow::anyhow!(
                            "seat '{}' is an ACP seat on provider {provider:?}, which has no \
                             built-in boot command — give it one, e.g. \
                             \"command\": {{\"program\": \"npx\", \"args\": [\"-y\", \"…\"]}}",
                            seat.id.as_str()
                        )
                    })?,
                };
                Ok(QuarkKind::Acp(target))
            }
        }
    }
}

/// Declarative description of one quark to register: id, role, backing CLI, model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarkSpec {
    pub id: QuarkId,
    pub flavor: Flavor,
    pub kind: QuarkKind,
    pub model: String,
}

/// Enforce the naming contract: ids must be non-empty, whitespace-free tokens
/// (so `@mention` routing works), and must not collide with the reserved actor
/// names `human` / `gluon` or the `orchestrator` role alias (which routing
/// resolves to whoever holds the role, so an id of that name would shadow it).
pub fn validate_quark_id(id: &QuarkId) -> anyhow::Result<()> {
    let s = id.as_str();
    if s.is_empty() || s.chars().any(|c| c.is_whitespace()) {
        anyhow::bail!("quark id must be a non-empty, whitespace-free token (got {s:?})");
    }
    // Case-insensitively, because mentions now resolve that way: an id like
    // `Team` would otherwise validate fine and then be permanently unreachable,
    // since `@Team` resolves to the alias before it ever reaches the roster.
    if s.eq_ignore_ascii_case("human")
        || s.eq_ignore_ascii_case("gluon")
        || s.eq_ignore_ascii_case(crate::router::ORCHESTRATOR_ALIAS)
        || s.eq_ignore_ascii_case(crate::router::TEAM_ALIAS)
    {
        anyhow::bail!("quark id '{s}' is reserved");
    }
    Ok(())
}

/// Validate the spec and build a live quark over a real `ProcessRunner`. Wiring
/// the runner does not spawn anything — the process is spawned only on `excite`.
pub fn build(spec: QuarkSpec) -> anyhow::Result<Box<dyn Quark>> {
    validate_quark_id(&spec.id)?;
    let quark: Box<dyn Quark> = match spec.kind {
        QuarkKind::Claude => {
            Box::new(ClaudeQuark::new(spec.id, spec.flavor, spec.model, ProcessRunner))
        }
        QuarkKind::Agy => Box::new(AgyQuark::new(spec.id, spec.flavor, spec.model, ProcessRunner)),
        // Booting the agent is lazy — the subprocess is spawned on the first
        // `excite`, exactly as the CLI path spawns nothing at wiring time.
        QuarkKind::Acp(target) => {
            Box::new(AcpQuark::new(spec.id, spec.flavor, spec.model, target))
        }
    };
    Ok(quark)
}

/// Build a live quark from a team-config `Seat`. The seat's `provider` picks the
/// transport: CLI (`claude`/`agy`) or ACP (`acp-claude`/`acp`).
pub fn build_seat(seat: &Seat) -> anyhow::Result<Box<dyn Quark>> {
    build(QuarkSpec {
        id: seat.id.clone(),
        flavor: seat.flavor.clone(),
        kind: QuarkKind::from_seat(seat)?,
        model: seat.model.clone(),
    })
}

/// As [`build_seat`], but the quark also publishes what it is doing mid-turn into
/// `live_dir` (see `hadron_lattice::live`) so the chamber can render it.
///
/// Only the ACP transport has a mid-turn stream to publish: the CLI adapters run a
/// process to completion and hand back one blob, so there is nothing to watch until
/// it is over. That is a fact about the transports, not an omission here.
pub fn build_seat_watched(seat: &Seat, live_dir: &std::path::Path) -> anyhow::Result<Box<dyn Quark>> {
    validate_quark_id(&seat.id)?;
    let kind = QuarkKind::from_seat(seat)?;
    if let QuarkKind::Acp(target) = kind {
        return Ok(Box::new(
            AcpQuark::new(seat.id.clone(), seat.flavor.clone(), seat.model.clone(), target)
                .watching(live_dir.to_path_buf()),
        ));
    }
    build_seat(seat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::AcpCommand;

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

    #[test]
    fn build_wires_the_right_adapter() {
        let claude = build(QuarkSpec {
            id: QuarkId::new("claude"),
            flavor: Flavor::Orchestrator,
            kind: QuarkKind::Claude,
            model: "opus-4.8".into(),
        })
        .unwrap();
        assert_eq!(claude.id(), QuarkId::new("claude"));
        assert_eq!(claude.flavor(), Flavor::Orchestrator);

        let agy = build(QuarkSpec {
            id: QuarkId::new("agy"),
            flavor: Flavor::Worker,
            kind: QuarkKind::Agy,
            model: String::new(),
        })
        .unwrap();
        assert_eq!(agy.id(), QuarkId::new("agy"));
        assert_eq!(agy.flavor(), Flavor::Worker);
    }

    #[test]
    fn build_rejects_reserved_id() {
        let err = build(QuarkSpec {
            id: QuarkId::new("gluon"),
            flavor: Flavor::Worker,
            kind: QuarkKind::Agy,
            model: String::new(),
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
        let seat = Seat::cli(QuarkId::new("opus"), "claude", "opus-4.8", Flavor::Orchestrator);
        let q = build_seat(&seat).unwrap();
        assert_eq!(q.id(), QuarkId::new("opus"));

        // Not wired yet — and on the CLI transport there is no free-form escape
        // hatch, so an unknown provider must still be an error.
        let bad = Seat::cli(QuarkId::new("x"), "chatgpt", "gpt-5", Flavor::Worker);
        assert!(build_seat(&bad).is_err());
    }

    /// **The transport seam.** The existing providers must still resolve to the
    /// one-shot CLI — a `team.json` written before ACP existed picks up no new
    /// behaviour at all. This is the "byte-for-byte" guarantee, at the fork itself.
    #[test]
    fn the_existing_providers_still_resolve_to_the_cli_transport() {
        assert_eq!(QuarkKind::from_seat(&seat("a", "claude")).unwrap(), QuarkKind::Claude);
        assert_eq!(QuarkKind::from_seat(&seat("b", "agy")).unwrap(), QuarkKind::Agy);
        // and a seat that carries no transport hint is still a CLI seat
        assert_eq!(seat("a", "claude").transport, Transport::Cli);
        assert!(seat("a", "claude").command.is_none());
    }

    /// `acp-claude` needs no `program`: it defaults to the Claude ACP adapter, so
    /// seating one is a one-line config change.
    #[test]
    fn acp_claude_defaults_to_the_claude_adapter() {
        let kind = QuarkKind::from_seat(&acp_seat("acp", "acp-claude")).unwrap();
        assert_eq!(kind, QuarkKind::Acp(AcpTarget::claude_adapter()));
    }

    /// …but the seat may override it — which is how a pinned version or a local
    /// checkout gets used.
    #[test]
    fn a_seat_can_override_the_acp_boot_command() {
        let mut s = acp_seat("acp", "acp-claude");
        s.command = Some(AcpCommand { program: "node".into(), args: vec!["./my-adapter.js".into()] });
        let QuarkKind::Acp(t) = QuarkKind::from_seat(&s).unwrap() else {
            panic!("expected an ACP transport");
        };
        assert_eq!(t.command_line(), "node ./my-adapter.js");
    }

    /// An uncatalogued ACP provider reaches an agent we have never heard of — and it
    /// must SAY so when the seat forgot to name a command, rather than booting nothing.
    #[test]
    fn an_uncatalogued_acp_seat_requires_a_command_and_says_so() {
        let mut s = acp_seat("goose", "goose");
        s.command = Some(AcpCommand { program: "goose".into(), args: vec!["acp".into()] });
        let QuarkKind::Acp(t) = QuarkKind::from_seat(&s).unwrap() else {
            panic!("expected an ACP transport");
        };
        assert_eq!(t.command_line(), "goose acp");

        let err = QuarkKind::from_seat(&acp_seat("nope", "goose")).unwrap_err().to_string();
        assert!(err.contains("no built-in boot command"), "must name the fix: {err}");
    }

    /// The catalogue is the SSOT for the provider list: every entry must resolve to
    /// a bootable target, and the chamber renders THIS, not a list of its own.
    #[test]
    fn every_catalogued_acp_agent_resolves_to_its_boot_command() {
        assert!(!ACP_AGENTS.is_empty());
        for a in ACP_AGENTS {
            let target = AcpTarget::for_provider(a.provider)
                .unwrap_or_else(|| panic!("{} is in the catalogue but will not resolve", a.provider));
            assert_eq!(target.program, a.program);
            assert_eq!(
                QuarkKind::from_seat(&acp_seat("q", a.provider)).unwrap(),
                QuarkKind::Acp(target),
                "a catalogued ACP seat needs no command of its own"
            );
        }
        assert!(AcpTarget::for_provider("no-such-agent").is_none());
    }

    /// An ACP seat builds a real quark, and building it spawns NOTHING — the agent
    /// subprocess boots lazily on the first `excite`, exactly as the CLI path does.
    /// (If this ever regressed, seating a team would fork an `npx` per ACP quark at
    /// daemon start.)
    #[test]
    fn building_an_acp_seat_spawns_no_process() {
        let s = acp_seat("acp", "acp-claude");
        let q = build_seat(&s).unwrap();
        assert_eq!(q.id(), QuarkId::new("acp"));
        assert_eq!(q.flavor(), Flavor::Worker);
    }
}
