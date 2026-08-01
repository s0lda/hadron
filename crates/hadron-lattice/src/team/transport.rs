use serde::{Deserialize, Serialize};

use crate::Mode;

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
    /// Reserved, unsupported: a native per-provider SDK adapter (e.g. a bespoke
    /// Anthropic/OpenAI SDK client). Kept nameable (`sdk-agy`) so the transport axis
    /// stays first-class and the id namespace is reserved, but a native SDK is NOT
    /// on the roadmap — CLI and ACP already reach the users-with-AI-plans providers
    /// this project targets, and [`Transport::Http`] already reaches the
    /// metered-API-key case over a wire format simple enough to need no
    /// per-provider SDK. `from_seat` rejects an `sdk` seat; do not present it as a
    /// build in progress.
    Sdk,
    /// An HTTP server: a local one on the user's own machine (Ollama, LM Studio —
    /// keyless), or a cloud OpenAI-compatible endpoint (OpenRouter, Groq, …), which
    /// authenticates via `Seat.secret_env` + an `Authorization: Bearer` header. No
    /// subprocess, no protocol handshake, in either case.
    Http,
}

impl Transport {
    /// The short wire/id code: `"cli"` / `"acp"` / `"sdk"` / `"http"`. SSOT for every
    /// place that needs the bare transport word — the `<transport>-<vendor>` id
    /// prefix ([`id_follows_convention`]) and the chamber's roster/provider display
    /// both read this instead of repeating the match.
    pub fn code(&self) -> &'static str {
        match self {
            Transport::Cli => "cli",
            Transport::Acp => "acp",
            Transport::Sdk => "sdk",
            Transport::Http => "http",
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
    /// Optional streaming output configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamSpec>,
}

/// Streaming output configuration for a one-shot CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamSpec {
    /// The stream format / protocol (e.g. `AgyStreamJson` or `Ndjson`).
    pub format: StreamFormat,
    /// Additional CLI flags needed to activate streaming (e.g. `["--output-format", "stream-json"]`).
    #[serde(default)]
    pub flags: Vec<String>,
}

/// The structure / format of lines emitted during streaming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamFormat {
    /// `agy`'s native stream-json NDJSON protocol.
    AgyStreamJson,
    /// Generic NDJSON carrying JSON path selectors for text deltas and usage.
    Ndjson {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_delta_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_tokens_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_tokens_path: Option<String>,
    },
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
            stream: None,
        }
    }

    /// The built-in `claude` preset. Drives `claude` CLI with tool mediation posture flags.
    pub fn claude() -> CliSpec {
        let mediation_args = vec![
            "--mcp-config".to_string(),
            "<hadron-forge-mcp>".to_string(),
            "--disallowedTools".to_string(),
            "Edit".to_string(),
            "Write".to_string(),
            "MultiEdit".to_string(),
            "NotebookEdit".to_string(),
        ];
        CliSpec {
            program: "claude".to_string(),
            args: Vec::new(),
            prompt: PromptChannel::Stdin,
            model_flag: Some("--model".to_string()),
            resume: ResumeMode::None,
            timeout: None,
            posture: PostureMap {
                ask: mediation_args.clone(),
                write: mediation_args.clone(),
                auto: mediation_args.clone(),
                bypass: mediation_args,
            },
            argv_guard: false,
            stream: None,
        }
    }

    /// The built-in `copilot` preset. Drives `copilot` CLI with tool mediation posture flags.
    pub fn copilot() -> CliSpec {
        let mediation_args = vec![
            "--additional-mcp-config".to_string(),
            "<hadron-forge-mcp>".to_string(),
            "--disallowedTools".to_string(),
            "Edit".to_string(),
            "Write".to_string(),
            "MultiEdit".to_string(),
            "NotebookEdit".to_string(),
        ];
        CliSpec {
            program: "copilot".to_string(),
            args: Vec::new(),
            prompt: PromptChannel::Stdin,
            model_flag: Some("--model".to_string()),
            resume: ResumeMode::None,
            timeout: None,
            posture: PostureMap {
                ask: mediation_args.clone(),
                write: mediation_args.clone(),
                auto: mediation_args.clone(),
                bypass: mediation_args,
            },
            argv_guard: false,
            stream: None,
        }
    }

    /// Resolve a built-in preset by vendor name, e.g. `"agy"` → [`CliSpec::agy`].
    /// `None` for any vendor with no built-in preset — the seat then needs an
    /// explicit `cli` spec or a bare `command` (see the design doc's resolution
    /// order in §4.3).
    pub fn preset(vendor: &str) -> Option<CliSpec> {
        match vendor {
            "agy" => Some(CliSpec::agy()),
            "claude" => Some(CliSpec::claude()),
            "copilot" => Some(CliSpec::copilot()),
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
            stream: None,
        }
    }
}
