use hadron_lattice::{CliSpec, Flavor, QuarkId, Seat, SeatCommands, Transport};

use crate::adapter::acp::AcpQuark;
use crate::adapter::cli::CliQuark;
use crate::adapter::local::{HttpTarget, LocalQuark};
use crate::adapter::runner::{ProcessRunner, RedactedEnv};
use crate::quark::Quark;

pub mod loader;

pub use loader::{
    parse_registry_json, resolve_agent_command, resolve_from_registry_data, AcpRegistryAgent,
    AcpRegistryData, AcpRegistryDistribution, RegistryError,
};

use presets::{ACP_AGENTS, REGISTRY_ALIASES};

/// The token a boot command uses for "the main checkout's root", resolved by
/// [`AcpTarget::resolved`] just before spawn.
///
/// It exists because a boot command may name a path inside this repo (the `agy`
/// bridge does), and there is no other honest way to write one: the catalogue is
/// compiled, the ACP registry is parsed from JSON, and a seat may supply its own
/// `command` in `team.json` — all three flow into the same [`AcpTarget`], so the
/// substitution has one home and covers all three.
pub const REPO_ROOT_TOKEN: &str = "{repo}";

/// The token for "the user's Hadron directory" (`~/.hadron`), resolved by
/// [`AcpTarget::resolved`] the same way as [`REPO_ROOT_TOKEN`] — but against
/// [`hadron_lattice::user_hadron_dir`], which needs no git and exists on every
/// install. `{repo}` only works from a source checkout (the files it names live in
/// the repository, and `cargo install` ships binaries only); `{hadron}` is how a
/// boot command names a path that a vendored/materialized asset can actually be
/// written to and found at on an installed build — see the agy bridge preset and
/// `notes/anchoring-a-boot-command-does-not-ship-it.md`.
pub const USER_HOME_TOKEN: &str = "{hadron}";

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
    /// A keyless local HTTP server (Ollama, LM Studio).
    Http(HttpTarget),
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

/// A boot command that has been through [`AcpTarget::resolved`] — no `{repo}` token
/// left, and no program that `execve` would resolve against the spawning cwd.
///
/// It exists because `resolved` used to return another [`AcpTarget`], so the two forms
/// were the same type and nothing stopped a call site from spawning the unresolved one.
/// That is exactly how the live `acp-agy` seat's relative `command` reached `spawn` and
/// died with a bare ENOENT. [`acp_stdio_descriptor`](crate::adapter::acp::session) takes
/// this type and only this type, so the descriptor handed to `AcpAgent::from_str` cannot
/// be built from anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAcpTarget(AcpTarget);

impl ResolvedAcpTarget {
    pub fn program(&self) -> &str {
        &self.0.program
    }

    pub fn args(&self) -> &[String] {
        &self.0.args
    }

    pub fn env(&self) -> &[(String, String)] {
        &self.0.env
    }

    /// The shell-ish command line, for diagnostics. Carries no secret: `env` is not in it.
    pub fn command_line(&self) -> String {
        self.0.command_line()
    }
}

/// A part that names a file relative to nothing: it has a separator, but is neither
/// absolute, nor `{repo}`-anchored, nor `{hadron}`-anchored, nor `~`-anchored (the
/// shell would expand that).
///
/// **Both tokens must be exempted here, not just substituted later.** This check runs
/// on the raw, pre-substitution string in more than one place (the seat-time guard
/// added in `0b8e9c05`, `mark_unseatable`'s resolvability probe) — a `{hadron}/…`
/// program that reached here without the carve-out would be misclassified as
/// "relative to the checkout" and refused, even though it never needed one.
fn is_repo_relative(part: &str) -> bool {
    part.contains('/')
        && !part.starts_with('/')
        && !part.starts_with(REPO_ROOT_TOKEN)
        && !part.starts_with(USER_HOME_TOKEN)
        && !part.starts_with('~')
}

/// `part` with `{repo}` substituted and, if it is repo-relative, anchored to `root`.
/// With `only_if_it_exists`, anchoring is skipped unless the joined path is really there
/// — the test that separates `crates/…/agy_acp.py` from `@scope/pkg` and `https://…`.
fn anchor(part: &str, root: &str, only_if_it_exists: bool) -> String {
    let part = part.replace(REPO_ROOT_TOKEN, root);
    if !is_repo_relative(&part) {
        return part;
    }
    let joined = std::path::Path::new(root).join(&part);
    if only_if_it_exists && !joined.exists() {
        return part;
    }
    joined.to_string_lossy().to_string()
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

    /// Whether any part of this boot command names [`USER_HOME_TOKEN`].
    pub fn needs_home_root(&self) -> bool {
        std::iter::once(&self.program)
            .chain(self.args.iter())
            .any(|s| s.contains(USER_HOME_TOKEN))
    }

    /// This target with [`USER_HOME_TOKEN`] substituted for the user's Hadron
    /// directory — unlike [`Self::anchored_by`]'s `{repo}` handling, this never
    /// requires a source checkout, so it runs unconditionally before that logic
    /// rather than sharing its git-backed root lookup. Errs rather than passing the
    /// token through to `spawn`: a program still containing a literal `{hadron}`
    /// would be a worse version of the same ENOENT, naming a path no human wrote.
    fn resolve_home_token(&self) -> anyhow::Result<AcpTarget> {
        if !self.needs_home_root() {
            return Ok(self.clone());
        }
        let home = hadron_lattice::user_hadron_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "boot command {:?} names {USER_HOME_TOKEN}, but the user's home \
                 directory could not be resolved — neither $HOME nor %USERPROFILE% is set",
                self.command_line()
            )
        })?;
        let home_str = home.to_string_lossy().to_string();
        Ok(AcpTarget {
            program: self.program.replace(USER_HOME_TOKEN, &home_str),
            args: self.args.iter().map(|a| a.replace(USER_HOME_TOKEN, &home_str)).collect(),
            env: self.env.clone(),
        })
    }

    /// Whether any part is a path written relative to the checkout — a part that names
    /// a file (it has a separator) but is neither absolute nor already `{repo}`-anchored.
    ///
    /// A seat's own `command` in `team.json` is written by hand and by the Settings UI,
    /// and nothing there teaches anyone about [`REPO_ROOT_TOKEN`] — the live global
    /// `team.json` had `crates/hadron-gluon/scripts/venv/bin/python` — so the token
    /// alone is not enough to decide whether resolution is needed.
    fn has_repo_relative_part(&self) -> bool {
        std::iter::once(&self.program)
            .chain(self.args.iter())
            .any(|s| is_repo_relative(s))
    }

    /// This target with [`REPO_ROOT_TOKEN`] replaced by the main checkout's root and
    /// every repo-relative path anchored to it — **the only form that may be spawned.**
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
    /// [`Self::resolved`], with the search origin injected so a test can exercise the
    /// installed case (no checkout above the binary) without installing anything.
    pub fn resolved_from(&self, near: &std::path::Path) -> anyhow::Result<ResolvedAcpTarget> {
        let this = self.resolve_home_token()?;
        this.anchored_by(near, || {
            crate::snapshot::main_repo_root(near).map_err(|e| e.to_string())
        })
    }

    /// [`Self::resolved_from`] with the root lookup supplied, so [`Self::resolved`] can
    /// memoise it. `lookup` is called only when this target actually needs a root — an
    /// `npx` seat must not pay for a git subprocess it has no use for.
    fn anchored_by(
        &self,
        near: &std::path::Path,
        lookup: impl FnOnce() -> Result<std::path::PathBuf, String>,
    ) -> anyhow::Result<ResolvedAcpTarget> {
        if !self.needs_repo_root() && !self.has_repo_relative_part() {
            return Ok(ResolvedAcpTarget(self.clone()));
        }
        let root = match lookup() {
            Ok(root) => Some(root),
            // A `{repo}` token or a relative program path requires a source checkout.
            // A relative arg may be an npm package spec (`@scope/pkg`), which can work
            // outside a git checkout if the program itself is not repo-relative.
            Err(e) if self.needs_repo_root() || is_repo_relative(&self.program) => {
                return Err(anyhow::anyhow!(
                    "boot command {:?} names a repository-relative path, so it only works from a source \
                     checkout: the files it boots live in the repository and are not installed \
                     by `cargo install`. Searched from {}: {e}",
                    self.command_line(),
                    near.display()
                ));
            }
            Err(_) => None,
        };
        let anchored = match &root {
            Some(root) => {
                let root_str = root.to_string_lossy().to_string();
                AcpTarget {
                    // A program holding a separator is opened as a path by `execve`, so
                    // anchoring it is always right: relative, it cannot work at all.
                    program: anchor(&self.program, &root_str, false),
                    // An arg is only a path if it names one that exists — otherwise it is
                    // a flag, a URL or an npm package spec, and must pass through whole.
                    args: self.args.iter().map(|a| anchor(a, &root_str, true)).collect(),
                    env: self.env.clone(),
                }
            }
            None => self.clone(),
        };
        if is_repo_relative(&anchored.program) {
            anyhow::bail!(
                "boot command program {:?} is a path relative to nothing — `execve` would \
                 resolve it against the spawning process's cwd. Anchor it with \
                 {REPO_ROOT_TOKEN} or give an absolute path.",
                anchored.program
            );
        }
        Ok(ResolvedAcpTarget(anchored))
    }

    /// The main checkout above THIS binary, looked up once per process.
    ///
    /// `main_repo_root` shells out to git, and [`Self::resolved`] is called from the
    /// roster reconcile and — through [`QuarkKind::available_agents`] — from the
    /// chamber's provider-wizard render fn, which runs on every frame. `current_exe`
    /// cannot move under a running process, so one answer is the only answer there is.
    ///
    /// **Only a success is cached.** `main_repo_root` goes through `git_with_env`, which
    /// returns `Err` on a `GIT_DEADLINE` timeout — a thing a loaded box under a
    /// concurrent swarm can produce. Caching that would refuse every repo-anchored ACP
    /// seat for the rest of the process's life, in a real checkout, over one blip. The
    /// cost of not caching it is a `git rev-parse` per lookup on a build where there is
    /// genuinely no checkout, and `anchored_by` only looks at all for a target that
    /// needs a root — in the catalogue, one row.
    fn installed_repo_root() -> Result<std::path::PathBuf, String> {
        static ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        if let Some(root) = ROOT.get() {
            return Ok(root.clone());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let near = exe.parent().unwrap_or(&exe);
        let root = crate::snapshot::main_repo_root(near).map_err(|e| e.to_string())?;
        Ok(ROOT.get_or_init(|| root).clone())
    }

    pub fn resolved(&self) -> anyhow::Result<ResolvedAcpTarget> {
        let this = self.resolve_home_token()?;
        let exe = std::env::current_exe()?;
        let near = exe.parent().unwrap_or(&exe).to_path_buf();
        this.anchored_by(&near, Self::installed_repo_root)
    }


    /// The shell-ish command line, for `AcpAgent::from_str` and for diagnostics.
    pub fn command_line(&self) -> String {
        std::iter::once(self.program.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Our vendor key for a published registry id: an explicit [`REGISTRY_ALIASES`]
/// entry, else the id with any `-acp` suffix stripped (`claude-acp` → `claude`),
/// else the id itself.
///
/// The alias table comes FIRST because the suffix rule is a guess and the table is
/// a fact — see [`REGISTRY_ALIASES`] for the six duplicate wizard rows it silently
/// produced.
fn vendor_for(registry_id: &str) -> &str {
    if let Some((_, vendor)) = REGISTRY_ALIASES.iter().find(|(id, _)| *id == registry_id) {
        return vendor;
    }
    registry_id.strip_suffix("-acp").unwrap_or(registry_id)
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
            let vendor = vendor_for(&agent.id);
            let blurb = agent.description.clone().unwrap_or_default();
            let command = loader::resolve_agent_command(agent)
                .ok()
                .map(|target| (target.program, target.args));
            match out.iter_mut().find(|e| e.vendor == vendor) {
                // A proven preset outranks the registry; leave ours alone.
                Some(existing) if existing.proven => {}
                // Ours was a guess — take the publisher's command and blurb.
                Some(existing) => {
                    match command {
                        // The publisher ships an `npx`/`uvx` command: it beats our guess whole.
                        Some(command) => existing.command = Some(command),
                        // `binary`-only. We will not download and execute a third-party
                        // archive, but the row still documents its ACP **argv**, so keep OUR
                        // program (the CLI the human installs themselves) and adopt those
                        // args. Eight agents — `goose`, `cursor`, `opencode`, `kimi`,
                        // `junie`, `poolside`, `stakpak`, `vtcode` — were greyed out as
                        // "Needs a manual command" despite the subcommand being published;
                        // the bare name alone would have been worse than useless, since an
                        // agent launched with no ACP flag starts its TUI and hangs on
                        // `initialize`. Nothing published and nothing to synthesise leaves
                        // our preset alone rather than blanking a command that may be right.
                        None => {
                            if let Some(args) = loader::binary_acp_args(agent).filter(|a| !a.is_empty()) {
                                if let Some((program, _)) = existing.command.take() {
                                    existing.command = Some((program, args));
                                }
                            }
                        }
                    }
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
        Self::mark_unseatable(&mut out, |t| t.resolved().is_ok());
        out
    }

    /// Drop the command of any entry whose boot command cannot resolve in THIS
    /// installation, so the wizard greys it out and says why.
    ///
    /// The `agy` bridge boots a python interpreter and a script that live in the Hadron
    /// repository, and `cargo install` ships binaries only: from an installed build the
    /// row is real but unseatable. Offering it anyway seated a quark that failed at turn
    /// time, minutes later, in the field — nowhere near the human who clicked it.
    ///
    /// `command: None` is the signal the wizard already has (a registry `binary` entry
    /// uses it), so this adds a reason, not a mechanism. Availability is DERIVED from
    /// [`AcpTarget::resolved`] rather than declared on the preset — one home for the rule.
    ///
    /// `resolvable` is injected for the same reason [`AcpTarget::resolved_from`] takes a
    /// search origin: a test in this checkout always HAS a repo root, so the installed
    /// case is unreachable otherwise.
    fn mark_unseatable(entries: &mut [CatalogueEntry], resolvable: impl Fn(&AcpTarget) -> bool) {
        for entry in entries.iter_mut() {
            let Some((program, args)) = entry.command.clone() else { continue };
            let target = AcpTarget { program, args, env: Vec::new() };
            if !resolvable(&target) {
                entry.description =
                    "needs a Hadron source checkout — the files it boots are not installed \
                     by `cargo install`"
                        .to_string();
                entry.command = None;
            }
        }
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
                // Fail at SEATING, not at turn time. A boot command that cannot resolve
                // in this installation — the `agy` bridge lives in the repository and
                // `cargo install` ships binaries only — used to seat fine and then error
                // every single turn, once per dispatch, forever. The seating loops report
                // and skip a seat that fails to build (`cli.rs`), so the swarm's idea of
                // its team stops claiming a quark that cannot boot. Resolved again at
                // `AcpSession::boot`: this is the early check, not the gate.
                let resolved = target.resolved()?;
                // Resolving proves the path is well-FORMED, not that anything is there.
                // `{hadron}` always resolves — every install has a home directory — so
                // an `agy` seat on a build whose bridge venv has never been provisioned
                // would seat happily and then die with a bare ENOENT once per dispatch,
                // forever: exactly the failure `{repo}`'s guard above was added to stop.
                // Only an ABSOLUTE program is checked; a bare `npx` is resolved against
                // `PATH` by `execve` and must not be stat'd here.
                //
                // Deliberately NOT in `resolved()`: that also backs the add-quark
                // wizard's availability probe (`mark_unseatable`), and greying the row
                // out on a machine with no venv would leave no way to create the seat
                // whose Settings page is the only thing that provisions one.
                let program = std::path::Path::new(resolved.program());
                if program.is_absolute() && !program.exists() {
                    anyhow::bail!(
                        "seat '{}' boots {} — that file does not exist. For the `agy` \
                         bridge, open Settings and select the seat to provision it \
                         (`adapter::bridge`); otherwise correct the seat's `command`.",
                        seat.id.as_str(),
                        program.display()
                    );
                }
                Ok(QuarkKind::Acp(target))
            }
            Transport::Sdk => anyhow::bail!(
                "seat '{}' uses the sdk transport, which is unsupported — Hadron has no \
                 native SDK adapter and none is planned; reach this provider over transport \
                 \"acp\" or \"cli\" instead",
                seat.id.as_str()
            ),
            Transport::Http => {
                let target = HttpTarget::for_seat(seat).ok_or_else(|| {
                    anyhow::anyhow!(
                        "seat '{}' is an http seat on vendor {:?}, which Hadron does not know \
                         how to reach over HTTP — use \"ollama\", \"lmstudio\", or \"openai-compatible\"",
                        seat.id.as_str(),
                        seat.vendor
                    )
                })?;
                Ok(QuarkKind::Http(target))
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
    /// Directories outside the worktree this seat's forge tools may reach (see
    /// `Seat::external_roots`). Empty = the jail is exactly what it always was.
    pub external_roots: Vec<hadron_lattice::ExternalRootSpec>,
    /// This seat's `model_params` (see `Seat::model_params`). Carried only onto
    /// `Transport::Http`, the one transport that composes its own request body —
    /// an ACP or CLI seat's model settings belong to the agent it boots.
    pub model_params: hadron_lattice::ModelParams,
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
    build_watched(spec, None)
}

/// The one construction site for every transport — [`build`] and
/// [`build_seat_watched`] are both this function, so a builder call added to an
/// arm cannot reach one caller and silently miss the other. It used to be two
/// copies of the same chains, and the Http copy was simply never written: that is
/// why `LocalQuark::watching` sat implemented with no caller and Ollama/OpenRouter
/// showed `working…` for a whole turn.
///
/// `live_dir` is where the quark publishes what it is doing mid-turn
/// (`hadron_lattice::live`). A plain CLI seat is deliberately left unwatched even
/// when `live_dir` is `Some`: it runs its process to completion and hands back one
/// blob, so there is nothing to watch until it is over.
fn build_watched(spec: QuarkSpec, live_dir: Option<&std::path::Path>) -> anyhow::Result<Box<dyn Quark>> {
    validate_quark_id(&spec.id)?;
    let name = spec.display_name.clone();
    let roles = spec.roles.clone();
    let exclusive = spec.exclusive;
    let commands = spec.commands.clone();
    let env = spec.env.clone();
    let energy_limit = spec.energy_limit;
    let deny_skills = spec.deny_skills.clone();
    let quark: Box<dyn Quark> = match spec.kind {
        QuarkKind::Cli(cli_spec) => {
            let watch = live_dir.filter(|_| cli_spec.stream.is_some());
            let mut q = CliQuark::new(spec.id, spec.flavor, spec.model, cli_spec, ProcessRunner)
                .with_display_name(name)
                .with_roles(roles, exclusive)
                .with_commands(commands)
                .with_env(env)
                .with_energy_limit(energy_limit)
                .with_deny_skills(deny_skills);
            if let Some(dir) = watch {
                q = q.watching(dir.to_path_buf());
            }
            Box::new(q)
        }
        QuarkKind::Acp(target) => {
            let mut q =
                AcpQuark::new(spec.id, spec.flavor, spec.model, spec.effort, spec.mode_config, target)
                    .with_display_name(name)
                    .with_roles(roles, exclusive)
                    .with_commands(commands)
                    .with_env(env)
                    .with_energy_limit(energy_limit)
                    .with_deny_skills(deny_skills)
                    .with_external_roots(spec.external_roots);
            if let Some(dir) = live_dir {
                q = q.watching(dir.to_path_buf());
            }
            Box::new(q)
        }
        QuarkKind::Http(target) => {
            let mut q = LocalQuark::new(spec.id, spec.flavor, spec.model, attach_http_api_key(target, &env))
                .with_display_name(name)
                .with_roles(roles, exclusive)
                .with_commands(commands)
                .with_energy_limit(energy_limit)
                .with_deny_skills(deny_skills)
                .with_model_params(spec.model_params);
            if let Some(dir) = live_dir {
                q = q.watching(dir.to_path_buf());
            }
            Box::new(q)
        }
    };
    Ok(quark)
}

/// The bearer token for a cloud HTTP vendor: whichever `secret_env` var the seat
/// declared, resolved to a value the same way ACP env is resolved above — never
/// read from `Seat` directly (see `HttpTarget::api_key`'s doc comment). Ollama/LM
/// Studio seats declare no `secret_env`, so `env` is empty and this is a no-op for
/// them. Pure and extracted out of `build()`'s match arm specifically so the
/// wiring itself (not just `HttpTarget`'s auth-header logic, already covered in
/// `adapter::local`'s tests) is unit-tested.
fn attach_http_api_key(target: HttpTarget, env: &[(String, String)]) -> HttpTarget {
    target.with_resolved_env(env)
}

/// Build a live quark from a team-config `Seat`. The seat's `transport` picks CLI vs
/// ACP; the seat's `vendor` (`claude`/`agy`/…) picks which one within that transport.
///
/// `store` resolves the seat's `secret_env` NAMES to VALUES (`Seat::resolve_env`) —
/// tests pass a `MemoryStore`; the daemon passes whatever backs its real credential
/// store. Resolution happens here, once, so every caller gets the same seam a real
/// keychain will eventually sit behind.
pub fn build_seat(seat: &Seat, store: &dyn hadron_lattice::secrets::SecretStore) -> anyhow::Result<Box<dyn Quark>> {
    build_watched(spec_for_seat(seat, store)?, None)
}

/// A `Seat` read out of `team.json` as the [`QuarkSpec`] both [`build_seat`] and
/// [`build_seat_watched`] construct from — one place, so a new `Seat` field
/// reaches the watched path and the unwatched one together.
fn spec_for_seat(seat: &Seat, store: &dyn hadron_lattice::secrets::SecretStore) -> anyhow::Result<QuarkSpec> {
    Ok(QuarkSpec {
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
        external_roots: seat.external_roots.clone(),
        model_params: seat.model_params.clone(),
    })
}

/// As [`build_seat`], but the quark also publishes what it is doing mid-turn into
/// `live_dir` (see `hadron_lattice::live`) so the chamber can render it.
///
/// Three transports have a mid-turn stream to publish: ACP always (it is
/// resident JSON-RPC), `Transport::Http` always (`LocalQuark` streams its chat
/// completion delta by delta), and a CLI seat only when its `CliSpec.stream` is
/// `Some` — a plain CLI adapter runs its process to completion and hands back one
/// blob, so there is nothing to watch until it is over. The per-transport rule
/// lives in [`build_watched`], not here.
pub fn build_seat_watched(
    seat: &Seat,
    live_dir: &std::path::Path,
    store: &dyn hadron_lattice::secrets::SecretStore,
) -> anyhow::Result<Box<dyn Quark>> {
    build_watched(spec_for_seat(seat, store)?, Some(live_dir))
}

