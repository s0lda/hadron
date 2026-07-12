# Transport: ACP — a protocol path alongside the one-shot CLI

> **Execution status (2026-07-12): PLAN ONLY. Not started.** No code in this doc has been
> written. Every `file:line` reference points at the code *as it stands on
> `wip/2026-07-12-session`* (`edd72ef` + the working-tree edit to `engine.rs`), not at code this
> plan introduces. Protocol claims carry a URL; anything I could not verify is marked
> **UNVERIFIED** rather than asserted.

**Goal:** stop driving quarks with one-shot CLI subprocesses that re-send the entire conversation
through argv/stdin every turn, and give the gluon a *resident, streaming, structured* transport —
**ACP** (Agent Client Protocol, JSON-RPC 2.0 over stdio, Zed's) — **alongside** the CLI adapters,
which keep working untouched. The `Quark` trait is the seam.

---

## 0. Ground truth — the four failures, and who actually has them

The brief lists four failures of the CLI transport. Two are **agy-specific**, and that matters
enormously, because agy is the one provider that *cannot safely speak ACP today* (§2).

| # | Failure | Who actually has it | Where |
|---|---|---|---|
| 1 | Prompt rides argv; `execve` rejects an element over `MAX_ARG_STRLEN` (128 KiB) with E2BIG | **agy only.** `claude` takes its prompt on **stdin** (`claude.rs:71`, `CliInvocation { … stdin: prompt … }`), which has no such limit. The `fit_prompt` bandage and its `SAFE_ARG_BYTES = 96 KiB` budget exist solely for agy (`agy.rs:19-89`). | `agy.rs:10-30`, `runner.rs:58-72` |
| 2 | No structured output ⇒ `used_tokens: 0` | **agy only.** `ClaudeQuark` already parses the JSON envelope and reports `input_tokens + output_tokens` (`claude.rs:94-125`). `AgyQuark` falls through to `reply_to_outcome`, which hardcodes `used_tokens: 0` (`runner.rs:39-46`). | `runner.rs:42-44` |
| 3 | No resident session ⇒ whole context re-sent every turn | **Both — but unequally.** `claude` threads `--resume <session-id>` (`claude.rs:67-70`), so the *CLI* keeps its own session; we nonetheless re-render and re-send the whole field window as the prompt each turn (`engine.rs:395`, `prompt::build`). `agy` has no session at all: every turn is standalone. | `engine.rs:386-399` |
| 4 | No streaming ⇒ presence is binary | **Both, structurally.** `Quark::excite` returns **once**, at end of turn (`quark.rs:12`). The engine announces `Status{Excited}` before and a terminal status after (`engine.rs:575-580`, `finish_turn` `:409-479`). There is no channel on which a mid-turn token could arrive. | `quark.rs:6-13` |

**The window cap makes #3 worse in a different way** — correct, and it is now *policy* in two
places: `FIELD_WINDOW_BUDGET_BYTES = 48 KiB` in the engine (`engine.rs:105-153`) and the
`fit_prompt` net in the agy adapter. We throw away transcript to survive a transport limit.

### Two corrections to the premise

1. **The `Quark` trait already anticipates this.** Its doc comment: *"The gluon never knows whether
   this is a CLI harness, a native API worker, or a future ACP/MCP adapter — only this contract"*
   (`quark.rs:5`). The seam is real. But the contract as written (`async fn excite(&mut self, turn:
   Projection) -> Result<TurnOutcome>`) **cannot express streaming**, so a drop-in `AcpQuark`
   fixes failures 1–3 and *not* 4. Failure 4 needs a signature change (§3b). Say it plainly: adding
   ACP without touching the trait leaves presence exactly as blind as it is today.
2. **`Projection` already carries `cwd`** (`projection.rs:26-32`), a `PathBuf`, non-optional, and
   `ProcessRunner` applies it (`runner.rs:62`). This is a *gift*: ACP's `session/new` takes a
   required `cwd` param
   ([session-setup](https://agentclientprotocol.com/protocol/v1/session-setup)), so the worktree
   plan's cwd chain lands on ACP with zero adaptation.

---

## 1. What ACP is, and what it buys us — one-to-one against the four failures

ACP is JSON-RPC 2.0 between a **client** (editor / orchestrator — that would be hadron) and an
**agent** (a subprocess spoken to over stdin/stdout, or a remote over HTTP/WS). The client boots the
agent **once** and holds the connection; one connection can carry several concurrent sessions.
Stable protocol version is `1`; official **Rust** crate `agent-client-protocol` **v1.2.0**
(published 2026-07-07, ~3.1M downloads) — hadron is Rust, so this is a direct dependency, not a
port. ([intro](https://agentclientprotocol.com/get-started/introduction),
[architecture](https://agentclientprotocol.com/get-started/architecture),
[repo](https://github.com/agentclientprotocol/agent-client-protocol),
[crates.io](https://crates.io/crates/agent-client-protocol))

| Failure | Does ACP fix it? | Mechanism (cited) |
|---|---|---|
| **1. argv E2BIG** | **YES — structurally.** | The prompt is a JSON-RPC `session/prompt` param over **stdio**, never argv. `MAX_ARG_STRLEN` cannot apply. `fit_prompt` and `SAFE_ARG_BYTES` become dead code *for any quark on the protocol path*. ([prompt-turn](https://agentclientprotocol.com/protocol/v1/prompt-turn)) |
| **2. `used_tokens: 0`** | **PARTIALLY.** | Stable today: the agent MAY send a `session/update` with `sessionUpdate: "usage_update"`, carrying **required** `used` and `size` (context tokens used / context window size) and an **optional** `cost { amount, currency }`. That is a *real number for the context UI* — arguably better than what we show for claude now. But **per-turn** token totals (`inputTokens`/`outputTokens`/`cachedReadTokens`…) are a **DRAFT RFD** (`end-turn-token-usage`), *not* stable protocol. Hadron's `Kind::EnergyReport { used_tokens: u32 }` (`event.rs:95`) is per-turn, so on ACP we either derive a delta from cumulative `used`, or take `cost` as the ledger currency. ([prompt-turn §usage](https://agentclientprotocol.com/protocol/v1/prompt-turn), [RFD: end-turn token usage — DRAFT](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/rfds/end-turn-token-usage.mdx)) |
| **3. No resident session** | **YES.** | `session/new { cwd, mcpServers }` → a `sessionId`; every later turn is `session/prompt { sessionId, prompt }`. The agent holds the conversation. `session/load` (capability `loadSession`) replays a session after a daemon restart; `session/resume` (capability `sessionCapabilities.resume`) restores *without* replay. So the field window we send becomes *the new message*, not the whole transcript — the quadratic re-send goes away, and with it the reason we were **throwing away context** to fit a transport limit. ([session-setup](https://agentclientprotocol.com/protocol/v1/session-setup)) |
| **4. No streaming** | **YES at the protocol layer; NO until we change the `Quark` trait.** | The agent streams `session/update` notifications mid-turn: `agent_message_chunk` (text as generated, with an opaque `messageId`), `plan`, `tool_call` with `in_progress`/`completed` status, `usage_update`. The `session/prompt` *response* only lands at the end, carrying a `StopReason` (`end_turn` / `max_tokens` / `refusal` / `cancelled` / `max_turn_requests`). Our `excite` has nowhere to put the chunks. ([prompt-turn](https://agentclientprotocol.com/protocol/v1/prompt-turn)) |

**Bonus, not in the brief but load-bearing for us:** ACP has **real mid-turn permission gating**.
The agent calls `session/request_permission { sessionId, toolCall, options[] }` and *blocks* on the
client's answer; each option carries an `optionId`, a display `name`, and a `kind` (the spec's
example shows `allow_once` and `reject_once`; the `*_always` kinds that would back trust-on-first-use
are **UNVERIFIED** — confirm against the schema before building `Mode::Auto` on them).
([tool-calls](https://agentclientprotocol.com/protocol/v1/tool-calls)) This is exactly the
`canUseTool`-shaped hole recorded in `MEMORY.md` (*"true Auto TOFU deferred to Agent SDK
canUseTool"*) and in `claude.rs:14-18` (*"Auto's per-command trust-on-first-use list is not
expressible against this CLI"*). On ACP, `Mode::Auto` becomes expressible **without a per-vendor
SDK**: `allow_always` ⇒ append `PermissionGrant { approved: true, remember: true }`, which
`hadron_gatekeeper::allow_rules` already folds. And `session/cancel` gives us a real stop button,
which today does not exist.

**Also fixed, quietly:** `agy`'s posture flags are marked *"NEEDS LIVE VALIDATION"* (`agy.rs:95-99`)
because a naive `--mode plan` confused the parser. Posture-by-flag-string is guesswork per CLI; on
ACP, permission is a *protocol request we answer*, not an argv incantation we hope parses.

### What ACP does **not** fix
- **It is not a model API.** It does not give us cheaper tokens, a different context window, or
  prompt caching. §5's cost argument is about *not re-sending*, nothing more.
- **It does not give us per-turn token accounting** (DRAFT, above).
- **It does not make agy work.** See §2 — this is the crux.
- **MCP is not a substitute and never was.** MCP connects an *agent* to *tools/context*; ACP
  connects a *client* to an *agent*. They compose: the ACP client passes `mcpServers` config in
  `session/new`, and the agent then connects to those MCP servers itself. If hadron wanted to
  *expose* the field to quarks as a tool, that is an MCP server — a different, additive piece of
  work that does nothing about any of the four failures. ([architecture §MCP](https://agentclientprotocol.com/get-started/architecture))

---

## 2. What each provider actually supports **today** (2026-07-12)

This is where a plan built on wishes dies. Be brutal.

| Provider | ACP today | How | Maturity / risk |
|---|---|---|---|
| **claude** (our orchestrator seat) | **Yes, via an adapter — not native.** | `@agentclientprotocol/claude-agent-acp` (was `@zed-industries/claude-code-acp`): a **Node** process that wraps the **Claude Agent SDK** and speaks ACP. Listed on the ACP agents page as *"via Zed's SDK adapter"*. | Actively maintained (v0.58.1, 2026-07-09; 121 releases; ~2.2k stars). Community/Zed-maintained, **not Anthropic-official**. Feature list: tool calls **with permission requests**, edit review, TODO lists, terminals, slash commands, client MCP servers. It uses the Agent SDK's `canUseTool` callback for permissions. ([agents](https://agentclientprotocol.com/overview/agents), [claude-agent-acp](https://github.com/agentclientprotocol/claude-agent-acp), [Zed blog](https://zed.dev/blog/claude-code-via-acp), [SDK permissions](https://docs.claude.com/en/docs/agent-sdk/permissions)) |
| **agy** (our worker seat, Antigravity/Gemini) | **NO.** | There is **no `agy --acp`**. The feature request ([antigravity-cli#31](https://github.com/google-antigravity/antigravity-cli/issues/31), open since 2026-05-20) asks for exactly the JSON-RPC-over-stdio mode `gemini-cli --acp` had; **no Google/maintainer response**. Third-party bridges exist ([antigravity-acp](https://github.com/shubzkothekar/antigravity-acp), `agy-acp`). | **Do not build on this.** The third-party adapter's own README warns that Google's ToS forbid *"third party software, tools, or services to access the Service"*, and that driving an OAuth-logged-in `agy` through it risks **account suspension**. It suggests Vertex/AI-Studio API keys instead — **UNVERIFIED** whether that actually clears the ToS. |
| **gemini-cli** (the ACP-native Google agent) | **Yes, native.** | Listed native on the ACP agents page. | **But it is being wound down for our tier:** Google stops serving Gemini CLI requests for AI Pro/Ultra and unpaid individual tiers on **2026-06-18**, directing users to **Antigravity CLI** — the one with no ACP. This is the trap: the Google agent that speaks ACP is the one we are being pushed off. |
| **Local: Ollama / LM Studio** | **Not directly — and they never will be.** | Ollama and LM Studio are **inference servers** (OpenAI-compatible HTTP APIs), not *agents*: no tool loop, no sessions, no permission requests. Nothing to speak ACP *with*. The path is an **agent harness** that is ACP-native and provider-agnostic: **Goose** (ACP server mode; native ACP on the agents list; supports Ollama / LM Studio / any OpenAI-compatible endpoint) — also **opencode** and **Qwen Code**, both native ACP. ([Goose ACP](https://zed.dev/acp/agent/goose), [Goose providers](https://goose-docs.ai/docs/getting-started/providers/), [agents](https://agentclientprotocol.com/overview/agents)) |

**The sentence the plan exists to make impossible to miss:**

> The two failures that hurt *most* (E2BIG, `used_tokens: 0`) are **agy's**, and agy is the single
> provider ACP **cannot** reach today. ACP's safe reach is claude (adapter) and local models (via
> Goose) — where it fixes the *universal* failures (#3 resident session, #4 streaming) and buys us
> real permission gating. It does nothing for agy until Google ships `--acp`.

So: **the byte-budget bandage on `agy` stays.** It is not a stopgap to be deleted by this plan; for
agy it is the permanent transport, until #31 lands.

---

## 3. The migration — CLI adapters keep working; a protocol path lands beside them

### 3a. The seam is `Quark`, and the registry is where it forks

Today (`registry.rs:9-23`) `QuarkKind` is `{ Claude, Agy }` mapped from `Seat.provider`
(`team.json`). The change is **additive at the registry**, so nothing existing moves:

```rust
pub enum QuarkKind {
    Claude,          // CLI, unchanged: `claude -p --output-format json`, prompt on stdin
    Agy,             // CLI, unchanged: `agy --print <prompt>` + fit_prompt byte cap
    Acp(AcpTarget),  // NEW: a resident ACP agent subprocess
}

/// How to boot the agent. Comes straight from team.json, so a new provider is a
/// config change, not a code change — which is the whole point of a protocol.
pub struct AcpTarget { pub program: String, pub args: Vec<String>, pub env: Vec<(String, String)> }
```

`Seat.provider` gains values: `"acp-claude"` → `npx @agentclientprotocol/claude-agent-acp`,
`"acp-goose"` → the Goose ACP server, `"acp"` → free-form `program`/`args` from the seat. The
existing `"claude"` / `"agy"` values keep resolving to today's `ClaudeQuark` / `AgyQuark`,
byte-for-byte. **A field/team written today still runs tomorrow.** `Seat` (in
`hadron-lattice/src/team.rs`) gains optional `program`/`args` fields, `#[serde(default)]`, same
additive discipline as `QuarkCard::provider`.

### 3b. The trait change — the load-bearing part, because of failure #4

`Quark::excite` returns once. Streaming needs a mid-turn channel. Mirror the worktree plan's cwd
chain: enumerate every layer, test each, because a silent break anywhere means presence goes blind
again with no error.

```rust
// hadron-lattice: what a quark can say WHILE it works. Not a field event — see below.
pub enum Presence {
    Chunk { text: String },                 // agent_message_chunk
    Tool  { name: String, status: String }, // tool_call in_progress / completed
    Plan  { entries: Vec<String> },
    Usage { used: u32, size: u32 },         // usage_update: context used / window size
}

#[async_trait]
pub trait Quark: Send {
    fn id(&self) -> QuarkId;
    fn flavor(&self) -> Flavor;
    fn energy(&self) -> EnergyState;
    /// Run one turn. `presence` is a best-effort sink for mid-turn updates: a CLI
    /// quark simply never sends on it. Dropping it must NOT fail a turn.
    async fn excite(&mut self, turn: Projection, presence: PresenceSink) -> anyhow::Result<TurnOutcome>;
}

pub type PresenceSink = tokio::sync::mpsc::UnboundedSender<(QuarkId, Presence)>;
```

| # | Layer | Today | Change |
|---|---|---|---|
| 1 | `Presence` + `PresenceSink` types | — | new, in `hadron-lattice` (the chamber must see them too, and the chamber does not depend on the gluon) |
| 2 | `Quark::excite` signature | `quark.rs:12` | `+ presence: PresenceSink` |
| 3 | `ClaudeQuark` / `AgyQuark` | `claude.rs:87`, `agy.rs:158` | accept and **ignore** it. One line each. This is why the CLI path keeps working. |
| 4 | Every test quark in `engine.rs` (`PermissionQuark` `:751`, `EchoQuark` `:812`, `OverlapQuark`, …) | ~6 impls | accept and ignore. Mechanical churn; the compiler enumerates it. |
| 5 | Engine holds the receiver | `run_until_quiesce` `:494-660` | create the channel once; pass a clone into each `turns.spawn` (`:583-587`); drain it in the `tokio::select!` that already races `join_next` against `FIELD_POLL` (`:606-609`) — a **third** branch, no new task |
| 6 | Where presence *goes* | — | **Decision (below)** |

**Decision: streamed chunks do NOT go in the field.** `field.jsonl` is the append-only,
reconstruct-everything-from-it log; token-by-token chunks would bloat it by orders of magnitude and
poison `bounded_window` (`engine.rs:141`), which every future projection is built from. Instead the
engine keeps an **in-memory `HashMap<QuarkId, String>` of live presence** and writes it to
`.hadron/presence.json` (gitignored, alongside `field.jsonl`) — last-writer-wins, debounced (~100 ms),
truncated (~4 KiB per quark). The chamber already watches `.hadron/`; it reads presence the same way
it reads the field, and *nothing about the field's semantics changes*. When the turn ends, the final
message goes into the field as today (`finish_turn` `:422-426`) and the presence entry is cleared.

> This also keeps the streaming work **separable**: layers 1–5 can ship with every adapter ignoring
> the sink (no behaviour change, all tests green), and the ACP adapter + the chamber's presence view
> land after.

### 3c. `AcpQuark` — the new adapter (`crates/hadron-gluon/src/adapter/acp.rs`)

Built on the official Rust crate `agent-client-protocol = "1.2"` — hadron implements the ACP
**Client** side; the seated agent is the ACP **Agent**.

```rust
pub struct AcpQuark {
    id: QuarkId,
    flavor: Flavor,
    target: AcpTarget,              // program + args to boot, from team.json
    conn: Option<AcpConnection>,    // the RESIDENT subprocess + JSON-RPC connection
    session: Option<SessionId>,     // from session/new; the whole point
    last_usage: Option<(u32, u32)>, // (used, size) — for the EnergyReport delta
}
```

Lifecycle, per turn:
1. **Boot once, lazily.** No connection ⇒ spawn `target.program` with piped stdio, `initialize`
   (exchange `protocolVersion` + capabilities — record whether the agent advertises `loadSession` /
   `sessionCapabilities.resume`), then `session/new { cwd: turn.cwd, mcpServers: [] }`. **`turn.cwd`
   is already the quark's worktree** (`projection.rs:26-32`) — the worktree plan's chain lands here
   for free. Keep the child alive across turns; that *is* failure #3's fix.
2. **Prompt.** `session/prompt { sessionId, prompt: [ContentBlock::Text(...)] }`. On a resident
   session, the prompt is **the new task only** — not `prompt::build`'s full re-render. Concretely:
   `prompt::build` splits into `build_preamble` (identity / invariants / nucleus / authority / where
   you are / how to respond — sent **once**, on the first turn of a session) and `build_turn`
   (the task + any *new* field lines since our last prompt on this session). The `field_window` and
   `git_diff` sections shrink to a delta. This is the concrete death of the quadratic re-send — and
   it is a *prompt* change, so it must be tested exactly as `prompt.rs` already tests its sections.
3. **Stream.** Every `session/update`: `agent_message_chunk` → accumulate into the reply **and**
   `presence.send(Chunk)`; `tool_call` → `presence.send(Tool)`; `usage_update` → record
   `(used, size)`, `presence.send(Usage)`.
4. **Permission.** On `session/request_permission`, do **not** block on a human inline: fold the
   field exactly as `finish_turn` does today (`engine.rs:433-469`) — `resolve_mode` + `allow_rules`
   + `gatekeeper::decide`. `AutoApprove` ⇒ answer `allow_once` immediately (and `allow_always` when
   the mode is `Auto` and the human said `remember`). `AskHuman` ⇒ append the `PermissionReq`,
   answer… **and here is the open question (§6.1): ACP's permission call is *blocking*, and our
   human answer arrives asynchronously via the field.** Two options: (a) hold the JSON-RPC response
   open across the human's decision (the agent sits idle mid-turn — honest, but the turn never
   "ends", so `finish_turn` never runs and the quark shows as Excited for minutes/hours); (b) answer
   `reject_once` and let the agent end its turn, then re-prompt on the grant (matches today's
   pause/resume semantics exactly, at the cost of the agent losing its in-flight tool). **Recommend
   (a) with a timeout**, because it is the *reason* to want ACP — but this must be decided with the
   engine's `Status{Waiting}` semantics in front of you.
5. **End.** The `session/prompt` response carries a `StopReason`. Map: `end_turn` → normal;
   `max_tokens` / `max_turn_requests` → message + a Gluon note; `refusal` → message; `cancelled` →
   no message. Return `TurnOutcome { message, used_tokens: <delta of usage_update `used`>, permission: None }`.

**`CliRunner` is untouched** (`runner.rs:32-35`). ACP needs a *long-lived duplex* connection, not
`run(inv) -> CliResult`; forcing it through that trait would be a lie. The new seam is a sibling:

```rust
/// The single seam where an ACP agent subprocess is spawned. Faked in tests
/// (an in-process agent that emits canned session/updates); real in production.
#[async_trait]
pub trait AcpTransport: Send + Sync {
    async fn connect(&self, target: &AcpTarget) -> anyhow::Result<AcpConnection>;
}
```

This mirrors `CliRunner`'s discipline exactly — one spawn point, fakeable — which is what makes §4's
test possible without a network call or an API key.

### 3d. Order of execution

0. **Trait + presence plumbing** (§3b layers 1–5), every adapter ignoring the sink. **No behaviour
   change; the whole suite must stay green.** Independently shippable.
1. **`AcpTransport` + `FakeAcpAgent`** — a canned agent that emits chunks, a `usage_update`, a
   `request_permission`, and a `StopReason`. All of §4's tests run against this.
2. **`AcpQuark`** against the fake. Registry + `Seat` wiring.
3. **Live: `acp-claude`.** Seat a third quark on `@agentclientprotocol/claude-agent-acp` **next to**
   the existing `claude` CLI seat in `.hadron/team.json` and run both. Compare.
4. **Prompt split** (`build_preamble` / `build_turn`) — the resident-session payoff. Do this *after*
   a live ACP turn works, so a broken prompt can't be confused with a broken transport.
5. **Chamber presence view** — out of scope for this plan's crates; the gluon writes
   `.hadron/presence.json` and the chamber team owns the rail.
6. **Local models via Goose** (`acp-goose` seat, Goose pointed at Ollama). This is the plan's actual
   strategic payload (§5).

**`hadron-gluon` and `hadron-chamber` are being edited by other agents right now.** Nothing here
lands until those settle; step 0 is the merge-conflict-heavy one (it touches every `Quark` impl).

---

## 4. The discriminating test

> **`streaming_presence_arrives_before_the_turn_ends`** — a turn on an `AcpQuark` (over
> `FakeAcpAgent`, which emits three `agent_message_chunk`s and a `usage_update` before its
> `StopReason`) delivers **≥2 `Presence::Chunk`s on the sink, and a `Presence::Usage` with a
> non-zero `used`, strictly before `excite` returns** — and the same assertion run against
> `AgyQuark`/`ClaudeQuark` yields **zero** presence events, because the CLI transport has no channel
> on which a mid-turn token could exist.

Why this one:
- **Structurally impossible on the old transport.** Not "slower", not "less pretty" — a one-shot
  subprocess whose stdout we read with `wait_with_output()` (`runner.rs:74`) *cannot* produce a
  mid-turn observation. The test can only pass on the new path. That is what "discriminating" means.
- **It also proves the trait change was necessary**, which is the one thing a reviewer will push
  back on (§3b is real churn across ~8 impls).
- **It is verifiable on a provider we can actually run**: the Claude ACP adapter streams (the Agent
  SDK's `includePartialMessages` yields partial events;
  [streaming](https://platform.claude.com/docs/en/agent-sdk/streaming-output)), and ACP defines
  `agent_message_chunk` as a normative mid-turn notification.
- **It carries usage with it** — the non-zero `used` assertion is failure #2's fix, in the same test,
  from a *structured* source rather than a regex over Markdown.

**Why not the obvious candidates.** The tempting discriminators are the agy ones — "a 400 KB field
no longer kills the turn", "agy finally reports a real token count" — and they are **untestable
against any agent that exists**: agy has no ACP (§2), so there is nothing to point the test at. A
test that can only pass against a hypothetical `agy --acp` proves nothing today. Say so out loud
rather than writing a test that is green against a fake and meaningless against reality.

**The live companion** (`#[ignore]`d, per `runner.rs:40`'s convention): seat `acp-claude`, send one
message, assert (i) chunks arrived before the reply, (ii) `usage.size` equals the model's real
context window, (iii) **turn 2's outbound `session/prompt` payload is smaller than turn 1's despite a
longer conversation** — the resident-session proof, failure #3, which today's transport inverts.

---

## 5. Recommendation

**Do it — but scope it as "ACP is how hadron reaches *new* providers", not "ACP replaces the CLI
adapters".** Specifically:

1. **Ship §3b (the presence sink) regardless.** It is small, it is additive, and failure #4
   (presence is blind) is a *hadron* bug, not a transport bug: our own trait cannot express a
   mid-turn event. Fixing it unblocks streaming for *any* future transport.
2. **Ship `AcpQuark` + `acp-claude` next.** Cost is bounded (one Rust dep, one new adapter, a fake
   agent for tests); payoff is a resident session, structured usage, real per-tool permission
   gating, and a stop button.
3. **Then `acp-goose` → Ollama / LM Studio.** This is the user's stated goal — *many providers,
   including local models* — and ACP is the only path on the table that reaches it. Which brings us
   to the argument that actually decides this:

> **The per-vendor-SDK alternative does not avoid the sidecar; it multiplies it.** Anthropic's Agent
> SDK is **TypeScript/Python**
> ([SDK docs](https://docs.claude.com/en/docs/agent-sdk/permissions)). Hadron is **Rust**. So "just
> use the Anthropic SDK for real `canUseTool`" means *a Node sidecar we spawn and speak some
> protocol to* — and then Google's SDK means a **second** bespoke sidecar and a second bespoke
> protocol, and a local model means a **third**. ACP is the same sidecar shape with the protocol
> **already specified, versioned (`protocolVersion: 1`), and implemented by ~50 agents**, with an
> **official Rust client crate** so hadron's side is `cargo add`, not a hand-rolled JSON-RPC layer.
> Given "many providers including local models", the per-vendor SDK path is strictly worse: it is
> N sidecars instead of one client.

### The case for NOT doing it (take it seriously)

- **It fixes neither of the two failures that actually bit us this week.** E2BIG and `used_tokens: 0`
  are agy's, and **agy cannot speak ACP** (§2). If the pain you are actually feeling is agy pain, ACP
  is the wrong medicine, and the honest fix is: keep the byte cap, and put weight behind
  [antigravity-cli#31](https://github.com/google-antigravity/antigravity-cli/issues/31).
- **The claude path swaps a first-party CLI for a community Node adapter.** `claude -p` is
  Anthropic's own binary; `@agentclientprotocol/claude-agent-acp` is a third-party wrapper around
  Anthropic's SDK. That is a new dependency, a new failure mode (Node), and a new upgrade treadmill
  (121 releases). We would be *adding* a moving part to the one seat that currently works.
- **Real churn for the streaming fix.** §3b touches every `Quark` impl including six in
  `engine.rs`'s test module — in a crate two other agents are editing today.
- **The narrower move exists:** do **only** §3b (the presence sink) + keep both CLI adapters. That
  buys streaming for claude *without* ACP (`claude -p --output-format stream-json` — **UNVERIFIED**
  that our invocation can consume it incrementally, but the flag family exists), fixes the one
  failure that is genuinely ours, and costs a fraction of this plan. **If you only do one thing, do
  that one.**

**On balance:** the four failures are the *symptom*; the disease is that hadron's transport is
"render a string, shell out, read stdout", which cannot express sessions, streams, permissions, or
usage — and cannot reach a local model at all. ACP is the cheapest available cure that is not
vendor-shaped. Do it Claude-first and local-second; leave agy on the CLI with its byte cap and
revisit when Google ships `--acp`.

---

## 6. Open decisions for execution

1. **Blocking `session/request_permission` vs. our async field-driven grant** (§3c step 4). The
   single hardest semantic mismatch in this plan. Recommend: hold the response open, with a timeout,
   and mark the quark `Waiting` while held — but verify against `run_until_quiesce`'s quiesce
   condition (`engine.rs:595`), which currently defines "done" as *no turn in flight*.
2. **`EnergyReport` from cumulative `used`.** Delta-per-turn, or change `Kind::EnergyReport` to also
   carry `context_used` / `context_size` (additive, `#[serde(default)]`, per this codebase's
   forward-compat discipline — cf. `QuarkCard::provider`)? The chamber's usage UI wants
   used/window; the ledger wants spend. They are different numbers.
3. **One agent process per quark, or one per provider with several sessions?** ACP explicitly
   supports *"several concurrent sessions"* per connection
   ([architecture](https://agentclientprotocol.com/get-started/architecture)). One-per-quark is
   simpler and matches the current `SharedQuark` mutex; revisit if boot cost bites.
4. **Does `prompt::build` still send the field window on a resident session?** (§3c step 2.) Sending
   nothing means the quark loses its view of *sibling* quarks' messages (which ACP has no way to
   know about — the field is ours). Probably: preamble once, then per turn the task + only the field
   lines appended since our last prompt on this session. Needs a live read.
5. **Does the Claude ACP adapter advertise `loadSession` / `sessionCapabilities.resume`?** If not,
   a daemon restart loses every session and we are back to re-sending the transcript to rebuild
   context. **UNVERIFIED** — check the `initialize` response before building step 4 on it.

---

## Appendix — claims I could NOT verify (do not build on these without checking)

- **UNVERIFIED:** whether the third-party `antigravity-acp` bridge passes the prompt to `agy` via
  **stdin or argv**. If argv, the E2BIG bug *reappears inside the adapter* and the whole exercise is
  circular. The README does not say.
- **UNVERIFIED:** whether using a **Vertex AI / AI Studio API key** with a third-party agy bridge
  actually clears Google's ToS. The bridge's README *claims* it mitigates; that is the bridge
  author's opinion, not Google's.
- **DRAFT, not stable:** ACP **per-turn** token usage (`usage` on `PromptResponse` with
  `inputTokens`/`outputTokens`/`thoughtTokens`/`cachedRead|WriteTokens`). The RFD says it is
  *"intentionally kept in Draft while token accounting semantics are still being refined"*.
  Stable today is only session-level `usage_update { used, size, cost? }`.
- **UNVERIFIED:** whether `@agentclientprotocol/claude-agent-acp` supports `loadSession` /
  `session/resume` (the protocol defines them; the adapter's published feature list does not mention
  them).
- **UNVERIFIED:** whether `claude -p --output-format stream-json` can be consumed incrementally
  through our `ProcessRunner` (the §5 "narrower move"). `ProcessRunner` uses `wait_with_output()`
  (`runner.rs:74`), so it would need an incremental reader regardless.
- **UNVERIFIED:** ACP's story for a *remote* agent (HTTP/WS) — the docs mention it, but every agent
  we care about is a local stdio subprocess, so this plan assumes stdio throughout.
