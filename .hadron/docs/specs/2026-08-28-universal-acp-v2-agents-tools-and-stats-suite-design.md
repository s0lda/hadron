# Universal ACP v2, Multi-Agent Swarm, Unified Tooling, and Next-Gen Stats Suite Design Specification

- **Date**: 2026-08-28
- **Status**: Validated & Approved (Bypass Autonomous Mode)
- **Target Crates**: `hadron-gluon`, `hadron-lattice`, `hadron-forge`, `hadron-forge-mcp`, `hadron` (`hadron-chamber`)

---

## 1. Executive Summary & Problem Statement

Hadron is a high-performance orchestration chamber and multi-agent swarm desktop environment. As AI tooling evolves rapidly, the swarm must scale to support every agent ecosystem, protocol version, and deployment topology seamlessly:

1. **Protocol Evolution & Universal Agent Support**:
   - The Agent Client Protocol (ACP) has advanced to **ACP v2** while many deployed agents remain on **ACP v1** or legacy JSON-RPC variations. Hadron must implement adaptive protocol version negotiation (favoring v2 while seamlessly falling back to v1) so that no agent is stranded.
   - First-class ACP support is required for Google Antigravity (`agy`), Claude Code (`claude`), Codex CLI (`codex`), Gemini CLI (`gemini`), GitHub Copilot (`copilot`), Cline (`cline`), Cursor (`cursor`), Mistral Vibe (`mistral`), Crow (`crow`), Minion (`minion`), Augment (`augment`), Docker Cagent (`cagent`), Fast-Agent (`fast-agent`), Goose (`goose`), and community ACP agents.
   - Non-ACP transports—Local HTTP (Ollama, LM Studio), Cloud OpenAI-compatible endpoints (OpenRouter, Groq, DeepSeek, Together), Python SDK Agy Bridge, CLI, and CLI streaming—must provide identical agility and full custom configuration (endpoints, headers, model selection, reasoning effort, temperature, mode profiles).

2. **Universal Tool Surface for All Swarm Agents**:
   - Every agent in the swarm must have access to Hadron's rich tool suite (over 50 specialized tools in `hadron-forge-mcp` and native `hadron-forge` tools, plus `context7` web docs) irrespective of whether the agent communicates over ACP, HTTP, Python Bridge, or CLI.
   - Robust capability negotiation and permission handling across all four security postures (`Ask`, `Write`, `Auto`, `Bypass`) ensure high autonomy without sacrificing worktree isolation or safe hash-checked editing.

3. **Exhaustive Multi-Dimensional Telemetry & Granular Stats Gathering**:
   - Collect every observable signal from every agent:
     - 4-way token breakdown: Fresh Input, Fresh Output, Cache-Read, Cache-Write, plus cumulative session and all-time aggregates.
     - Live context window consumption vs total capacity (`ContextUsage`), computing exact context saturation percentages.
     - Upstream quota buckets & rate limits: Provider-reported utilization (e.g. `_claude/rateLimit` utilization and `resetsAt`, Gemini 5-hour / weekly buckets, OpenRouter credit balances, Ollama evaluation durations).
     - Financial spend tracking (USD) across recognized model families (Claude 3.5/3.7/Opus/Sonnet/Haiku, Gemini 1.5/2.0/3.0 Pro/Flash, GPT-4o, o1, o3, DeepSeek V3/R1) with transparent unpriced indicators.
     - Operational metrics: Tool invocation counts, file edits, git commands, worktree snapshots, turn latency, and error rates.

4. **Next-Gen Chamber Visual & Technical Stats Suite**:
   - Transform the Chamber `Stats` view into an interactive command center:
     - Multi-window time scoping (`Current Turn`, `Session`, `24h Day`, `7d Week`, `30d Month`, `All Time`).
     - Real-time quota cards with circular progress rings, active/warning/critical status badges, exact time-to-reset countdown timers, and account-shared peer fallback labeling.
     - Interactive multi-series cumulative spend area chart / timeline (`SpendTimeline`) tracking per-quark trajectories and team totals.
     - Granular per-quark telemetry cards and tabular breakdowns with token metric chips, context saturation gauges, tool activity counts, and financial cost estimates.
     - Budget limit indicators and visual alerts (e.g., 80% warning / 95% critical exhaustion).

---

## 2. Architectural Approaches Evaluated

### Approach A: Monolithic Transport Branching
- **Concept**: Add ad-hoc version checks and tool switches inside each adapter (`acp/session.rs`, `local.rs`, `bridge.rs`).
- **Trade-offs**: High coupling, duplicated tool mapping schemas, high maintenance burden when new ACP versions or tools emerge.
- **Verdict**: Rejected. Violates Standard Model Rule 3 (SSOT) and Rule 10 (Simplicity & Modular Boundaries).

### Approach B: Layered Adapter Matrix with Shared Capabilities & Universal Stats Pipeline (RECOMMENDED)
- **Concept**:
  - Implement a dedicated **ACP Protocol Negotiator** (`ProtocolVersion::V2` primary with `ProtocolVersion::V1` fallback) and dynamic capability discovery (`mcpServers`, `configOptions`, `usage` telemetry).
  - Unify tool exposure via a **Shared Forge Tool Registry** feeding both MCP servers (`hadron-forge-mcp`) and OpenAI Function Calling schema generators (`local_tools.rs`).
  - Standardize telemetry ingestion into `hadron_lattice::Usage` and `hadron_lattice::QuotaBucket`, processed through a centralized `StatsAggregator` supporting multi-window slicing.
  - Build modern GPUI visual widgets (progress rings, countdown badges, stacked spend charts, token breakdown tables) in `hadron-chamber`.
- **Trade-offs**: Clean separation of concerns, robust invariant protection, future-proof for ACP v3+, and zero regression risk across existing seats.
- **Verdict**: Selected.

### Approach C: External Gateway Proxy
- **Concept**: Spin up a standalone local HTTP/gRPC proxy daemon translating all protocols into a single proprietary wire format.
- **Trade-offs**: Introduces inter-process latency, separate lifecycle failure modes, and breaks daemon process group isolation.
- **Verdict**: Rejected. Overcomplicated and violates Standard Model Rule 10.

---

## 3. System Architecture & Component Interactions

```
+---------------------------------------------------------------------------------------------------+
|                                      HADRON CHAMBER (GPUI UI)                                     |
|                                                                                                   |
|  +---------------------------+  +-------------------------------+  +---------------------------+  |
|  | Settings: Provider Wizard |  | Stats: Multi-Window Dashboard |  | Roster & Live Activity    |  |
|  | - ACP (v2/v1) Auto-Probe  |  | - Quota Cards & Countdowns    |  | - Context % Gauges        |  |
|  | - HTTP (Ollama/LM/Cloud)  |  | - Cumulative Spend Timeline   |  | - Token Breakdown Chips   |  |
|  | - Custom Adjustments      |  | - Per-Quark Telemetry Table   |  | - Real-time Status Badges |  |
|  +-------------+-------------+  +---------------+---------------+  +-------------+-------------+  |
+----------------|--------------------------------|--------------------------------|----------------+
                 | IPC / Events                   | Query / Projections            | Telemetry
                 v                                v                                v
+---------------------------------------------------------------------------------------------------+
|                                      HADRON GLUON (Swarm Daemon)                                  |
|                                                                                                   |
|  +---------------------------------------------------------------------------------------------+  |
|  |                                  Universal Adapter Engine                                   |  |
|  |                                                                                             |  |
|  |  +--------------------------+  +--------------------------+  +---------------------------+  |  |
|  |  | ACP Transport (v2 / v1)  |  | HTTP Transport           |  | Bridge & CLI Transports   |  |  |
|  |  | - Adaptive Handshake     |  | - Ollama / LM Studio     |  | - Python SDK (Agy ACP)    |  |  |
|  |  | - MCP Servers Provision  |  | - OpenRouter / Cloud     |  | - Subprocess Stream       |  |  |
|  |  | - Vendor Telemetry Hooks |  | - Function Calling Tools |  | - Prompt Tool Ingestion   |  |  |
|  |  +------------+-------------+  +------------+-------------+  +-------------+-------------+  |  |
|  +---------------|-----------------------------|------------------------------|----------------+  |
|                  |                             |                              |                   |
|                  v                             v                              v                   |
|  +---------------------------------------------------------------------------------------------+  |
|  |                           Unified Telemetry & Quota Aggregator                              |  |
|  |  - Token Spend (Input, Output, Cache Write, Cache Read, Fresh vs Cached)                     |  |
|  |  - Context Usage Tracking (Saturation %, Window Bounds, Truncation Warnings)               |  |
|  |  - Provider Quota Engine (Claude _meta, Gemini Buckets, OpenRouter Balance, Ollama Eval)    |  |
|  |  - Real-time Multi-Model USD Cost Matrix                                                    |  |
|  +---------------------------------------------------------------------------------------------+  |
+------------------------------------------------|--------------------------------------------------+
                                                 |
                                                 v
+---------------------------------------------------------------------------------------------------+
|                                     HADRON FORGE & MCP SUITE                                      |
|                                                                                                   |
|  +------------------------------+  +------------------------------+  +-------------------------+  |
|  | hadron-forge-mcp (Stdio MCP) |  | Native hadron-forge Tools    |  | External Tool Providers |  |
|  | - 50+ Specialized Tools      |  | - In-Process Hash-Edit Engine|  | - context7 (Web Docs)   |  |
|  | - Worktree Sandboxing        |  | - Jailed Exec Allowlist      |  | - Browser & DAP Debug   |  |
|  +------------------------------+  +------------------------------+  +-------------------------+  |
+---------------------------------------------------------------------------------------------------+
```

---

## 4. Subsystem 1: Universal ACP Protocol Engine (v2 & v1 Adaptive Negotiation)

### 4.1 Protocol Version Handshake & Adaptive Negotiation
- When establishing a connection with any ACP agent (`AcpAgent::from_str` or stdio process), the client negotiates capabilities using modern ACP standards:
  1. Attempt `InitializeRequest` with `ProtocolVersion::V2` (or highest supported version).
  2. If the agent returns an unsupported version error or rejects the handshake, fall back gracefully to `ProtocolVersion::V1`.
  3. Detect and record negotiated protocol version in `AcpSession::protocol_version` for diagnostic reporting.

### 4.2 Comprehensive ACP Agent Registry & Catalogue
- Ensure built-in, verified presets and zero-friction 1-click boot commands for the complete ACP agent ecosystem:
  - **`agy`**: Google Antigravity (`{hadron}/bridges/agy/venv/bin/python {hadron}/bridges/agy/agy_acp.py`).
  - **`claude`**: Claude Code (`npx -y @agentclientprotocol/claude-agent-acp@latest`).
  - **`codex`**: Codex CLI (`npx -y @agentclientprotocol/codex-acp@latest`).
  - **`gemini`**: Gemini CLI (`gemini --experimental-acp`).
  - **`copilot`**: GitHub Copilot (`copilot --acp`).
  - **`cline`**: Cline ACP (`cline`).
  - **`cursor`**: Cursor ACP (`cursor`).
  - **`mistral`**: Mistral Vibe (`mistral-vibe`).
  - **`crow`**: Crow CLI (`crow-cli`).
  - **`minion`**: Minion Code (`minion-code`).
  - **`augment`**: Augment Code (`augmentcode`).
  - **`cagent`**: Docker Cagent (`cagent`).
  - **`fast-agent`**: Fast-Agent (`fast-agent`).
  - **`goose`**: Goose ACP (`goose`).
  - **`kilo`**: Kilo CLI (`kilo`).
  - **Custom ACP Agents**: Arbitrary custom commands, environment variables, and argument lists with `{repo}` and `{hadron}` token expansion.

### 4.3 MCP Server Provisioning & Capability Injection
- On `NewSessionRequest`, Hadron injects standard and specialized MCP servers:
  - `hadron-forge-mcp`: Jailed worktree editing, AST rewriting, DAP debugging, git operations, trace slicing, flamegraph profiling, and sandboxed test execution.
  - `context7`: Real-time library documentation lookup via `@context7/mcp`.
  - Additional user-configured MCP servers declared in team settings.

### 4.4 Dynamic Session Configuration & Selector Resolution
- Autonomously query and configure session options advertised by the agent:
  - Model selector (`category: "model"`) matching requested seat model.
  - Reasoning effort selector (`category: "thought"` or `"effort"`).
  - Session execution mode (`category: "mode"`).

---

## 5. Subsystem 2: Universal Swarm Transports & Custom Adjustments

### 5.1 Local & Cloud HTTP Transports
- **Ollama**: Keyless `http://localhost:11434`, streaming NDJSON `/api/chat`, automatic model discovery via `/api/tags`.
- **LM Studio**: Keyless `http://localhost:1234/v1`, OpenAI-compatible Server-Sent Events `/chat/completions`, model discovery via `/models`.
- **Cloud OpenAI-Compatible (OpenRouter, Groq, DeepSeek, Together)**:
  - `Authorization: Bearer <key>` header resolution from secure keyring.
  - Custom base URL overrides (e.g. `https://openrouter.ai/api/v1`, `https://api.groq.com/openai/v1`, `https://api.deepseek.com/v1`).
  - Custom model naming, reasoning effort parameters, and temperature controls.

### 5.2 Python SDK Bridge & CLI Adapters
- First-class Agy Bridge integration with isolated venv management under `~/.hadron/bridges/agy`.
- Standard CLI and CLI streaming transports for subprocess-based quark seats with full environment redaction and stream capture.

---

## 6. Subsystem 3: Universal Tool Surface & Mediation Pipeline

### 6.1 Tool Surface Alignment Across Transports

| Tool Category | ACP Agents (via `hadron-forge-mcp`) | HTTP Agents (via `local_tools.rs`) | CLI / Bridge Agents (via Prompt/IPC) |
|---|---|---|---|
| **File Read / Inspect** | `hadron_forge_read_file`, `inspect` | `read_file`, `read_blocks`, `list_dir` | Jailed read primitives |
| **Hash-Based Editing** | `hadron_forge_edit`, `ast_rewrite` | `edit_block`, `create_file` | Hash-checked patch application |
| **Search & Navigation** | `grep`, `symbols`, `semantic` | `grep`, `find_files` | Project grep & symbol lookups |
| **Execution & Testing** | `exec`, `gate`, `mutation` | `exec` (jailed cargo/git allowlist) | Worktree bounded test runs |
| **Debugging & Profiling** | `dap_debug`, `trace_slicer`, `profile_runner` | Trace slicing & diagnostic formatters | Standard error logs |
| **External Docs & Web** | `context7` MCP, `browser` | Query documentation helpers | Injected reference manuals |

### 6.2 Security Postures & Permission Ladder
- **`Ask`**: Read-only conversation; all state-mutating tools strictly rejected.
- **`Write`**: Hash-checked forge file edits auto-approved; shell/exec commands gated.
- **`Auto`**: Safe allowlisted tools and forge edits auto-approved within worktree boundaries.
- **`Bypass`**: Full autonomous tool access within worktree sandbox.

---

## 7. Subsystem 4: Exhaustive Multi-Dimensional Telemetry & Stats Collector

### 7.1 Data Schema Extensions in `hadron-lattice` & `hadron-gluon`

```rust
pub struct ComprehensiveUsage {
    pub spend: TokenSpend,
    pub context: Option<ContextUsage>,
    pub model: Option<String>,
    pub quota: Vec<QuotaBucket>,
    pub latency_ms: Option<u64>,
    pub tool_calls_count: u64,
    pub edits_count: u64,
}

pub struct QuotaBucket {
    pub key: String,                  // e.g. "claude-limit", "gemini-5h", "openrouter-credits"
    pub remaining_fraction: f64,       // 0.0 to 1.0
    pub reset_time: Option<DateTime<Utc>>, // UTC timestamp of bucket renewal
}
```

### 7.2 Multi-Vendor Rate Limit & Telemetry Extractors
1. **Claude Code (`claude-agent-acp`)**: Extract `_meta["_claude/rateLimit"]` -> `utilization` & `resetsAt` timestamp.
2. **Google Antigravity / Gemini**: Extract 5-hour rolling and weekly quota buckets with renewal countdowns.
3. **OpenRouter / OpenAI Cloud**: Extract `x-ratelimit-remaining-*` headers and credit balance snapshots.
4. **Ollama / Local HTTP**: Extract `eval_count`, `prompt_eval_count`, `eval_duration`, and active VRAM model context parameters.

### 7.3 Real-Time Pricing Matrix
- Transparent USD cost computation per million tokens:
  - **Claude 3.7 / 3.5 Sonnet**: $3.00 Input / $15.00 Output / $3.75 Cache Write / $0.30 Cache Read.
  - **Claude 3.5 Haiku**: $0.80 Input / $4.00 Output / $1.00 Cache Write / $0.08 Cache Read.
  - **Claude 3 Opus**: $15.00 Input / $75.00 Output / $18.75 Cache Write / $1.50 Cache Read.
  - **Gemini 2.0 / 1.5 Pro**: $3.50 Input / $10.50 Output / $3.50 Cache Write / $1.75 Cache Read.
  - **Gemini 2.0 / 1.5 Flash**: $0.10 Input / $0.40 Output / $0.10 Cache Write / $0.025 Cache Read.
  - **GPT-4o**: $2.50 Input / $10.00 Output / $1.25 Cache Read.
  - **DeepSeek V3 / R1**: $0.14 / $0.55 Input (Cache Miss/Hit) / $2.19 Output.
  - **Custom / Local (Ollama, LM Studio)**: $0.00 (Self-hosted) with distinct local execution badge.

---

## 8. Subsystem 5: Next-Gen Chamber Stats & Spending Visualizer

### 8.1 Multi-Window Filter System
- **Tabs**:
  1. `Current`: Turn-level metrics since the last human message.
  2. `Session`: Live session since last `/clear`.
  3. `Day (24h)`: Rolling 24-hour aggregate.
  4. `Week (7d)`: Rolling 7-day aggregate across archives.
  5. `Month (30d)`: Rolling 30-day aggregate across archives.
  6. `All Time`: Full history across all stored session ledgers.

### 8.2 Interactive Visual Components in `hadron-chamber`
- **Quota & Reset Countdowns**:
  - Radial progress ring and horizontal fill bars color-coded by status (Green >40%, Yellow 15–40%, Red <15%).
  - Live formatted countdown timer (`"Resets in 3h 42m"` or `"Resets in 18m"`).
  - Explicit `"Account-shared"` badge when reading fallback metrics from a same-vendor peer.
- **Cumulative Spend Area Timeline (`SpendTimeline`)**:
  - Interactive multi-line stacked area chart displaying team total and per-quark trajectories over step indices.
- **Per-Quark Metric Grid**:
  - Rich cards displaying Quark Name, Role Badge, Transport Icon (ACP/HTTP/Bridge/CLI), Resolved Model.
  - Metric Pills: Fresh Input, Fresh Output, Cache Read, Cache Write, Context Saturation Gauge (% of max window), Estimated USD Cost, Tool Calls, and Edits count.
- **Summary Header KPI Tiles**:
  - Total Spend (USD), Total Fresh Tokens, Total Cache Traffic (Tokens saved), Context Window Saturation, Active Quarks, and Quota Health status.

---

## 9. Invariants, Security Bounds & Error Handling

### 9.1 Invariant Adherence
- **SSOT (Rule 3)**: Single source of truth for ACP presets in `crates/hadron-gluon/src/adapter/registry/presets.rs` and local vendors in `crates/hadron-gluon/src/adapter/local.rs`.
- **Honesty Rule**: Missing quota telemetry is treated as unknown/unreported, never fabricated as 0% or 100%.
- **Worktree Isolation**: Every tool call and ACP process is strictly anchored to its assigned git worktree (`/home/Jake/dev/hadron/.hadron/trees/<slug>`).

### 9.2 Security Review (Rule 7)
- **Secret Redaction**: API keys and tokens are stored in the OS keyring and passed exclusively via private environment descriptors (`RedactedEnv`) or `Authorization` headers. No secret ever leaks to argv, disk logs, or UI render streams.
- **Sandboxed Execution**: `exec` tools restrict commands to allowlisted binaries (`cargo`, `git`) and reject arbitrary shell syntax.

---

## 10. Verification Matrix & Testing Strategy

1. **Unit & Protocol Tests**:
   - `test_acp_v2_and_v1_handshake`: Verify `InitializeRequest` negotiation and fallback behavior.
   - `test_all_preset_catalogue_specifications`: Ensure every catalogued ACP agent has valid program/arg structures.
   - `test_telemetry_quota_aggregation`: Verify Claude, Gemini, and OpenRouter quota parsing with clock arithmetic.
   - `test_pricing_matrix_accuracy`: Verify cost calculations across token tiers and unpriced models.
2. **Integration Verification**:
   - Verify `hadron-gluon` and `hadron` build cleanly with zero compiler warnings or errors (`cargo check --workspace`).
   - Run end-to-end telemetry windowing tests on synthetic message logs (`cargo test -p hadron-chamber --lib model::stats`).
