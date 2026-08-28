# Universal ACP v2, Multi-Agent Swarm, Unified Tooling, and Next-Gen Stats Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement universal ACP v2/v1 negotiation, universal swarm agent support (Antigravity ACP, Claude Code, Codex, Gemini, Copilot, Cline, Cursor, Mistral, Crow, Minion, Augment, Cagent, Fast-Agent, Goose, Kilo, Ollama, LM Studio, OpenRouter, and custom transports), provide full MCP and native tool access across all agents, collect granular telemetry/quota metrics from every provider, and visually upgrade the Chamber Stats dashboard with live quota cards, reset countdowns, cumulative area charts, and token metric breakdowns.

**Architecture:** A multi-layered architecture featuring an adaptive ACP protocol negotiator (`ProtocolVersion::V2` with `ProtocolVersion::V1` fallback) in `hadron-gluon`, unified tool capability injection into MCP servers (`hadron-forge-mcp`) and local OpenAI function calling declarations (`local_tools.rs`), exhaustive multi-vendor telemetry ingestion (`Usage`, `QuotaBucket`, `ContextUsage`), and an enhanced GPUI reactive Stats view in `hadron-chamber`.

**Tech Stack:** Rust 2021, `agent-client-protocol` (v1.2.0 / schema v1.4.0), `reqwest` (rustls-tls), `rmcp`, `gpui`, `hadron-lattice`, `hadron-gluon`, `hadron-forge`, `hadron-forge-mcp`, `rusqlite`, `chrono`.

## Global Constraints

- Standard Model Rules 0–11 strictly enforced (SSOT, prove it runs, make invalid states unrepresentable, evidence over adjectives, no unverified claims).
- Invariants: Lavapipe GPU software rendering compatibility, single font family, no unmediated arbitrary shell execution without worktree jail, honest quota reporting (empty means unprovided, not zero/full).
- No credentials or API keys exposed on command line arguments or logs; store in OS keyring via `hadron-chamber::app::settings::secrets` and pass securely via private descriptors.

---

### Task 1: Adaptive ACP Protocol Negotiator (v2 & v1 Fallback) & Comprehensive Agent Preset Catalogue

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/acp/session.rs`
- Modify: `crates/hadron-gluon/src/adapter/acp/model.rs`
- Modify: `crates/hadron-gluon/src/adapter/registry/presets.rs`
- Modify: `crates/hadron-gluon/src/adapter/registry/mod.rs`
- Test: `crates/hadron-gluon/src/adapter/acp/tests.rs`

**Interfaces:**
- Consumes: `agent_client_protocol::schema::ProtocolVersion`, `agent_client_protocol::schema::v1::InitializeRequest`, `ResolvedAcpTarget`.
- Produces: `AcpSession::protocol_version`, adaptive `InitializeRequest` negotiation, complete ACP catalog specifications for Antigravity (`agy`), Claude, Codex, Gemini, Copilot, Cline, Cursor, Mistral, Crow, Minion, Augment, Cagent, Fast-Agent, Goose, Kilo, and custom seats.

- [x] **Step 1: Write unit tests for protocol version negotiation and preset catalogue integrity (commit 8a962ba3)**
- [x] **Step 2: Implement adaptive handshake trying latest protocol version with graceful fallback to V1 (commit 8a962ba3)**
- [x] **Step 3: Update and expand ACP agent preset catalog with verified command definitions and aliases (commit 8a962ba3)**
- [x] **Step 4: Run cargo check and cargo test for hadron-gluon ACP adapter (commit 8a962ba3)**
- [x] **Step 5: Commit changes (commit 8a962ba3)**

---

### Task 2: Universal Tool Surface & MCP Capability Alignment Across All Transports

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/acp/session.rs`
- Modify: `crates/hadron-gluon/src/adapter/local_tools.rs`
- Modify: `crates/hadron-gluon/src/adapter/local.rs`
- Modify: `crates/hadron-forge-mcp/src/tools/mod.rs`
- Test: `crates/hadron-gluon/src/adapter/acp/tests.rs`

**Interfaces:**
- Consumes: `ForgeMcpServer`, `hadron_forge::file::Root`, `Mode` permission ladder.
- Produces: Universal tool availability across ACP (`hadron-forge-mcp`, `context7`) and HTTP Function Calling (`local_tools.rs` mapping 50+ forge primitives).

- [x] **Step 1: Write tests for permission ladder evaluation across Ask, Write, Auto, Bypass modes (commit 8a962ba3)**
- [x] **Step 2: Align `local_tools.rs` declarations with full forge capabilities (read, edit, diff, grep, exec, trace slicing, DAP) (commit 8a962ba3)**
- [x] **Step 3: Ensure ACP session initialization provisions both `hadron-forge-mcp` and `context7` with worktree anchoring (commit 8a962ba3)**
- [x] **Step 4: Verify tool execution in local HTTP and ACP mock sessions (commit 8a962ba3)**
- [x] **Step 5: Commit changes (commit 8a962ba3)**

---

### Task 3: Multi-Vendor Telemetry, Quota Extraction & Real-Time Cost Matrix

**Files:**
- Modify: `crates/hadron-lattice/src/telemetry.rs`
- Modify: `crates/hadron-gluon/src/adapter/acp/session.rs`
- Modify: `crates/hadron-gluon/src/adapter/acp/spend.rs`
- Modify: `crates/hadron-gluon/src/adapter/local.rs`
- Test: `crates/hadron-lattice/src/telemetry.rs` (tests module)

**Interfaces:**
- Consumes: Raw JSON-RPC `session/update` notifications, HTTP headers / SSE chunks.
- Produces: `Usage` struct with 4-way `TokenSpend` (input, output, cache-read, cache-write), `ContextUsage`, `QuotaBucket` vector with `resetsAt` timestamps, and `cost_usd()` real-time pricing calculation.

- [x] **Step 1: Write unit tests for Claude `_claude/rateLimit`, Gemini quota buckets, and OpenRouter credit parsing (commit 6656d1ff)**
- [x] **Step 2: Expand pricing matrix in `hadron_lattice::telemetry::Usage::cost_usd` for Claude 3.7, Gemini 2.0/Flash, GPT-4o, DeepSeek V3/R1 (commit 6656d1ff)**
- [x] **Step 3: Ensure Ollama and LM Studio telemetry (eval_count, prompt_eval_count, eval_duration) are properly captured (commit 6656d1ff)**
- [x] **Step 4: Run cargo test on hadron-lattice telemetry and spend calculations (commit 6656d1ff)**
- [x] **Step 5: Commit changes (commit 6656d1ff)**

---

### Task 4: Multi-Window Telemetry Aggregation & Spend Timeline Processing

**Files:**
- Modify: `crates/hadron-chamber/src/model/mod.rs`
- Modify: `crates/hadron-chamber/src/model/stats.rs`
- Test: `crates/hadron-chamber/src/model/stats.rs` (unit tests)

**Interfaces:**
- Consumes: `ChamberView::messages`, `archived_messages`, `StatsWindow`.
- Produces: `SessionStats` with 6-window scoping (`Current`, `Session`, `Day`, `Week`, `Month`, `AllTime`), `SpendTimeline` with cumulative multi-quark spend points, and unpriced quark accounting.

- [x] **Step 1: Write unit tests for `Day (24h)` window filtering and cumulative spend timeline calculation (commit 5ad1e4ac)**
- [x] **Step 2: Implement `StatsWindow::Day` and update window cutoff filters (commit 5ad1e4ac)**
- [x] **Step 3: Refine spend timeline interpolation to ensure accurate team and per-quark trajectories (commit 5ad1e4ac)**
- [x] **Step 4: Run cargo test for hadron-chamber stats model (commit 5ad1e4ac)**
- [x] **Step 5: Commit changes (commit 5ad1e4ac)**

---

### Task 5: Chamber Stats Dashboard Visual Redesign & Real-Time Monitoring UI

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/stats.rs`
- Modify: `crates/hadron-chamber/src/app/render/attention_hud.rs`
- Modify: `crates/hadron-chamber/src/app/settings/providers.rs`
- Test: `crates/hadron-chamber/src/app/render/stats.rs`

**Interfaces:**
- Consumes: `SessionStats`, `SpendTimeline`, `latest_quota`, `latest_context`.
- Produces: Modern GPUI dashboard featuring multi-window tab controls, live quota cards with reset countdowns, account-shared badges, cumulative spend timeline graph, per-quark metric cards, and budget warning indicators.

- [x] **Step 1: Build live quota card component with circular/bar progress, status coloring, and reset countdown strings (commit 0283f86f)**
- [x] **Step 2: Enhance cumulative spend timeline rendering with clear legends and step axes (commit 0283f86f)**
- [x] **Step 3: Upgrade per-quark cards and table with 4-way token chips, context saturation bars, cost metrics, and tool execution counts (commit 0283f86f)**
- [x] **Step 4: Wire summary KPI tiles in the stats header (commit 0283f86f)**
- [x] **Step 5: Run cargo check -p hadron and verify compilation (commit 0283f86f)**
- [x] **Step 6: Commit changes (commit 0283f86f)**

---

### Task 6: Swarm Integration, End-to-End Verification & Documentation

**Files:**
- Modify: `crates/hadron-gluon/src/lib.rs`
- Modify: `.hadron/nucleus/features.md`
- Test: Full workspace test suite (`cargo test --workspace`)

**Interfaces:**
- Consumes: All completed subsystems.
- Produces: Complete end-to-end operational verification across all agents, tools, telemetry collectors, and stats visualizers.

- [ ] **Step 1: Run full gate test suite across all workspace crates (`cargo test --workspace`)**
- [ ] **Step 2: Update feature map in `.hadron/nucleus/features.md` with updated stats and ACP v2 capabilities**
- [ ] **Step 3: Verify all invariants and ensure clean git tree state**
- [ ] **Step 4: Commit changes and prepare final report**
