//! The team roster config: the seats the human has added. A **seat** is one
//! quark instance = an id bound to a provider (backing CLI) running a model.
//! Stored as `team.json` and read by both the daemon (to instantiate adapters)
//! and the chamber (to make each roster row legible: `id · provider · model`).
//!
//! Pure and offline: this only parses the config. Spawning adapters from it is
//! the daemon's job; annotating the roster is the chamber's.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Flavor, Mode, QuarkId};

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
    /// Reserved, unsupported: a native per-provider SDK adapter. Kept nameable
    /// (`sdk-agy`) so the transport axis stays first-class and the id namespace is
    /// reserved, but a native SDK is NOT on the roadmap — the only providers a raw
    /// SDK would reach are metered API keys, not the users-with-AI-plans path Hadron
    /// targets, so every provider is reached over CLI or ACP instead. `from_seat`
    /// rejects an `sdk` seat; do not present it as a build in progress.
    Sdk,
}

impl Transport {
    /// The short wire/id code: `"cli"` / `"acp"` / `"sdk"`. SSOT for every place that
    /// needs the bare transport word — the `<transport>-<vendor>` id prefix
    /// ([`id_follows_convention`]) and the chamber's roster/provider display both read
    /// this instead of repeating the match.
    pub fn code(&self) -> &'static str {
        match self {
            Transport::Cli => "cli",
            Transport::Acp => "acp",
            Transport::Sdk => "sdk",
        }
    }

    /// Build the conventional `<transport>-<vendor>` id for a pure vendor string, e.g.
    /// `Transport::Acp.conventional_id("claude")` → `"acp-claude"`. The SSOT counterpart
    /// to [`id_follows_convention`]: that function *checks* an id against the convention,
    /// this one *constructs* one, off the same `code()` — so a caller that just resolved a
    /// pure vendor (e.g. from a re-keyed preset list) never has to hand-format the prefix.
    pub fn conventional_id(&self, vendor: &str) -> String {
        format!("{}-{vendor}", self.code())
    }
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

/// Where the prompt text goes when a [`Transport::Cli`] adapter invokes its
/// subprocess. Two channels, covering the two CLIs Hadron has driven so far:
/// `claude` takes its prompt on stdin (no size limit); `agy` ignores stdin in
/// print mode and needs the prompt as the value of an argv flag.
///
/// Deliberately `#[default]` on [`PromptChannel::Stdin`], mirroring
/// [`Transport`]'s pattern — but note [`CliSpec::prompt`] itself is **not**
/// `#[serde(default)]`: a custom vendor must say which channel it means, since
/// silently defaulting to `Stdin` would misdrive an agy-shaped CLI that ignores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PromptChannel {
    /// Prompt piped to the subprocess's stdin.
    #[default]
    Stdin,
    /// Prompt is the value of an argv flag (e.g. `--print`); `flag: None` means the
    /// prompt rides as a bare positional argument instead.
    Arg {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flag: Option<String>,
    },
}

/// Whether — and how — a [`Transport::Cli`] adapter resumes a prior turn's
/// conversation instead of starting a fresh one each time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResumeMode {
    /// No resume support: every turn is a fresh, stateless invocation.
    #[default]
    None,
    /// Resume the most recent conversation with the given flag, e.g. agy's
    /// `--continue` (resumes the most recent conversation in the working directory).
    Continue { flag: String },
}

/// A CLI's own per-turn timeout flag, e.g. agy's `--print-timeout 29m`. Raised past
/// the engine's own turn deadline so the CLI gives up *because Hadron said so*, not
/// on a short default nobody chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutArg {
    pub flag: String,
    pub value: String,
}

/// The permission-gating flags to pass per [`Mode`], e.g. agy's `--mode plan` for
/// [`Mode::Ask`]. Defaults to all-empty — a CLI with no gating flags of its own
/// (the generic/raw case) simply gets no extra args for any posture.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostureMap {
    #[serde(default)]
    pub ask: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
    #[serde(default)]
    pub auto: Vec<String>,
    #[serde(default)]
    pub bypass: Vec<String>,
}

impl PostureMap {
    /// The flags for a given turn's resolved [`Mode`]. `Write` and `Auto` share the
    /// same posture on every CLI seen so far (edits auto, no raw shell-bypass), but
    /// they are stored as two fields — not merged — so a future CLI that *does*
    /// distinguish them needs no shape change here.
    pub fn for_mode(&self, mode: Mode) -> &[String] {
        match mode {
            Mode::Ask => &self.ask,
            Mode::Write => &self.write,
            Mode::Auto => &self.auto,
            Mode::Bypass => &self.bypass,
        }
    }
}

/// The CLI invocation shape for a [`Transport::Cli`] seat: how to build the
/// subprocess argv/stdin for one turn. Config-driven so reaching a new CLI vendor
/// is a `team.json` change, not a new adapter — the generic CLI transport this
/// type exists for.
///
/// All fields but `program` and `prompt` are `#[serde(default)]`, so a minimal
/// custom-CLI seat needs only those two; see [`CliSpec::generic`] for what the rest
/// default to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliSpec {
    /// The executable to spawn, e.g. `"agy"` or an absolute path to a CLI binary.
    pub program: String,
    /// Static leading args, applied before the prompt/model/resume/posture args.
    #[serde(default)]
    pub args: Vec<String>,
    /// Where the prompt text goes. Not defaulted — see [`PromptChannel`].
    pub prompt: PromptChannel,
    /// The flag that carries the model name, e.g. `"--model"`; `None` = never pass
    /// a model argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_flag: Option<String>,
    /// How (if at all) this CLI resumes a prior conversation.
    #[serde(default)]
    pub resume: ResumeMode,
    /// This CLI's own per-turn timeout flag, if it has one worth overriding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TimeoutArg>,
    /// Permission-gating flags per [`Mode`]. Defaults to all-empty.
    #[serde(default)]
    pub posture: PostureMap,
    /// Whether to apply the E2BIG `fit_prompt` argv-size guard (agy needs it: no
    /// stdin, so the whole prompt rides as one argv element, which Linux caps).
    #[serde(default)]
    pub argv_guard: bool,
}

impl CliSpec {
    /// The built-in `agy` preset. Mirrors `crates/hadron-gluon/src/adapter/agy.rs`
    /// exactly, so an existing `cli-agy` seat (no explicit `cli` spec, `vendor:
    /// "agy"`) behaves byte-for-byte once seats resolve through this type.
    ///
    /// SSOT note: this is the one place `agy`'s CLI flags are encoded as a
    /// [`CliSpec`]. `agy.rs` itself is untouched by this task and keeps its own
    /// copy until a later task rewires it onto this spec — see
    /// `agy_preset_matches_todays_agy_flags` for the pin that stops the two
    /// drifting apart in the meantime.
    pub fn agy() -> CliSpec {
        CliSpec {
            program: "agy".to_string(),
            args: Vec::new(),
            prompt: PromptChannel::Arg { flag: Some("--print".to_string()) },
            model_flag: Some("--model".to_string()),
            resume: ResumeMode::Continue { flag: "--continue".to_string() },
            timeout: Some(TimeoutArg {
                flag: "--print-timeout".to_string(),
                value: "29m".to_string(),
            }),
            posture: PostureMap {
                ask: vec!["--mode".to_string(), "plan".to_string()],
                write: vec!["--mode".to_string(), "accept-edits".to_string()],
                auto: vec!["--mode".to_string(), "accept-edits".to_string()],
                bypass: vec!["--dangerously-skip-permissions".to_string()],
            },
            argv_guard: true,
        }
    }

    /// Resolve a built-in preset by vendor name, e.g. `"agy"` → [`CliSpec::agy`].
    /// `None` for any vendor with no built-in preset — the seat then needs an
    /// explicit `cli` spec or a bare `command` (see the design doc's resolution
    /// order in §4.3).
    pub fn preset(vendor: &str) -> Option<CliSpec> {
        match vendor {
            "agy" => Some(CliSpec::agy()),
            _ => None,
        }
    }

    /// A generic spec for a bare `program` + `args`: prompt on stdin, raw stdout,
    /// no model flag, no resume, no timeout override, no posture gating, no argv
    /// guard. The "pipe prompt in, read reply out" default that works for most
    /// CLIs that were never specifically taught to Hadron.
    pub fn generic(program: String, args: Vec<String>) -> CliSpec {
        CliSpec {
            program,
            args,
            prompt: PromptChannel::Stdin,
            model_flag: None,
            resume: ResumeMode::None,
            timeout: None,
            posture: PostureMap::default(),
            argv_guard: false,
        }
    }
}

/// One seat: an identity bound to a provider (CLI/vendor) and a model. Same
/// provider with a different model is a different seat (independent trust).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub id: QuarkId,
    /// The human-readable name of the quark, e.g. "Google Girl".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The pure vendor, e.g. "claude", "agy", "codex". WAS `provider`, which smeared
    /// vendor and transport ("acp-claude"); `transport` is now the authoritative axis.
    /// `#[serde(alias)]` keeps an un-migrated team.json (with `provider`) parsing; the
    /// prefix it may carry is stripped by `normalize_vendor` in `parse_team`.
    #[serde(alias = "provider")]
    pub vendor: String,
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
    /// The program to boot. Primarily the ACP agent command: read whenever `transport`
    /// is [`Transport::Acp`], where absent means "resolve the command from `vendor`".
    /// Since the generic CLI transport landed, a [`Transport::Cli`] seat ALSO falls
    /// back to reading this field — as a bare `program`/`args` pair, wrapped into a
    /// generic [`CliSpec`] — when it has no explicit `cli` spec and its `vendor` has no
    /// built-in [`CliSpec::preset`] (see `QuarkKind::from_seat` in `hadron-gluon`'s
    /// adapter registry, and §4.3 of the custom-CLI-transport design doc). So this
    /// field is no longer ACP-exclusive; it is the shared "boot command" fallback for
    /// both transports, just consumed differently by each.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<AcpCommand>,
    /// The CLI invocation shape to use. Ignored unless `transport` is
    /// [`Transport::Cli`]; absent there means "resolve from `vendor`" —
    /// [`CliSpec::preset`] first, then a bare `command` (see §4.3 of the design
    /// doc). Mirrors the ACP `command` field's resolution pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<CliSpec>,
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
    /// Roles this seat plays for `@role` routing (e.g. `"architect"`, `"security"`).
    /// Empty by default: a seat with no roles is never a role-mention's preferred
    /// target, and every `team.json` written before role-routing existed decodes to
    /// this default. See `docs/superpowers/specs/2026-07-18-role-routing-design.md`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    /// Whether this seat is scoped ONLY to tasks that name one of its `roles`
    /// (a Phase 2 dispatch filter, not yet wired by this task). `false` by default,
    /// so a legacy `team.json` keeps every seat in general dispatch exactly as before.
    #[serde(default, skip_serializing_if = "is_false")]
    pub exclusive: bool,
}

/// `true`. Serde needs a function, and an absent `enabled` must mean *on* — a seat
/// that predates this field was never disabled.
fn enabled_by_default() -> bool {
    true
}

/// `skip_serializing_if` needs a `&bool -> bool` predicate; `false` is the default
/// for `Seat::exclusive`/`QuarkCard::exclusive` (same crate, same convention — see
/// `quark.rs`), so this is what keeps a seat/card that never opted in from growing
/// an `"exclusive":false` key in the file.
pub(crate) fn is_false(b: &bool) -> bool {
    !*b
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
        let Seat { id, display_name: _, vendor, model, flavor, transport, command, cli, enabled: _, effort, mode_config, roles, exclusive } = self;
        id == &other.id
            && vendor == &other.vendor
            && model == &other.model
            && flavor == &other.flavor
            && transport == &other.transport
            && command == &other.command
            && cli == &other.cli
            && effort == &other.effort
            && mode_config == &other.mode_config
            && roles == &other.roles
            && exclusive == &other.exclusive
    }

    /// A CLI seat — the shape every seat had before ACP. Keeps construction sites
    /// (and tests) from having to spell out two ACP fields they do not care about.
    pub fn cli(id: QuarkId, vendor: impl Into<String>, model: impl Into<String>, flavor: Flavor) -> Seat {
        Seat {
            id,
            display_name: None,
            vendor: vendor.into(),
            model: model.into(),
            flavor,
            transport: Transport::Cli,
            command: None,
            cli: None,
            enabled: true,
            effort: None,
            mode_config: None,
            roles: vec![],
            exclusive: false,
        }
    }

    /// Strip a leading transport prefix a legacy `provider` value may carry, leaving the
    /// pure vendor: "acp-claude" → "claude", "cli-agy" → "agy", "agy" → "agy". Idempotent.
    pub fn normalize_vendor(&mut self) {
        for prefix in ["cli-", "acp-", "sdk-"] {
            if let Some(rest) = self.vendor.strip_prefix(prefix) {
                self.vendor = rest.to_string();
                return;
            }
        }
    }
}

/// A per-repo override of one catalogue seat, layered over the global definition.
///
/// The definition — provider/model/command/transport plus the tunable knobs — lives
/// once in the global catalogue (`~/.hadron/team.json`). A repo names a quark by `id`
/// and records only what differs *here*: which role it plays, whether it participates,
/// and any per-repo adjustment to the model or the session knobs. Everything left
/// absent **inherits the catalogue's value**, so opening a fresh repo shows the shared
/// default (e.g. `acp-claude` = Opus) while another repo can pin its own (= Sonnet)
/// without touching the catalogue or the other repo.
///
/// **Two kinds of optionality, and they are not the same.** `flavor`/`enabled`/`model`
/// are `Option<T>`: absent = inherit. But `effort`/`mode_config`/`display_name` are
/// *already* `Option<String>` on [`Seat`], so a single `Option` could not tell "inherit
/// the catalogue" apart from "override it back to none/cleared". They are therefore
/// `Option<Option<String>>`: absent = inherit; `Some(None)` (serialized as JSON `null`)
/// = explicitly cleared here; `Some(Some(x))` = set to `x` here. Without this a repo
/// that clears an inherited `effort=high` would silently keep running `high`.
///
/// This is what "different orchestrator/model per repo" costs: a small delta row, not a
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
    /// Per-repo model, e.g. this repo runs `acp-claude` on Sonnet while the catalogue
    /// default (and every other repo) stays on Opus. Absent = inherit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Per-repo effort. Absent = inherit; `null` = cleared here; a value = set here.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_option"
    )]
    pub effort: Option<Option<String>>,
    /// Per-repo mode/config. Absent = inherit; `null` = cleared here; a value = set here.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_option"
    )]
    pub mode_config: Option<Option<String>>,
    /// Per-repo display name. Absent = inherit; `null` = cleared here; a value = set here.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_option"
    )]
    pub display_name: Option<Option<String>>,
    /// Per-repo role assignment. Absent = inherit the catalogue's `roles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,
    /// Per-repo exclusivity. Absent = inherit the catalogue's `exclusive`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusive: Option<bool>,
}

/// Deserialize an `Option<Option<T>>` field so the three states stay distinct: an
/// **absent** JSON key falls to `#[serde(default)]` = `None` (inherit), while a key that
/// **is present** — including an explicit `null` — is deserialized as the inner
/// `Option<T>` and wrapped in `Some`, giving `Some(None)` for `null` (cleared here) and
/// `Some(Some(x))` for a value (set here). Without this, serde reads a plain `null` as
/// the *outer* `None`, collapsing "cleared" back into "inherit".
fn present_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

impl SeatOverride {
    /// A role/state-only override with **no** definition delta — every def field inherits
    /// the catalogue verbatim. This is what a fresh adoption, a toggle, and the one-shot
    /// [`migrate_to_catalogue`] all write; callers that carry a per-repo adjustment start
    /// here and set the one field they change (`..SeatOverride::role(id)`), so the all-
    /// inherit baseline is defined in exactly one place.
    pub fn role(id: QuarkId) -> SeatOverride {
        SeatOverride {
            id,
            flavor: None,
            enabled: None,
            model: None,
            effort: None,
            mode_config: None,
            display_name: None,
            roles: None,
            exclusive: None,
        }
    }
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
        // Per-repo definition deltas, layered over the catalogue default. Absent =
        // inherit; for the already-optional knobs, `Some(None)` = cleared here.
        if let Some(model) = ov.model.clone() {
            seat.model = model;
        }
        if let Some(effort) = ov.effort.clone() {
            seat.effort = effort;
        }
        if let Some(mode_config) = ov.mode_config.clone() {
            seat.mode_config = mode_config;
        }
        if let Some(display_name) = ov.display_name.clone() {
            seat.display_name = display_name;
        }
        if let Some(roles) = ov.roles.clone() {
            seat.roles = roles;
        }
        if let Some(exclusive) = ov.exclusive {
            seat.exclusive = exclusive;
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
            flavor: Some(seat.flavor.clone()),
            enabled: Some(seat.enabled),
            ..SeatOverride::role(seat.id.clone())
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

/// Express a user's edit of a catalogue-adopted quark as a per-repo **delta** from the
/// catalogue default. `def` is the shared default (from the global catalogue), `desired`
/// is the seat the user wants *here*, and `prev` is any existing role/participation
/// override for this id (preserved). Each definition knob is carried only when it differs
/// from the default — so a knob left at the default inherits it (and stays in step if the
/// default later changes), while the catalogue and every other repo are untouched.
///
/// This is the inverse of the definition-layering in [`resolve_team`]: for any `def`, a
/// repo carrying `seat_override_delta(id, def, desired, prev)` resolves that id back to
/// `desired`. Proven by `a_settings_edit_becomes_a_delta_that_resolves_back_to_the_edit`.
pub fn seat_override_delta(
    id: QuarkId,
    def: &Seat,
    desired: &Seat,
    prev: Option<&SeatOverride>,
) -> SeatOverride {
    SeatOverride {
        flavor: prev.and_then(|o| o.flavor.clone()),
        enabled: prev.and_then(|o| o.enabled),
        model: (desired.model != def.model).then(|| desired.model.clone()),
        effort: (desired.effort != def.effort).then(|| desired.effort.clone()),
        mode_config: (desired.mode_config != def.mode_config).then(|| desired.mode_config.clone()),
        display_name: (desired.display_name != def.display_name)
            .then(|| desired.display_name.clone()),
        roles: (desired.roles != def.roles).then(|| desired.roles.clone()),
        exclusive: (desired.exclusive != def.exclusive).then_some(desired.exclusive),
        ..SeatOverride::role(id)
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

/// The one-shot legacy id renames, in one place so every consumer (the team.json pass
/// below and the chamber's ChamberPrefs key move) reads the SAME map. Only the two
/// built-ins that predate the `<transport>-<vendor>` convention; every other id is left
/// alone, so a user's custom id is never surprise-renamed.
pub fn legacy_id_renames() -> &'static [(&'static str, &'static str)] {
    &[("agy", "cli-agy"), ("opus", "cli-claude")]
}

/// Apply [`legacy_id_renames`] to a team in place: both full-seat ids and roster override
/// ids (a roster entry references a catalogue id, so it must move in lockstep). Idempotent
/// — an already-renamed id is not in the map's left column, so a second run changes nothing.
pub fn rename_legacy_ids(team: &mut Team) {
    let rename = |id: &mut QuarkId| {
        if let Some((_, new)) = legacy_id_renames().iter().find(|(old, _)| *old == id.as_str()) {
            *id = QuarkId::new(*new);
        }
    };
    for seat in &mut team.quarks {
        rename(&mut seat.id);
    }
    for ov in &mut team.roster {
        rename(&mut ov.id);
    }
}

/// Soft convention check: does `id` start with its transport prefix (`cli-`, `acp-`, `sdk-`)?
/// Advisory only — used to default new-seat ids and to warn, never to reject (custom ids like
/// `cli-agy-pro` stay legal).
pub fn id_follows_convention(id: &str, transport: Transport) -> bool {
    id.starts_with(&format!("{}-", transport.code()))
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
    let mut team: Team = serde_json::from_str(text).map_err(std::io::Error::other)?;
    for seat in &mut team.quarks {
        seat.normalize_vendor();
    }
    Ok(team)
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
}

#[cfg(test)]
mod resolve_tests {
    use super::*;
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
                flavor: Some(Flavor::Orchestrator),
                enabled: Some(false),
                ..SeatOverride::role(QuarkId::new("acp-claude"))
            }],
            max_exchanges: None,
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
        };
        let repo = Team {
            quarks: vec![],
            roster: vec![SeatOverride::role(QuarkId::new("q"))],
            max_exchanges: None,
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
                flavor: Some(Flavor::Worker),
                enabled: Some(false),
                ..SeatOverride::role(QuarkId::new("dup"))
            }],
            max_exchanges: None,
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
        let global = Team { quarks: vec![def.clone()], roster: vec![], max_exchanges: None };

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
mod cli_spec_tests {
    use super::*;

    #[test]
    fn cli_spec_serde_round_trips() {
        let spec = CliSpec {
            program: "mycli".into(),
            args: vec!["--flag".into()],
            prompt: PromptChannel::Arg { flag: Some("--print".into()) },
            model_flag: Some("--model".into()),
            resume: ResumeMode::Continue { flag: "--continue".into() },
            timeout: Some(TimeoutArg { flag: "--timeout".into(), value: "10m".into() }),
            posture: PostureMap {
                ask: vec!["--ask".into()],
                write: vec!["--write".into()],
                auto: vec!["--auto".into()],
                bypass: vec!["--bypass".into()],
            },
            argv_guard: true,
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
    }

    #[test]
    fn preset_resolves_agy_and_none_for_unknown() {
        assert_eq!(CliSpec::preset("agy"), Some(CliSpec::agy()));
        assert_eq!(CliSpec::preset("nonexistent-vendor"), None);
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
        assert_eq!(spec.resume, ResumeMode::None);
        assert_eq!(spec.timeout, None);
        assert_eq!(spec.posture, PostureMap::default());
        assert!(!spec.argv_guard);
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
