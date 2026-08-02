use serde::{Deserialize, Serialize};

use crate::{Flavor, QuarkId};

use super::transport::{AcpCommand, CliSpec, Transport};

/// Per-seat command allow/deny lists, folded into the gatekeeper's
/// `AllowRules`/`DenyRules` under No-Human-Mode (a later task wires this in;
/// `hadron_gatekeeper::decide` is untouched by this one).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatCommands {
    /// Command-op patterns this seat may auto-run under No-Human-Mode (Auto).
    /// Each entry is an `op_matches` pattern: an exact command string or a
    /// trailing-`*` glob (e.g. `"git *"`). Empty = no config allow-list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
    /// Command-op patterns explicitly denied. A deny is absolute against the
    /// orchestrator under No-Human-Mode (see gatekeeper `decide`). Same pattern
    /// syntax as `allowed`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_allowed: Vec<String>,
}

impl SeatCommands {
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty() && self.not_allowed.is_empty()
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
    /// Per-seat command allow/deny lists, folded into the gatekeeper's
    /// `AllowRules`/`DenyRules` under No-Human-Mode. Empty by default, so a
    /// `team.json` written before this field existed decodes to no rules and
    /// behaves exactly as before.
    #[serde(default, skip_serializing_if = "SeatCommands::is_empty")]
    pub commands: SeatCommands,
    /// Names of environment variables whose VALUES are secrets kept in the OS
    /// credential store (see `secrets::SecretStore`). For `Transport::Cli`/`Acp`,
    /// injected into this seat's spawned subprocess; for `Transport::Http`, the
    /// first resolved value is sent as the `Authorization: Bearer` header instead
    /// (there is no subprocess to inject into — see `hadron_gluon::adapter::local`).
    /// Only the NAMES live here — never the values, which are never written to
    /// `team.json`. Empty by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_env: Vec<String>,
    /// Per-seat budget energy limit (token ceiling).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_limit: Option<u32>,
    /// Skill names this seat must NEVER be handed (hard lock, e.g. an image model
    /// never gets `writing-plans`). Matched against `skills::select`'s chosen name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny_skills: Vec<String>,
    /// Directories **outside** this quark's worktree that its forge tools may reach.
    ///
    /// Empty by default, and empty **is** the off state — there is no `Off` rung on
    /// `Mode` and none is needed. Per seat, never global: elevating one quark must not
    /// elevate the swarm. Enforced in `hadron-forge`'s `resolve_jailed_path`, which
    /// canonicalises each entry once and compares with `starts_with`; a path here is a
    /// *request*, and a root that does not exist on disk is dropped at spawn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_roots: Vec<ExternalRootSpec>,
    /// The base URL of this seat's local HTTP server, e.g. `http://localhost:11434`
    /// for Ollama. Ignored unless `transport` is [`Transport::Http`]; absent there
    /// means "resolve the vendor's default" (see `hadron_gluon::adapter::local`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_base_url: Option<String>,
    /// Per-seat model parameters (temperature, top_p, max_tokens).
    /// Empty by default, and omitted when empty.
    #[serde(default, skip_serializing_if = "ModelParams::is_empty")]
    pub model_params: ModelParams,
}

/// Per-seat model parameters (e.g. temperature, top_p, max_tokens).
/// Every field is optional: `None` means "use vendor/provider default".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

fn float_opt_eq(a: Option<f32>, b: Option<f32>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.to_bits() == y.to_bits(),
        _ => false,
    }
}

impl PartialEq for ModelParams {
    fn eq(&self, other: &Self) -> bool {
        float_opt_eq(self.temperature, other.temperature)
            && float_opt_eq(self.top_p, other.top_p)
            && self.max_tokens == other.max_tokens
    }
}

impl Eq for ModelParams {}

impl ModelParams {
    pub fn is_empty(&self) -> bool {
        self.temperature.is_none() && self.top_p.is_none() && self.max_tokens.is_none()
    }
}

/// One granted external directory, as written in `team.json`.
///
/// `writable` is the whole ladder: absent/false is read-only, which covers every case
/// observed so far (reading `~/.hadron/sessions/`, a sibling repo) at a fraction of the
/// risk. The enforcement type is `hadron_forge::file::ExternalAccess`; this is only the
/// wire shape, so `hadron-lattice` does not have to depend on the forge crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalRootSpec {
    pub path: std::path::PathBuf,
    #[serde(default, skip_serializing_if = "is_false")]
    pub writable: bool,
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
        let Seat { id, display_name: _, vendor, model, flavor, transport, command, cli, enabled: _, effort, mode_config, roles, exclusive, commands, secret_env, energy_limit, deny_skills, external_roots, http_base_url, model_params } = self;
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
            && commands == &other.commands
            && secret_env == &other.secret_env
            && energy_limit == &other.energy_limit
            && deny_skills == &other.deny_skills
            && external_roots == &other.external_roots
            && http_base_url == &other.http_base_url
            && model_params == &other.model_params
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
            commands: SeatCommands::default(),
            secret_env: Vec::new(),
            energy_limit: None,
            deny_skills: vec![],
            external_roots: vec![],
            http_base_url: None,
            model_params: ModelParams::default(),
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

    /// Resolve this seat's `secret_env` names to `(name, value)` pairs via `store`,
    /// for injection into the spawned subprocess env. A name with no stored value
    /// (or a store error) is SKIPPED — the agent then surfaces its own
    /// "missing credential" error, exactly as it does today when the env var is
    /// simply unset. The resolved VALUES are secrets: never log them, never put
    /// them in `team.json`, argv, the field, or a prompt.
    pub fn resolve_env(&self, store: &dyn crate::secrets::SecretStore) -> Vec<(String, String)> {
        self.secret_env
            .iter()
            .filter_map(|name| match store.get(&self.id, name) {
                Ok(Some(value)) => Some((name.clone(), value)),
                _ => std::env::var(name).ok().map(|val| (name.clone(), val)),
            })
            .collect()
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
    /// Per-repo command allow/deny lists. Absent = inherit the catalogue's `commands`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<SeatCommands>,
    /// Per-repo energy limit. Absent = inherit the catalogue's `energy_limit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_limit: Option<u32>,
    /// Per-repo skill locks. Absent = inherit the catalogue's `deny_skills`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny_skills: Option<Vec<String>>,
    /// Per-repo model parameters. Absent = inherit the catalogue's `model_params`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_params: Option<ModelParams>,
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
            commands: None,
            energy_limit: None,
            deny_skills: None,
            model_params: None,
        }
    }
}
