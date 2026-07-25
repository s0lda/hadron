use hadron_lattice::{CliSpec, Flavor, QuarkId, Seat, SeatCommands, Transport};

use crate::adapter::acp::AcpQuark;
use crate::adapter::cli::CliQuark;
use crate::adapter::runner::{ProcessRunner, RedactedEnv};
use crate::quark::Quark;

pub mod loader;

pub use loader::{
    parse_registry_json, resolve_agent_command, resolve_from_registry_data, AcpRegistryAgent,
    AcpRegistryData, AcpRegistryDistribution, RegistryError,
};

use presets::ACP_AGENTS;

/// The token a boot command uses for "the main checkout's root", resolved by
/// [`AcpTarget::resolved`] just before spawn.
///
/// It exists because a boot command may name a path inside this repo (the `agy`
/// bridge does), and there is no other honest way to write one: the catalogue is
/// compiled, the ACP registry is parsed from JSON, and a seat may supply its own
/// `command` in `team.json` — all three flow into the same [`AcpTarget`], so the
/// substitution has one home and covers all three.
pub const REPO_ROOT_TOKEN: &str = "{repo}";

/// One row of the add-quark catalogue the chamber renders, from whichever source knows
/// the agent best.
///
/// Owned, unlike the compiled preset list's `&'static str` tuples, because half these
/// rows are parsed from JSON at runtime. This is the only catalogue view the chamber
/// builds its wizard on — see [`QuarkKind::available_agents`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueEntry {
    pub vendor: String,
    pub name: String,
    pub description: String,
    /// The boot command, or `None` for a registry `binary` entry — which the wizard must
    /// show as "needs a manual command" rather than offer as one click.
    pub command: Option<(String, Vec<String>)>,
    /// Whether Hadron has driven a real turn through this agent.
    pub proven: bool,
}

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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AcpTarget {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
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
        if let Some(target) = ACP_AGENTS.iter().find(|a| a.vendor == vendor).map(|a| AcpTarget {
            program: a.program.to_string(),
            args: a.args.iter().map(|s| s.to_string()).collect(),
            env: Vec::new(),
        }) {
            return Some(target);
        }
        if let Some(reg) = loader::load_cached_registry() {
            if let Ok(target) = loader::resolve_from_registry_data(&reg, vendor) {
                return Some(target);
            }
        }
        None
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
            Some(cmd) => Some(AcpTarget { program: cmd.program.clone(), args: cmd.args.clone(), env: Vec::new() }),
            None => AcpTarget::for_vendor(&seat.vendor),
        }
    }

    /// Build an [`AcpTarget`] for `seat` with its resolved secret environment
    /// from `store` attached so probe boots inherit required API keys.
    pub fn for_seat_with_env(seat: &Seat, store: &dyn hadron_lattice::secrets::SecretStore) -> Option<AcpTarget> {
        let mut target = Self::for_seat(seat)?;
        target.env = seat.resolve_env(store);
        Some(target)
    }

    /// Whether any part of this boot command names [`REPO_ROOT_TOKEN`].
    pub fn needs_repo_root(&self) -> bool {
        std::iter::once(&self.program)
            .chain(self.args.iter())
            .any(|s| s.contains(REPO_ROOT_TOKEN))
    }

    /// This target with [`REPO_ROOT_TOKEN`] replaced by the main checkout's root —
    /// **the only form that may be spawned.**
    ///
    /// A boot command that names a path in this repo cannot be written relative: the
    /// ACP transport spawns it with `Command::new(program)` and never sets
    /// `current_dir` (`agent-client-protocol`'s `AcpAgent::spawn_process`), so a
    /// relative program is resolved against whatever cwd the spawning process happens
    /// to have — the daemon inherits the chamber's, and the chamber's is wherever the
    /// human launched it from. Reproduced: launching the chamber from `target/release`
    /// made the `agy` seat's interpreter miss and every turn died with a bare
    /// `No such file or directory (os error 2)` that named nothing.
    ///
    /// The root is found from **`current_exe`**, not from the cwd (which is the bug) and
    /// not from `CARGO_MANIFEST_DIR` (which bakes in whichever checkout compiled the
    /// binary — a quark's worktree builds into the main checkout's shared `target/`, and
    /// the worktree has no `scripts/venv` because it is gitignored). `main_repo_root`
    /// asks git via `--git-common-dir`, so it answers the MAIN checkout's root even when
    /// asked from a linked worktree — which is exactly where the venv lives.
    ///
    /// Errs rather than passing an unresolved `{repo}` through to `spawn`: that would be
    /// a worse version of the same ENOENT, naming a path no human ever wrote.
    pub fn resolved(&self) -> anyhow::Result<AcpTarget> {
        if !self.needs_repo_root() {
            return Ok(self.clone());
        }
        let exe = std::env::current_exe()?;
        let near = exe.parent().unwrap_or(&exe);
        let root = crate::snapshot::main_repo_root(near).map_err(|e| {
            anyhow::anyhow!(
                "boot command {:?} names {REPO_ROOT_TOKEN}, but the main checkout root could \
                 not be found from {}: {e}",
                self.command_line(),
                near.display()
            )
        })?;
        let root = root.to_string_lossy();
        let sub = |s: &String| s.replace(REPO_ROOT_TOKEN, &root);
        Ok(AcpTarget {
            program: sub(&self.program),
            args: self.args.iter().map(sub).collect(),
            env: self.env.clone(),
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
    /// The short human blurb for a first-class preset. Best-effort presets have none and
    /// fall back to their command line — or, once merged below, to the registry's own
    /// description.
    fn preset_description(vendor: &str) -> &'static str {
        match vendor {
            "claude" => "Anthropic Claude Code, over ACP",
            "codex" => "OpenAI Codex CLI, over ACP",
            "gemini" => "Google Gemini CLI, over ACP",
            "agy" => "Google Antigravity (Gemini), via the bundled ACP bridge",
            _ => "",
        }
    }

    /// The full add-quark catalogue: the compiled presets **merged with** the published
    /// ACP registry ([`loader`]). This is the only view the chamber's "seat an agent"
    /// wizard renders from.
    ///
    /// The merge rule, keyed on vendor:
    ///
    /// * A **proven** preset wins — we have driven a real turn through that exact command
    ///   line, and no registry row outranks that.
    /// * Otherwise the **registry** wins. Its command comes from the agent's own
    ///   publisher, while the unproven presets are bare binary names guessed from a
    ///   package name (`goose`, `cline`, `cursor`) — precisely the "install the CLI
    ///   first" wall. A real `npx`/`uvx` command beats our guess every time.
    /// * A preset with no registry counterpart still shows, so nothing disappears.
    ///
    /// A `binary` agent lands with `command: None`: it is a real agent worth listing, but
    /// booting one means downloading and executing a third-party archive, which Hadron
    /// does not do. `None` is the honest answer and what the wizard greys out.
    pub fn available_agents() -> Vec<CatalogueEntry> {
        let mut out: Vec<CatalogueEntry> = ACP_AGENTS
            .iter()
            .map(|a| CatalogueEntry {
                vendor: a.vendor.to_string(),
                name: a.name.to_string(),
                description: Self::preset_description(a.vendor).to_string(),
                command: Some((
                    a.program.to_string(),
                    a.args.iter().map(|s| s.to_string()).collect(),
                )),
                proven: a.proven,
            })
            .collect();
        let Some(registry) = loader::load_cached_registry() else {
            return out;
        };
        for agent in &registry.agents {
            let vendor = agent.id.strip_suffix("-acp").unwrap_or(&agent.id);
            let blurb = agent.description.clone().unwrap_or_default();
            let command = loader::resolve_agent_command(agent)
                .ok()
                .map(|target| (target.program, target.args));
            match out.iter_mut().find(|e| e.vendor == vendor) {
                // A proven preset outranks the registry; leave ours alone.
                Some(existing) if existing.proven => {}
                // Ours was a guess — take the publisher's command and blurb.
                Some(existing) => {
                    existing.command = command;
                    if !blurb.is_empty() {
                        existing.description = blurb;
                    }
                }
                None => out.push(CatalogueEntry {
                    vendor: vendor.to_string(),
                    name: agent.name.clone(),
                    description: blurb,
                    command,
                    proven: false,
                }),
            }
        }
        out
    }

    /// The secret env-var NAMES a vendor needs supplied (via the OS keychain — see
    /// `hadron_lattice::secrets`). A FACT about the provider, kept here in the
    /// catalogue SSOT: the Antigravity SDK (`agy`) authenticates with a Gemini API
    /// key; the ACP agents that authenticate by OAuth/login (claude, codex, the
    /// gemini CLI) need none, so the chamber shows no API-key field for them.
    /// Empty for any vendor not listed. Extend as providers are confirmed to need a
    /// key — do not guess (a wrong entry shows a pointless field).
    pub fn secret_env_for(vendor: &str, transport: Transport) -> &'static [&'static str] {
        match (vendor, transport) {
            ("agy", Transport::Acp) | ("agy", Transport::Sdk) => &["GEMINI_API_KEY"],
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
    /// Per-seat energy limit (token ceiling).
    pub energy_limit: Option<u32>,
    /// Skill names this seat must NEVER be handed (hard lock). Matched against
    /// `skills::select`'s chosen name.
    pub deny_skills: Vec<String>,
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
    let energy_limit = spec.energy_limit;
    let deny_skills = spec.deny_skills.clone();
    let quark: Box<dyn Quark> = match spec.kind {
        QuarkKind::Cli(cli_spec) => Box::new(
            CliQuark::new(spec.id, spec.flavor, spec.model, cli_spec, ProcessRunner)
                .with_display_name(name)
                .with_roles(roles, exclusive)
                .with_commands(commands)
                .with_env(env)
                .with_energy_limit(energy_limit)
                .with_deny_skills(deny_skills),
        ),
        QuarkKind::Acp(target) => Box::new(
            AcpQuark::new(spec.id, spec.flavor, spec.model, spec.effort, spec.mode_config, target)
                .with_display_name(name)
                .with_roles(roles, exclusive)
                .with_commands(commands)
                .with_env(env)
                .with_energy_limit(energy_limit)
                .with_deny_skills(deny_skills),
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
        energy_limit: seat.energy_limit,
        deny_skills: seat.deny_skills.clone(),
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
                .with_env(seat.resolve_env(store))
                .with_energy_limit(seat.energy_limit)
                .with_deny_skills(seat.deny_skills.clone()),
        ));
    }
    build_seat(seat, store)
}

