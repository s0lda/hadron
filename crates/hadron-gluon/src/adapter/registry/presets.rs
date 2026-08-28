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
pub(super) struct AcpAgentSpec {
    /// The pure vendor a seat carries, e.g. `"claude"`.
    pub vendor: &'static str,
    /// What a human sees in the provider list.
    pub name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    /// Whether Hadron has completed a live ACP round-trip against this agent.
    pub proven: bool,
}

/// Registry ids that name an agent we already carry a preset for under a
/// different vendor key: `(registry id, our vendor)`.
///
/// [`super::QuarkKind::available_agents`] merges the published registry into the
/// preset list keyed on **vendor**, and it used to derive that key by stripping a
/// `-acp` suffix from the registry id. That rule matched `claude-acp`→`claude` and
/// `pi-acp`→`pi` and nothing else, so every agent whose publisher spells its id
/// differently from our vendor landed as a SECOND row: `github-copilot-cli`
/// alongside `copilot` is the one Jake reported, and there were six.
///
/// Keyed by the registry id because that is the string that varies — the vendor is
/// ours and stable. Guarded by `tests::no_two_catalogue_rows_name_the_same_agent`,
/// which fails if a registry refresh introduces another mismatch.
pub(super) const REGISTRY_ALIASES: &[(&str, &str)] = &[
    ("github-copilot-cli", "copilot"),
    ("factory-droid", "factory"),
    ("crow-cli", "crow"),
    ("minion-code", "minion"),
    ("mistral-vibe", "mistral"),
    ("qwen-code", "qwen"),
    ("auggie", "augment"),
    ("kilo-code", "kilo"),
    ("kilocode", "kilo"),
    ("antigravity-acp", "agy"),
    ("google-antigravity", "agy"),
];

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
pub(super) const ACP_AGENTS: &[AcpAgentSpec] = &[
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
        // The ONLY preset that names a path rather than a program on `PATH`, because
        // the bridge is a script rather than a published CLI. Written `{hadron}`-
        // anchored, NOT `{repo}`-anchored: the script and its venv are materialized
        // under `~/.hadron/bridges/agy` (`adapter::bridge`) precisely so this preset
        // works on an installed (`cargo install`) build with no source checkout — see
        // `notes/anchoring-a-boot-command-does-not-ship-it.md`. A bare relative path
        // would resolve by `execve` against the SPAWNING PROCESS's cwd — see
        // `AcpTarget::resolved`.
        #[cfg(windows)]
        program: "{hadron}/bridges/agy/venv/Scripts/python.exe",
        #[cfg(not(windows))]
        program: "{hadron}/bridges/agy/venv/bin/python",
        args: &["{hadron}/bridges/agy/agy_acp.py"],
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
        // `copilot` bare launches the interactive TUI and never speaks ACP, so the
        // client hangs forever on the `initialize` handshake ("never ending
        // connecting"). `--acp` is the CLI's documented "Start as Agent Client
        // Protocol server" flag; verified here to return a valid `initialize`
        // result (agent v1.0.73). Not `proven` — no full turn driven yet.
        args: &["--acp"],
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
        vendor: "kilo",
        name: "Kilo Code (ACP)",
        program: "kilo",
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
