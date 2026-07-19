use hadron_lattice::{CliSpec, Flavor, QuarkId, Seat, SeatCommands, Transport};

use crate::adapter::acp::AcpQuark;
use crate::adapter::cli::CliQuark;
use crate::adapter::runner::{ProcessRunner, RedactedEnv};
use crate::quark::Quark;

use presets::ACP_AGENTS;

mod presets;
#[cfg(test)]
mod tests;

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
    /// The add-quark preset catalogue as `(vendor, name, description, program, args)`.
    /// `description` is a short human blurb for the first-class agents so the wizard
    /// row reads consistently (Antigravity otherwise shows a raw local python path
    /// next to the others' clean `npx` package specs); best-effort presets have `""`,
    /// and the UI falls back to showing their command line.
    pub fn available_presets(
    ) -> Vec<(&'static str, &'static str, &'static str, &'static str, Vec<&'static str>)> {
        ACP_AGENTS
            .iter()
            .map(|a| {
                let description = match a.vendor {
                    "claude" => "Anthropic Claude Code, over ACP",
                    "codex" => "OpenAI Codex CLI, over ACP",
                    "gemini" => "Google Gemini CLI, over ACP",
                    "agy" => "Google Antigravity (Gemini), via the bundled ACP bridge",
                    _ => "",
                };
                (a.vendor, a.name, description, a.program, a.args.to_vec())
            })
            .collect()
    }

    /// The secret env-var NAMES a vendor needs supplied (via the OS keychain — see
    /// `hadron_lattice::secrets`). A FACT about the provider, kept here in the
    /// catalogue SSOT: the Antigravity SDK (`agy`) authenticates with a Gemini API
    /// key; the ACP agents that authenticate by OAuth/login (claude, codex, the
    /// gemini CLI) need none, so the chamber shows no API-key field for them.
    /// Empty for any vendor not listed. Extend as providers are confirmed to need a
    /// key — do not guess (a wrong entry shows a pointless field).
    pub fn secret_env_for(vendor: &str) -> &'static [&'static str] {
        match vendor {
            "agy" => &["GEMINI_API_KEY"],
            _ => &[],
        }
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
                "seat '{}' uses the sdk transport, which is unsupported — Hadron has no \
                 native SDK adapter and none is planned; reach this provider over transport \
                 \"acp\" or \"cli\" instead",
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
    /// This seat's `@role` roles (see [`Quark::roles`]). Resolved from the team
    /// config; empty means the seat plays no particular role.
    pub roles: Vec<String>,
    /// Whether this seat is scoped only to its roles (see [`Quark::exclusive`]).
    pub exclusive: bool,
    /// This seat's per-seat command allow/deny lists (see [`Quark::commands`]).
    /// Resolved from the team config; empty means no config allow/deny.
    pub commands: SeatCommands,
    /// This seat's resolved secret env — `(name, value)` pairs from
    /// `Seat::resolve_env(store)` — carried onto the built adapter and, from there,
    /// onto the spawned subprocess's `Command::env()` and NOWHERE else. Wrapped in
    /// `RedactedEnv` so this struct's derived `Debug` cannot leak a value.
    pub env: RedactedEnv,
}

/// Enforce the naming contract: ids must be non-empty, whitespace-free, path- and
/// git-ref-safe tokens (so `@mention` routing works AND the id can be used verbatim as
/// a worktree directory name — `worktree.rs` joins it onto `trees_dir` — a git branch
/// ref segment — `quark/<id>/...` — and a live-file name — `hadron_lattice::live`),
/// and must not collide with the reserved actor names `human` / `gluon` or the
/// `orchestrator` role alias (which routing resolves to whoever holds the role, so an
/// id of that name would shadow it).
pub fn validate_quark_id(id: &QuarkId) -> anyhow::Result<()> {
    let s = id.as_str();
    if s.is_empty() || s.chars().any(|c| c.is_whitespace()) {
        anyhow::bail!("quark id must be a non-empty, whitespace-free token (got {s:?})");
    }
    // Path- and git-ref-safe character set. Rejects `/` and `\` (would nest or break a
    // worktree path / branch ref segment), `:` (invalid in a git ref, and a path
    // separator on Windows), and anything else outside plain ASCII — deliberately an
    // allowlist, not a blocklist of the characters found so far, so a not-yet-imagined
    // unsafe character (e.g. a shell metacharacter) is rejected by default too.
    if let Some(c) = s.chars().find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))) {
        anyhow::bail!(
            "quark id {s:?} contains {c:?}, which is not path/git-ref-safe — ids may only use \
             ASCII letters, digits, '.', '_', and '-'"
        );
    }
    // The charset allowlist above permits `.`, but git's ref-format rules are stricter
    // than "any allowed character in any position": a ref component may never contain a
    // `..` run (git reads it as a revision range, not a literal path segment — confirmed
    // via `git check-ref-format`). An id that broke this rule would pass every other
    // check, persist to team.json, and only fail when the daemon actually ran
    // `git checkout -b quark/<id>/<ulid>` — long after the human could still easily undo
    // it, and with a much less legible error.
    if s.contains("..") {
        anyhow::bail!("quark id {s:?} may not contain '..' — not a valid git ref component");
    }
    // Every id here follows (or is meant to follow) the `<transport>-<vendor>`
    // convention, so treat each `-`-delimited segment as its own path-like component and
    // require it not to start or end with `.` — a segment like `.x` or `x.` reads as a
    // hidden/dotfile-shaped or truncated name and has no legitimate use as a vendor
    // label. This is intentionally MORE conservative than strict git-ref legality (git
    // itself accepts a mid-path segment starting/ending with `.` as long as it isn't the
    // *whole* ref's leading/trailing component); the extra caution costs nothing here,
    // since no real vendor name needs it. A single interior dot (`cli_tool.v2`) stays
    // legal — only a dot at a segment's own start/end is rejected.
    if s.split('-').any(|seg| seg.starts_with('.') || seg.ends_with('.')) {
        anyhow::bail!(
            "quark id {s:?} has a '-'-delimited segment starting or ending with '.' — not a \
             valid vendor/transport label"
        );
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
    let roles = spec.roles.clone();
    let exclusive = spec.exclusive;
    let commands = spec.commands.clone();
    let env = spec.env.clone();
    let quark: Box<dyn Quark> = match spec.kind {
        QuarkKind::Cli(cli_spec) => Box::new(
            CliQuark::new(spec.id, spec.flavor, spec.model, cli_spec, ProcessRunner)
                .with_display_name(name)
                .with_roles(roles, exclusive)
                .with_commands(commands)
                .with_env(env),
        ),
        QuarkKind::Acp(target) => Box::new(
            AcpQuark::new(spec.id, spec.flavor, spec.model, spec.effort, spec.mode_config, target)
                .with_display_name(name)
                .with_roles(roles, exclusive)
                .with_commands(commands)
                .with_env(env),
        ),
    };
    Ok(quark)
}

/// Build a live quark from a team-config `Seat`. The seat's `transport` picks CLI vs
/// ACP; the seat's `vendor` (`claude`/`agy`/…) picks which one within that transport.
///
/// `store` resolves the seat's `secret_env` NAMES to VALUES (`Seat::resolve_env`) —
/// tests pass a `MemoryStore`; the daemon passes whatever backs its real credential
/// store. Resolution happens here, once, so every caller gets the same seam a real
/// keychain will eventually sit behind.
pub fn build_seat(seat: &Seat, store: &dyn hadron_lattice::secrets::SecretStore) -> anyhow::Result<Box<dyn Quark>> {
    build(QuarkSpec {
        id: seat.id.clone(),
        flavor: seat.flavor.clone(),
        kind: QuarkKind::from_seat(seat)?,
        model: seat.model.clone(),
        effort: seat.effort.clone(),
        mode_config: seat.mode_config.clone(),
        display_name: seat.display_name.clone(),
        roles: seat.roles.clone(),
        exclusive: seat.exclusive,
        commands: seat.commands.clone(),
        env: seat.resolve_env(store).into(),
    })
}

/// As [`build_seat`], but the quark also publishes what it is doing mid-turn into
/// `live_dir` (see `hadron_lattice::live`) so the chamber can render it.
///
/// Only the ACP transport has a mid-turn stream to publish: the CLI adapters run a
/// process to completion and hand back one blob, so there is nothing to watch until
/// it is over. That is a fact about the transports, not an omission here.
pub fn build_seat_watched(
    seat: &Seat,
    live_dir: &std::path::Path,
    store: &dyn hadron_lattice::secrets::SecretStore,
) -> anyhow::Result<Box<dyn Quark>> {
    validate_quark_id(&seat.id)?;
    let kind = QuarkKind::from_seat(seat)?;
    if let QuarkKind::Acp(target) = kind {
        return Ok(Box::new(
            AcpQuark::new(seat.id.clone(), seat.flavor.clone(), seat.model.clone(), seat.effort.clone(), seat.mode_config.clone(), target)
                .watching(live_dir.to_path_buf())
                .with_display_name(seat.display_name.clone())
                .with_roles(seat.roles.clone(), seat.exclusive)
                .with_commands(seat.commands.clone())
                .with_env(seat.resolve_env(store)),
        ));
    }
    build_seat(seat, store)
}

