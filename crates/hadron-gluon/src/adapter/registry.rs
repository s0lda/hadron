use hadron_lattice::{CliSpec, Flavor, QuarkId, Seat, Transport};

use crate::adapter::acp::AcpQuark;
use crate::adapter::cli::CliQuark;
use crate::adapter::runner::ProcessRunner;
use crate::quark::Quark;

/// Which **transport** backs a configured quark — the one-shot CLI, or a resident
/// ACP agent.
///
/// This is the transport seam. `Cli` is the one-shot CLI path, config-driven by a
/// [`CliSpec`] (see `docs/superpowers/specs/2026-07-17-custom-cli-transport-design.md`):
/// same argv, same stdin, same `ProcessRunner`, but which flags/channel to use is
/// data, not a bespoke adapter per vendor. `Acp` is a resident JSON-RPC-over-stdio
/// session. A `team.json` that names no transport gets the CLI, so the default is
/// "exactly what happened before".
///
/// No longer `Copy`: both variants carry data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuarkKind {
    /// One-shot CLI, driven by a [`CliSpec`]: program, args, prompt channel,
    /// model flag, resume mode, timeout, posture, argv guard.
    Cli(CliSpec),
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
    /// The pure vendor a seat carries, e.g. `"claude"`.
    pub vendor: &'static str,
    /// What a human sees in the provider list.
    pub name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    /// Whether Hadron has completed a live ACP round-trip against this agent.
    pub proven: bool,
}

/// Every ACP agent with a built-in boot command. A seat may still override the
/// command, and a seat on an unlisted provider must supply one.
///
/// The first four entries are the ones we have actually written down from a
/// vendor's own docs (and `claude` we have driven live). Everything after them
/// is a **best-effort** preset generated from the upstream catalogue in
/// `docs/research/acp-providers.md`: the provider string and a plausible boot
/// command derived from the agent's package/CLI name, all `proven: false`. The
/// upstream page lists each agent's *package*, not always its exact ACP-mode
/// invocation — so these argv are a starting point, not a guarantee. If one does
/// not boot as written, override it with a `command` in `team.json` (which is the
/// escape hatch every uncatalogued provider already uses) and, once confirmed,
/// promote the preset here.
pub const ACP_AGENTS: &[AcpAgentSpec] = &[
    AcpAgentSpec {
        vendor: "claude",
        name: "Claude Code (ACP)",
        program: "npx",
        args: &["-y", "@agentclientprotocol/claude-agent-acp@latest"],
        // 82339b5: a real turn, a real reply, real token counts.
        proven: true,
    },
    AcpAgentSpec {
        vendor: "codex",
        name: "Codex CLI (ACP)",
        program: "npx",
        args: &["-y", "@agentclientprotocol/codex-acp@latest"],
        // Same publisher and the same npx shape as the Claude adapter above, and the
        // package bundles its own `@openai/codex` — so this seat needs no separate
        // Codex install, only auth (a ChatGPT login, or `OPENAI_API_KEY`/`CODEX_API_KEY`
        // in the daemon's environment). Command line taken from the adapter's README;
        // never driven here, so it does not get to claim `proven`.
        proven: false,
    },
    AcpAgentSpec {
        vendor: "gemini",
        name: "Gemini CLI (ACP)",
        program: "gemini",
        args: &["--experimental-acp"],
        // Command line written down from the agent's own docs, never driven here.
        proven: false,
    },
    AcpAgentSpec {
        vendor: "agy",
        name: "Antigravity (SDK)",
        program: "crates/hadron-gluon/scripts/venv/bin/python",
        args: &["crates/hadron-gluon/scripts/agy_acp.py"],
        proven: false,
    },
    // ── Best-effort presets from docs/research/acp-providers.md (all unproven) ──
    // Bare CLI name as program, no args: the upstream page rarely documents the
    // exact ACP-mode flag, so we do not guess one. Override in team.json if needed.
    AcpAgentSpec {
        vendor: "agentpool",
        name: "AgentPool (ACP)",
        program: "agentpool",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "augment",
        name: "Augment Code (ACP)",
        program: "augmentcode",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "autodev",
        name: "AutoDev (ACP)",
        program: "auto-dev",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "blackbox",
        name: "Blackbox AI (ACP)",
        program: "blackbox-cli",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "bub",
        name: "Bub (ACP)",
        program: "bub-acp-server",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "cagent",
        name: "Docker cagent (ACP)",
        program: "cagent",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "cline",
        name: "Cline (ACP)",
        program: "cline",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "code-assistant",
        name: "Code Assistant (ACP)",
        program: "code-assistant",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "construct",
        name: "Construct (ACP)",
        program: "construct",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "copilot",
        name: "GitHub Copilot (ACP)",
        program: "copilot",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "crow",
        name: "crow-cli (ACP)",
        program: "crow-cli",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "cursor",
        name: "Cursor (ACP)",
        program: "cursor",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "factory",
        name: "Factory Droid (ACP)",
        program: "factory",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "fast-agent",
        name: "fast-agent (ACP)",
        program: "fast-agent",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "fount",
        name: "fount (ACP)",
        program: "fount",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "goose",
        name: "Goose (ACP)",
        program: "goose",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "hermes",
        name: "Hermes Agent (ACP)",
        program: "hermes-agent",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "junie",
        name: "Junie (ACP)",
        program: "junie",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "kimi",
        name: "Kimi CLI (ACP)",
        program: "kimi-cli",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "kiro",
        name: "Kiro CLI (ACP)",
        program: "kiro",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "minion",
        name: "Minion Code (ACP)",
        program: "minion-code",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "mistral",
        name: "Mistral Vibe (ACP)",
        program: "mistral-vibe",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "openclaw",
        name: "OpenClaw (ACP)",
        program: "openclaw",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "opencode",
        name: "OpenCode (ACP)",
        program: "opencode",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "openhands",
        name: "OpenHands (ACP)",
        program: "openhands",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "pi",
        name: "Pi (ACP)",
        program: "pi-acp",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "poolside",
        name: "Poolside (ACP)",
        program: "pool",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "qoder",
        name: "Qoder CLI (ACP)",
        program: "qoder",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "qwen",
        name: "Qwen Code (ACP)",
        program: "qwen-code",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "sigit",
        name: "siGit Code (ACP)",
        program: "sigit",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "stakpak",
        name: "Stakpak (ACP)",
        program: "agent",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "stdiobus",
        name: "stdio Bus (ACP)",
        program: "stdiobus",
        args: &[],
        proven: false,
    },
    AcpAgentSpec {
        vendor: "vtcode",
        name: "VT Code (ACP)",
        program: "vtcode",
        args: &[],
        proven: false,
    },
];

impl AcpTarget {
    /// The Claude ACP adapter — a Node process wrapping the Claude Agent SDK and
    /// speaking ACP. This is the default boot command for the `"claude"`
    /// vendor, and it is what the live round-trip test drives.
    pub fn claude_adapter() -> AcpTarget {
        AcpTarget::for_vendor("claude").expect("claude is in the catalogue")
    }

    /// The built-in boot command for a catalogued vendor, or `None` for a
    /// vendor Hadron has never heard of (which must name its own command).
    pub fn for_vendor(vendor: &str) -> Option<AcpTarget> {
        ACP_AGENTS.iter().find(|a| a.vendor == vendor).map(|a| AcpTarget {
            program: a.program.to_string(),
            args: a.args.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// The boot target for a seat: its explicit `command`, else the vendor's
    /// built-in default. `None` for a non-ACP seat, or an ACP seat on a vendor
    /// with no catalogue command and no command of its own. The same resolution
    /// [`QuarkKind::from_seat`] uses, factored out so the chamber can build a probe
    /// target from a seat without going through the whole `QuarkKind` mapping.
    pub fn for_seat(seat: &Seat) -> Option<AcpTarget> {
        if seat.transport != Transport::Acp {
            return None;
        }
        match &seat.command {
            Some(cmd) => Some(AcpTarget { program: cmd.program.clone(), args: cmd.args.clone() }),
            None => AcpTarget::for_vendor(&seat.vendor),
        }
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
    /// The provider list the chamber renders: `(vendor, display name, program, args)`.
    ///
    /// A *view* of [`ACP_AGENTS`], not a list of its own — so the wizard can only
    /// offer an agent the daemon can actually boot. The previous version of this
    /// function kept its own literals and offered `agy acp`, which does not speak
    /// ACP at all: a provider list that drifts from the registry is a list of
    /// promises nothing keeps.
    pub fn available_presets() -> Vec<(&'static str, &'static str, &'static str, Vec<&'static str>)> {
        ACP_AGENTS.iter().map(|a| (a.vendor, a.name, a.program, a.args.to_vec())).collect()
    }

    /// Resolve a seat's transport. `Transport::Cli` resolves a [`CliSpec`] per
    /// §4.3 of the design doc: the seat's explicit `cli` spec wins; else the
    /// vendor's built-in preset (so `cli-agy` needs no config); else a bare
    /// `command` (program + args) builds a generic spec; else the seat has
    /// nothing to build from and errors, naming the fix. `Transport::Acp` reads
    /// the seat's boot `command`, falling back to the vendor's built-in default
    /// when the seat names none — so `claude` needs no command, and an agent we
    /// have never heard of needs one.
    pub fn from_seat(seat: &Seat) -> anyhow::Result<QuarkKind> {
        match seat.transport {
            Transport::Cli => {
                if let Some(spec) = seat.cli.clone() {
                    Ok(QuarkKind::Cli(spec))
                } else if let Some(spec) = CliSpec::preset(&seat.vendor) {
                    Ok(QuarkKind::Cli(spec))
                } else if let Some(cmd) = &seat.command {
                    Ok(QuarkKind::Cli(CliSpec::generic(cmd.program.clone(), cmd.args.clone())))
                } else {
                    anyhow::bail!(
                        "cli seat '{}' on vendor {:?} has no built-in preset — give it a \
                         `cli` spec or a `command`",
                        seat.id.as_str(),
                        seat.vendor
                    )
                }
            }
            Transport::Acp => {
                let target = AcpTarget::for_seat(seat).ok_or_else(|| {
                    let vendor = seat.vendor.as_str();
                    anyhow::anyhow!(
                        "seat '{}' is an ACP seat on vendor {vendor:?}, which has no \
                         built-in boot command — give it one, e.g. \
                         \"command\": {{\"program\": \"npx\", \"args\": [\"-y\", \"…\"]}}",
                        seat.id.as_str()
                    )
                })?;
                Ok(QuarkKind::Acp(target))
            }
            Transport::Sdk => anyhow::bail!(
                "seat '{}' uses the sdk transport, which is reserved but not yet implemented \
                 (see sub-project #3); use transport \"cli\" or \"acp\" for now",
                seat.id.as_str()
            ),
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
    pub effort: Option<String>,
    pub mode_config: Option<String>,
    /// The `@mention` name the router matches (see [`Quark::display_name`]). Resolved
    /// from the team config; `None` means the seat is addressable by id only.
    pub display_name: Option<String>,
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
    let name = spec.display_name.clone();
    let quark: Box<dyn Quark> = match spec.kind {
        QuarkKind::Cli(cli_spec) => Box::new(
            CliQuark::new(spec.id, spec.flavor, spec.model, cli_spec, ProcessRunner).with_display_name(name),
        ),
        QuarkKind::Acp(target) => Box::new(
            AcpQuark::new(spec.id, spec.flavor, spec.model, spec.effort, spec.mode_config, target)
                .with_display_name(name),
        ),
    };
    Ok(quark)
}

/// Build a live quark from a team-config `Seat`. The seat's `transport` picks CLI vs
/// ACP; the seat's `vendor` (`claude`/`agy`/…) picks which one within that transport.
pub fn build_seat(seat: &Seat) -> anyhow::Result<Box<dyn Quark>> {
    build(QuarkSpec {
        id: seat.id.clone(),
        flavor: seat.flavor.clone(),
        kind: QuarkKind::from_seat(seat)?,
        model: seat.model.clone(),
        effort: seat.effort.clone(),
        mode_config: seat.mode_config.clone(),
        display_name: seat.display_name.clone(),
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
            AcpQuark::new(seat.id.clone(), seat.flavor.clone(), seat.model.clone(), seat.effort.clone(), seat.mode_config.clone(), target)
                .watching(live_dir.to_path_buf())
                .with_display_name(seat.display_name.clone()),
        ));
    }
    build_seat(seat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{AcpCommand, CliSpec};

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
        let q = build_seat(&seat).unwrap();
        assert_eq!(q.id(), QuarkId::new("opus"));

        // A CLI seat on an uncatalogued vendor with no `cli` spec and no bare
        // `command` has nothing to build from — still an error.
        let bad = Seat::cli(QuarkId::new("x"), "chatgpt", "gpt-5", Flavor::Worker);
        assert!(build_seat(&bad).is_err());
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
    fn sdk_transport_is_reserved_and_not_seatable() {
        let mut seat = acp_seat("sdk-agy", "agy");
        seat.transport = Transport::Sdk;
        let err = QuarkKind::from_seat(&seat).expect_err("sdk must not resolve yet");
        assert!(
            err.to_string().contains("sdk") && err.to_string().contains("not yet implemented"),
            "error must name the reserved transport, got: {err}"
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
        let q = build_seat(&s).unwrap();
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
}
