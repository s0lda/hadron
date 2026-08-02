//! A quark backed by an HTTP server — Ollama, LM Studio, or any cloud
//! OpenAI-compatible endpoint (OpenRouter, Groq, DeepSeek, Together, …) — over
//! [`hadron_lattice::Transport::Http`]. No subprocess, no protocol handshake: just
//! a GET to list models and a POST to run one turn.
//!
//! Wire contracts read from the vendors' own APIs (Zed's checked-in `ollama`/
//! `lmstudio` crates were the reference for the shapes, confirmed live against a
//! running Ollama on this box and LM Studio's own server log):
//! - **Ollama**: keyless. `GET {base}/api/tags` lists models; `POST {base}/api/chat`
//!   streams newline-delimited JSON chat chunks.
//! - **LM Studio** and **cloud OpenAI-compatible**: the same OpenAI-shaped surface,
//!   `GET {base}/models` and `POST {base}/chat/completions`, streaming
//!   Server-Sent-Events `data: {...}` lines terminated by `data: [DONE]`. LM
//!   Studio is keyless (localhost); a cloud endpoint needs [`HttpTarget::api_key`],
//!   sent as `Authorization: Bearer <key>` — the same wire shape OpenRouter and
//!   every other OpenAI-compatible provider documents.
//!
//! Deserialization is deliberately lenient — every struct here names only the
//! field this adapter actually reads, so a vendor's response growing an unrelated
//! field never breaks Connect or a turn.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::StreamExt;
use hadron_lattice::live::{self, Activity, Doing};
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, Seat, SeatCommands, Transport, TurnOutcome};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::quark::Quark;

/// Which HTTP provider a [`Transport::Http`] seat speaks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpVendor {
    Ollama,
    LmStudio,
    /// Any cloud server speaking the OpenAI-compatible surface (OpenRouter, Groq,
    /// DeepSeek, Together, …) — same wire shape as [`HttpVendor::LmStudio`], plus
    /// an `Authorization: Bearer` header (see [`HttpTarget::api_key`]).
    OpenAiCompatible,
}

impl HttpVendor {
    /// Parse a seat's `vendor` string. `None` for anything else — a
    /// [`Transport::Http`] seat on an unknown vendor has nothing to boot.
    pub fn parse(vendor: &str) -> Option<HttpVendor> {
        match vendor {
            "ollama" => Some(HttpVendor::Ollama),
            "lmstudio" => Some(HttpVendor::LmStudio),
            "openai-compatible" => Some(HttpVendor::OpenAiCompatible),
            _ => None,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            HttpVendor::Ollama => "ollama",
            HttpVendor::LmStudio => "lmstudio",
            HttpVendor::OpenAiCompatible => "openai-compatible",
        }
    }

    /// The default base URL — for the two local vendors, what "just works"
    /// zero-setup against the out-of-the-box default; for the cloud vendor, a
    /// one-paste-friendly prefill (OpenRouter's endpoint) rather than a claim of
    /// working with no key.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            HttpVendor::Ollama => "http://localhost:11434",
            HttpVendor::LmStudio => "http://localhost:1234/v1",
            HttpVendor::OpenAiCompatible => "https://openrouter.ai/api/v1",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            HttpVendor::Ollama => "Ollama (local)",
            HttpVendor::LmStudio => "LM Studio (local)",
            HttpVendor::OpenAiCompatible => "Cloud (OpenAI-compatible)",
        }
    }

    /// Whether this vendor's server needs an `Authorization: Bearer` header to
    /// answer at all — the two local vendors are keyless; the cloud vendor is not.
    pub fn requires_api_key(&self) -> bool {
        matches!(self, HttpVendor::OpenAiCompatible)
    }
}

/// How to reach a [`Transport::Http`] seat's server: which vendor's wire shape to
/// speak, the base URL (the seat's own, or the vendor's default), and — for a
/// keyed vendor — the bearer token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpTarget {
    pub vendor: HttpVendor,
    pub base_url: String,
    /// Sent as `Authorization: Bearer <key>` on every request when present. Never
    /// read from `Seat` directly — [`HttpTarget::for_seat`] always leaves this
    /// `None`; the registry resolves it from the seat's `secret_env` via the
    /// credential store and attaches it after construction, the same way
    /// `AcpTarget::for_seat_with_env` attaches a resolved env. Ollama and LM
    /// Studio seats declare no `secret_env`, so this stays `None` for them.
    pub api_key: Option<String>,
}

impl HttpTarget {
    /// The target for a seat, or `None` for a non-`Http` seat or an unknown vendor.
    /// `api_key` is always `None` here — see the field's doc comment.
    pub fn for_seat(seat: &Seat) -> Option<HttpTarget> {
        if seat.transport != Transport::Http {
            return None;
        }
        let vendor = HttpVendor::parse(&seat.vendor)?;
        let base_url =
            seat.http_base_url.clone().unwrap_or_else(|| vendor.default_base_url().to_string());
        Some(HttpTarget { vendor, base_url, api_key: None })
    }

    /// Attach a bearer token from a resolved `secret_env` — whichever var comes
    /// first, since a cloud vendor needs exactly one. Ollama/LM Studio seats
    /// declare no `secret_env`, so this is a no-op for them. The one place this
    /// rule lives; `registry::attach_http_api_key` (the live turn path) and
    /// [`HttpTarget::for_seat_with_env`] (the Settings probe path) both call it
    /// rather than each re-deciding "first var wins".
    pub fn with_resolved_env(mut self, env: &[(String, String)]) -> HttpTarget {
        if let Some((_, value)) = env.first() {
            self.api_key = Some(value.clone());
        }
        self
    }

    /// Build a target for `seat` with its resolved secret environment attached —
    /// mirrors `AcpTarget::for_seat_with_env`, so the Settings Model probe reaches
    /// a keyed cloud seat the same way a live turn does.
    pub fn for_seat_with_env(seat: &Seat, store: &dyn hadron_lattice::secrets::SecretStore) -> Option<HttpTarget> {
        let target = Self::for_seat(seat)?;
        Some(target.with_resolved_env(&seat.resolve_env(store)))
    }

    /// Every request in this module goes through here, so this is the one place a
    /// base URL is normalised — a literal-built `HttpTarget` (the chamber's Connect
    /// probe builds one) cannot bypass it.
    ///
    /// LM Studio's OpenAI surface lives at `{host}/v1` and its API answers any other
    /// path with **HTTP 200** and an `{"error": …}` body, so a base typed without
    /// the suffix reads as a successful connection listing zero models rather than
    /// as a wrong URL. Its `/v1` is a fixed part of the vendor's API, not a
    /// deployment choice — unlike the cloud vendor, whose prefix differs per
    /// provider (`/api/v1`, `/openai/v1`, …) and is therefore left exactly as typed.
    fn url(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        if self.vendor == HttpVendor::LmStudio && !base.ends_with("/v1") {
            return format!("{base}/v1{path}");
        }
        format!("{base}{path}")
    }
}

/// One model this server can run, as offered by its list-models endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModel {
    pub id: String,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModelEntry>,
}

#[derive(Deserialize)]
struct OllamaModelEntry {
    name: String,
}

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModelEntry>,
    /// An OpenAI-shaped server reports a bad path or a rejected key in the BODY
    /// while still answering 200 (measured against LM Studio: `GET {host}/models`
    /// → `200 {"error":"Unexpected endpoint or method. (GET /models)"}`). Reading
    /// only `data` turned that into "connected, 0 models" — a green light on a
    /// request that failed. Both shapes seen in the wild: a bare string, and
    /// OpenAI's own `{"error": {"message": …}}`.
    #[serde(default)]
    error: Option<serde_json::Value>,
}

impl OpenAiModelsResponse {
    fn error_message(&self) -> Option<String> {
        let e = self.error.as_ref()?;
        Some(match e.get("message").and_then(|m| m.as_str()).or_else(|| e.as_str()) {
            Some(msg) => msg.to_string(),
            None => e.to_string(),
        })
    }
}

#[derive(Deserialize)]
struct OpenAiModelEntry {
    id: String,
}

/// List the models this server currently has available. Blocking, deliberately:
/// mirrors `adapter::acp::probe_selectors` (also a plain blocking fn), so the
/// chamber's Connect button can run it via `cx.background_spawn` with no tokio
/// reactor of its own — a bare background thread, same as the ACP probe.
pub fn fetch_models(target: &HttpTarget) -> anyhow::Result<Vec<LocalModel>> {
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(10)).build()?;
    match target.vendor {
        HttpVendor::Ollama => {
            let resp = client.get(target.url("/api/tags")).send()?;
            anyhow::ensure!(resp.status().is_success(), "Ollama returned {}", resp.status());
            let body: OllamaTagsResponse = resp.json()?;
            Ok(body.models.into_iter().map(|m| LocalModel { id: m.name }).collect())
        }
        HttpVendor::LmStudio | HttpVendor::OpenAiCompatible => {
            let mut req = client.get(target.url("/models"));
            if let Some(key) = &target.api_key {
                req = req.bearer_auth(key);
            }
            let resp = req.send()?;
            anyhow::ensure!(resp.status().is_success(), "{} returned {}", target.vendor.display_name(), resp.status());
            let body: OpenAiModelsResponse = resp.json()?;
            if let Some(msg) = body.error_message() {
                anyhow::bail!("{} returned: {msg}", target.vendor.display_name());
            }
            Ok(body.data.into_iter().map(|m| LocalModel { id: m.id }).collect())
        }
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Value],
    stream: bool,
    /// Present only on a tool-capable turn — see [`LocalQuark::excite`]. Both
    /// vendors accept `tools` alongside `stream: true` and stream `tool_calls`
    /// deltas the same way they stream content: confirmed live against the real
    /// OpenRouter and Ollama endpoints, not assumed from documentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a Value>,
}

#[derive(Deserialize, Default)]
struct OllamaChatMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    /// Ollama's native `/api/chat` emits a call's whole `tool_calls` array in
    /// ONE chunk — never split token-by-token the way OpenAI's `arguments`
    /// string can be — so no cross-chunk accumulation is needed here.
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Deserialize)]
struct OllamaToolCall {
    #[serde(default)]
    id: Option<String>,
    function: OllamaToolCallFunction,
}

#[derive(Deserialize)]
struct OllamaToolCallFunction {
    name: String,
    /// Ollama sends this as a JSON **object**, unlike OpenAI's string — see
    /// [`ToolCall::arguments_json`] which normalises both to the same accessor.
    #[serde(default)]
    arguments: Value,
}

#[derive(Deserialize, Default)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    /// One entry per tool call touched by this delta, keyed by
    /// [`OpenAiToolCallDelta::index`] — `id`/`function.name` arrive once on the
    /// first delta for that index, `function.arguments` arrives as a string
    /// FRAGMENT on every delta after and must be concatenated in order.
    #[serde(default)]
    tool_calls: Vec<OpenAiToolCallDelta>,
}

#[derive(Deserialize)]
struct OpenAiToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiToolCallFunctionDelta>,
}

#[derive(Deserialize, Default)]
struct OpenAiToolCallFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAiChoice {
    #[serde(default)]
    delta: OpenAiDelta,
}

#[derive(Debug, Deserialize, Default)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct OpenAiChatChunk {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OllamaChatChunk {
    #[serde(default)]
    message: Option<OllamaChatMessage>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

/// One tool call a model asked for, normalised across both vendors' wire
/// shapes (Ollama's whole-object `arguments`, OpenAI's incrementally-streamed
/// `arguments` string) into the one shape [`crate::adapter::local_tools::execute`]
/// consumes.
#[derive(Debug, Clone, Default)]
pub(crate) struct ToolCall {
    pub id: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ToolCallFunction {
    pub name: String,
    /// Always a JSON-encoded string here, whichever vendor it came from — see
    /// [`ToolCall::arguments_json`].
    pub arguments: String,
}

impl ToolCall {
    /// Never fails: a model that emits malformed arguments gets an empty
    /// object and then an `ERROR:` result telling it what it did — an
    /// `unwrap` here would kill the turn instead.
    pub fn arguments_json(&self) -> Value {
        serde_json::from_str(&self.function.arguments).unwrap_or_else(|_| json!({}))
    }

    /// How this call's arguments must be written when the assistant turn is
    /// echoed back on the next round.
    ///
    /// The two wire formats are not interchangeable and the mismatch is fatal,
    /// not cosmetic: Ollama's native `/api/chat` answers a string-form
    /// `arguments` with `400 Bad Request` and rejects the WHOLE request, so
    /// round 1 succeeds and round 2 kills the turn.
    pub fn echoed_arguments(&self, vendor: HttpVendor) -> Value {
        match vendor {
            HttpVendor::Ollama => self.arguments_json(),
            HttpVendor::LmStudio | HttpVendor::OpenAiCompatible => Value::String(self.function.arguments.clone()),
        }
    }
}

impl From<OllamaToolCall> for ToolCall {
    fn from(t: OllamaToolCall) -> Self {
        ToolCall {
            id: t.id.unwrap_or_default(),
            function: ToolCallFunction { name: t.function.name, arguments: t.function.arguments.to_string() },
        }
    }
}

/// Accumulates OpenAI-style streamed tool-call deltas, keyed by `index` so
/// fragments for the same call (arriving across many SSE lines) land on the
/// same entry regardless of what else interleaves.
#[derive(Default)]
struct ToolCallAccumulator {
    by_index: std::collections::BTreeMap<usize, ToolCall>,
}

impl ToolCallAccumulator {
    fn add(&mut self, delta: OpenAiToolCallDelta) {
        let entry = self.by_index.entry(delta.index).or_default();
        if let Some(id) = delta.id {
            entry.id = id;
        }
        if let Some(f) = delta.function {
            if let Some(name) = f.name {
                entry.function.name = name;
            }
            if let Some(args) = f.arguments {
                entry.function.arguments.push_str(&args);
            }
        }
    }

    fn finish(self) -> Vec<ToolCall> {
        self.by_index.into_values().collect()
    }
}

/// Read `resp`'s body as a byte stream and hand each newline-terminated line to
/// `on_line`, buffering across chunk boundaries — an HTTP chunk has no reason to
/// end on a line boundary. Shared by both vendors: Ollama's raw NDJSON and LM
/// Studio's SSE (`data: {...}`) are both, at this layer, just lines.
async fn for_each_line(
    resp: reqwest::Response,
    mut on_line: impl FnMut(&str),
) -> anyhow::Result<()> {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk?);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim();
            if !line.is_empty() {
                on_line(line);
            }
        }
    }
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if !line.is_empty() {
            on_line(line);
        }
    }
    Ok(())
}

/// A field the wire may send as `null`, as an EMPTY STRING, or with real text.
/// The three must not mean three different things: OpenRouter's reasoning deltas
/// carry `"content": ""` beside the thought, and a chain that treats `Some("")`
/// as present short-circuits there and drops the thought — see
/// `a_reasoning_delta_carrying_an_empty_content_still_streams`.
fn present(field: &Option<String>) -> Option<&str> {
    field.as_deref().filter(|s| !s.is_empty())
}

/// Fail with the server's OWN explanation, not just its status line. A bare
/// `403 Forbidden` names neither the model nor the key that was refused, and the
/// body always does.
async fn ensure_chat_ok(resp: reqwest::Response, vendor: HttpVendor) -> anyhow::Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let detail = body.trim();
    anyhow::bail!(
        "{} chat request failed: {status}{}",
        vendor.display_name(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(" — {}", detail.chars().take(400).collect::<String>())
        }
    )
}

/// Run one chat turn against `target`, handing every text delta to `on_delta` as
/// it arrives — tagged [`Doing::Thinking`] for the model's reasoning channel and
/// [`Doing::Speaking`] for its answer — and returning the accumulated **reply**
/// once the stream ends, along with reported usage.
///
/// Only the answer becomes the reply. A chain of thought is published so the Live
/// card moves while the model thinks, exactly as `adapter::acp::session` already
/// does with `Doing::Thinking`, but it is not something the model said to the
/// swarm. The one exception is a model that never leaves its reasoning channel —
/// then the thought IS the reply, because the alternative is an empty message.
async fn stream_chat(
    target: &HttpTarget,
    model: &str,
    messages: &[Value],
    tools: Option<&Value>,
    mut on_delta: impl FnMut(Doing, &str),
) -> anyhow::Result<(String, Vec<ToolCall>, hadron_lattice::Usage)> {
    let client = reqwest::Client::new();
    let body = ChatRequest { model, messages, stream: true, tools };
    let mut full = String::new();
    let mut thought = String::new();
    let mut usage = hadron_lattice::Usage::default();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    match target.vendor {
        HttpVendor::Ollama => {
            let resp = client.post(target.url("/api/chat")).json(&body).send().await?;
            let resp = ensure_chat_ok(resp, target.vendor).await?;
            let mut prompt_tokens = 0u32;
            let mut eval_tokens = 0u32;
            for_each_line(resp, |line| {
                if let Ok(chunk) = serde_json::from_str::<OllamaChatChunk>(line) {
                    if let Some(p) = chunk.prompt_eval_count {
                        prompt_tokens = p;
                    }
                    if let Some(e) = chunk.eval_count {
                        eval_tokens = e;
                    }
                    if let Some(msg) = chunk.message {
                        if !msg.content.is_empty() {
                            full.push_str(&msg.content);
                            on_delta(Doing::Speaking, &msg.content);
                        } else if let Some(t) = present(&msg.reasoning).or_else(|| present(&msg.thinking)) {
                            thought.push_str(t);
                            on_delta(Doing::Thinking, t);
                        }
                        tool_calls.extend(msg.tool_calls.into_iter().map(ToolCall::from));
                    }
                }
            })
            .await?;
            if prompt_tokens > 0 || eval_tokens > 0 {
                usage.spend = hadron_lattice::TokenSpend { input: Some(prompt_tokens), output: Some(eval_tokens), ..Default::default() };
            }
        }
        HttpVendor::LmStudio | HttpVendor::OpenAiCompatible => {
            let mut req = client.post(target.url("/chat/completions")).json(&body);
            if let Some(key) = &target.api_key {
                req = req.bearer_auth(key);
            }
            let resp = req.send().await?;
            let resp = ensure_chat_ok(resp, target.vendor).await?;
            let mut acc = ToolCallAccumulator::default();
            for_each_line(resp, |line| {
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else { return };
                let data = data.trim();
                if data == "[DONE]" {
                    return;
                }
                if let Ok(chunk) = serde_json::from_str::<OpenAiChatChunk>(data) {
                    if let Some(u) = chunk.usage {
                        usage.spend = hadron_lattice::TokenSpend { input: Some(u.prompt_tokens), output: Some(u.completion_tokens), ..Default::default() };
                    }
                    if let Some(c) = chunk.choices.into_iter().next() {
                        if let Some(content) = present(&c.delta.content) {
                            full.push_str(content);
                            on_delta(Doing::Speaking, content);
                        } else if let Some(t) = present(&c.delta.reasoning_content)
                            .or_else(|| present(&c.delta.reasoning))
                        {
                            thought.push_str(t);
                            on_delta(Doing::Thinking, t);
                        }
                        for delta in c.delta.tool_calls {
                            acc.add(delta);
                        }
                    }
                }
            })
            .await?;
            tool_calls = acc.finish();
        }
    }
    // A model that never left its reasoning channel said exactly one thing; an
    // empty reply would report that turn to the swarm as silence.
    if full.is_empty() && tool_calls.is_empty() {
        full = thought;
    }
    Ok((full, tool_calls, usage))
}

/// The minimum gap between two published draft updates — mirrors
/// `adapter::acp::session::LiveFeed::THROTTLE` (private to that module), so a
/// local model streaming a chunk every few tokens doesn't rewrite the live file
/// hundreds of times a turn. `pub(crate)`: `adapter::cli::CliQuark`'s own
/// streaming path reuses this exact constant rather than duplicating it (SSOT).
pub(crate) const PUBLISH_THROTTLE: Duration = Duration::from_millis(200);

/// A quark backed by an HTTP server (Ollama, LM Studio, or a cloud OpenAI-compatible
/// endpoint). Single-shot, like
/// [`crate::adapter::cli::CliQuark`]: the whole prompt goes in as one user
/// message and the reply comes back as one string — a bare chat completion has
/// no tool loop, no file edits, no multi-turn resume, so there is no session to
/// keep resident between turns.
pub struct LocalQuark {
    id: QuarkId,
    flavor: Flavor,
    display_name: Option<String>,
    model: String,
    target: HttpTarget,
    roles: Vec<String>,
    exclusive: bool,
    commands: SeatCommands,
    energy_limit: Option<u32>,
    deny_skills: Vec<String>,
    /// Where to publish mid-turn draft activity. `None` = nobody is watching
    /// (tests, and any quark this daemon is not watching) — mirrors
    /// `adapter::acp::AcpQuark`'s same-shaped field.
    live_dir: Option<PathBuf>,
}

impl LocalQuark {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, target: HttpTarget) -> Self {
        LocalQuark {
            id,
            flavor,
            display_name: None,
            model: model.into(),
            target,
            roles: Vec::new(),
            exclusive: false,
            commands: SeatCommands::default(),
            energy_limit: None,
            deny_skills: Vec::new(),
            live_dir: None,
        }
    }

    pub fn watching(mut self, dir: PathBuf) -> Self {
        self.live_dir = Some(dir);
        self
    }

    pub fn with_display_name(mut self, name: Option<String>) -> Self {
        self.display_name = name;
        self
    }

    pub fn with_roles(mut self, roles: Vec<String>, exclusive: bool) -> Self {
        self.roles = roles;
        self.exclusive = exclusive;
        self
    }

    pub fn with_commands(mut self, commands: SeatCommands) -> Self {
        self.commands = commands;
        self
    }

    pub fn with_energy_limit(mut self, limit: Option<u32>) -> Self {
        self.energy_limit = limit;
        self
    }

    pub fn with_deny_skills(mut self, deny_skills: Vec<String>) -> Self {
        self.deny_skills = deny_skills;
        self
    }
}

#[async_trait]
impl Quark for LocalQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        self.flavor.clone()
    }
    fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }
    fn roles(&self) -> Vec<String> {
        self.roles.clone()
    }
    fn exclusive(&self) -> bool {
        self.exclusive
    }
    fn deny_skills(&self) -> Vec<String> {
        self.deny_skills.clone()
    }
    fn commands(&self) -> &SeatCommands {
        &self.commands
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    fn energy_limit(&self) -> Option<u32> {
        self.energy_limit
    }

    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let mut prompt = crate::adapter::prompt::build(&turn, &self.id);
        let root = hadron_forge::file::Root::new(turn.cwd.clone());
        let quark_id = self.id.clone();
        let dir = self.live_dir.clone();
        let mode = turn.mode;
        let tools = crate::adapter::local_tools::declarations_for_mode(mode);
        if mode == hadron_lattice::Mode::Auto {
            prompt.push_str(crate::adapter::local_tools::AUTO_MODE_EXEC_NOTE);
        }
        let mut messages = vec![json!({ "role": "user", "content": prompt })];
        let mut spend = hadron_lattice::TokenSpend::default();

        for round in 0..MAX_TOOL_ROUNDS {
            let mut draft = String::new();
            let mut thought = String::new();
            let mut last_publish: Option<Instant> = None;
            let (text, calls, usage) =
                stream_chat(&self.target, &self.model, &messages, tools.as_ref(), |doing, delta| {
                    // Two buffers, because they are two different things: the draft is
                    // the reply the chat will show, the thought is only ever the Live
                    // card's "thinking" line. Mirrors `adapter::acp::session`'s split.
                    let buf = if doing == Doing::Speaking { &mut draft } else { &mut thought };
                    buf.push_str(delta);
                    let Some(dir) = &dir else { return };
                    let due = last_publish.is_none_or(|t| t.elapsed() >= PUBLISH_THROTTLE);
                    if due {
                        last_publish = Some(Instant::now());
                        let activity = if doing == Doing::Speaking {
                            Activity::speaking(quark_id.clone(), &draft)
                        } else {
                            Activity::new(quark_id.clone(), Doing::Thinking, &thought)
                        };
                        let _ = live::publish(dir, &activity);
                    }
                })
                .await?;
            accumulate_spend(&mut spend, &usage.spend);

            if calls.is_empty() {
                if let Some(dir) = &self.live_dir {
                    let _ = live::clear(dir, &self.id);
                }
                return Ok(TurnOutcome {
                    message: Some(text),
                    usage: hadron_lattice::Usage { spend, ..Default::default() },
                    ..Default::default()
                });
            }

            messages.push(json!({
                "role": "assistant",
                "content": text,
                "tool_calls": calls.iter().map(|c| json!({
                    "id": c.id, "type": "function",
                    "function": { "name": c.function.name, "arguments": c.echoed_arguments(self.target.vendor) }
                })).collect::<Vec<_>>(),
            }));
            for call in &calls {
                if let Some(dir) = &self.live_dir {
                    let _ = live::publish(
                        dir,
                        &Activity::new(
                            quark_id.clone(),
                            Doing::Working,
                            &format!("round {}: {}", round + 1, call.function.name),
                        ),
                    );
                }
                let result =
                    crate::adapter::local_tools::execute(&root, mode, &call.function.name, &call.arguments_json());
                messages.push(json!({
                    "role": "tool", "tool_call_id": call.id, "content": truncate_tool_result(&result),
                }));
            }
        }

        if let Some(dir) = &self.live_dir {
            let _ = live::clear(dir, &self.id);
        }
        Ok(TurnOutcome {
            message: Some(format!(
                "I ran {MAX_TOOL_ROUNDS} tool rounds without reaching an answer and stopped. \
                 The work so far is on disk; ask me to continue."
            )),
            usage: hadron_lattice::Usage { spend, ..Default::default() },
            ..Default::default()
        })
    }
}

/// How many tool rounds one turn may take before it stops and answers with
/// whatever it has. A model that loops forever must cost a bounded number of
/// requests, not an unbounded bill.
const MAX_TOOL_ROUNDS: usize = 24;

/// A whole `cargo test` log in a tool result blows the context window on a
/// 32k local model, and the round after it fails for a reason nobody can see.
///
/// Truncates by **character** count on both the length check and the
/// head/tail split, so `s.len() - LIMIT` (bytes) is never used to describe a
/// chars-based cut — multi-byte content would make that arithmetic lie.
fn truncate_tool_result(s: &str) -> String {
    const LIMIT: usize = 12_000;
    let char_count = s.chars().count();
    if char_count <= LIMIT {
        return s.to_string();
    }
    let head: String = s.chars().take(LIMIT / 2).collect();
    let tail: String = s.chars().rev().take(LIMIT / 2).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}\n… [{} chars elided] …\n{tail}", char_count - LIMIT)
}

/// Add one round's usage into a running total. Never treats an ABSENT round as
/// zero-cost — a round with no reported tokens simply leaves `spend`
/// untouched, per the "absent is not zero" invariant; only a round that DID
/// report a number gets added in.
fn accumulate_spend(spend: &mut hadron_lattice::TokenSpend, round: &hadron_lattice::TokenSpend) {
    if let Some(i) = round.input {
        spend.input = Some(spend.input.unwrap_or(0).saturating_add(i));
    }
    if let Some(o) = round.output {
        spend.output = Some(spend.output.unwrap_or(0).saturating_add(o));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Bind a local server that replies with `status`/`body` to every request it
    /// accepts, once, on a background thread. Returns the base URL (`http://127.0.0.1:<port>`).
    fn serve_once(status: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    /// As [`serve_once`], but also hands back the raw request text it received —
    /// for asserting on a header (`Authorization: Bearer …`, or its absence) or
    /// on the `tools` the request carried. A single fixed-size read truncates a
    /// tools-carrying request (the one behind `build_seat_watched_wires_an_http_seat_to_the_live_dir`'s
    /// prior regression) — drain until the client stops sending instead, same
    /// fix as `serve_twice_capturing`.
    fn serve_once_capturing(status: &'static str, body: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_millis(200))).ok();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                while let Ok(n) = stream.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}"), rx)
    }

    /// OpenAI-style streamed `tool_calls` deltas arrive char-by-char, keyed by
    /// `index` — confirmed live against the real OpenRouter endpoint (see the
    /// task's verification notes). A model that asks for a tool and gets
    /// nothing back is a quark that silently cannot edit files — the exact bug
    /// this task exists to fix.
    #[tokio::test]
    async fn a_streamed_openai_reply_reassembles_split_tool_call_arguments() {
        // Built with `json!` rather than hand-escaped literals — the SSE
        // payload nests a JSON-encoded string (`arguments`) inside JSON, and a
        // hand-typed literal here previously mismatched its own braces.
        let chunks = [
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "c1", "type": "function", "function": {"name": "read_file", "arguments": ""}}
            ]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "{\"path\":"}}
            ]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "\"Cargo.toml\"}"}}
            ]}}]}),
        ];
        let mut sse = String::new();
        for c in &chunks {
            sse.push_str("data: ");
            sse.push_str(&c.to_string());
            sse.push('\n');
        }
        sse.push_str("data: [DONE]\n");
        let sse: &'static str = Box::leak(sse.into_boxed_str());
        let (base, _rx) = serve_once_capturing("200 OK", sse);
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let (text, calls, _) =
            stream_chat(&target, "some-model", &[json!({"role": "user", "content": "hi"})], None, |_, _| {})
                .await
                .unwrap();
        assert!(text.is_empty(), "a tool-only reply must not be mistaken for prose: {text:?}");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(calls[0].arguments_json()["path"], "Cargo.toml");
    }

    /// A plain answer must NOT be mistaken for a tool round, or the loop spins.
    #[tokio::test]
    async fn a_streamed_reply_with_no_tool_calls_ends_the_loop() {
        let (base, _rx) =
            serve_once_capturing("200 OK", "data: {\"choices\":[{\"delta\":{\"content\":\"done\"}}]}\ndata: [DONE]\n");
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let (text, calls, _) =
            stream_chat(&target, "some-model", &[json!({"role": "user", "content": "hi"})], None, |_, _| {})
                .await
                .unwrap();
        assert!(calls.is_empty());
        assert_eq!(text, "done");
    }

    #[test]
    fn fetch_models_sends_bearer_auth_header_when_an_api_key_is_set() {
        let (base, rx) =
            serve_once_capturing("200 OK", r#"{"object":"list","data":[{"id":"openrouter/some-model"}]}"#);
        let target =
            HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: Some("sk-test-123".to_string()) };
        let models = fetch_models(&target).unwrap();
        assert_eq!(models, vec![LocalModel { id: "openrouter/some-model".to_string() }]);
        let request = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.to_lowercase().contains("authorization: bearer sk-test-123"), "request:\n{request}");
    }

    #[test]
    fn fetch_models_sends_no_authorization_header_when_no_api_key_is_set() {
        // The Ollama/LM Studio case — a keyless server must never see the header at all.
        let (base, rx) = serve_once_capturing("200 OK", r#"{"models":[]}"#);
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: base, api_key: None };
        fetch_models(&target).unwrap();
        let request = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!request.to_lowercase().contains("authorization"), "request:\n{request}");
    }

    #[tokio::test]
    async fn stream_chat_sends_bearer_auth_header_when_an_api_key_is_set() {
        let (base, rx) = serve_once_capturing("200 OK", "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\ndata: [DONE]\n");
        let target =
            HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: Some("sk-test-456".to_string()) };
        stream_chat(&target, "some-model", &[json!({"role": "user", "content": "hi"})], None, |_, _| {}).await.unwrap();
        let request = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.to_lowercase().contains("authorization: bearer sk-test-456"), "request:\n{request}");
    }

    #[test]
    fn open_ai_compatible_vendor_parses_and_defaults_to_openrouter() {
        assert_eq!(HttpVendor::parse("openai-compatible"), Some(HttpVendor::OpenAiCompatible));
        assert_eq!(HttpVendor::OpenAiCompatible.default_base_url(), "https://openrouter.ai/api/v1");
        assert!(HttpVendor::OpenAiCompatible.requires_api_key());
        assert!(!HttpVendor::Ollama.requires_api_key());
        assert!(!HttpVendor::LmStudio.requires_api_key());
    }

    #[test]
    fn fetch_models_parses_a_real_ollama_tags_response() {
        // Captured live from `curl http://localhost:11434/api/tags` on this box.
        let base = serve_once(
            "200 OK",
            r#"{"models":[{"name":"nemotron-3-ultra:cloud","model":"gemma3:27b","modified_at":"2026-05-24T17:56:53Z","size":1,"digest":"x","details":{"format":"gguf","family":"gemma3","parameter_size":"27.4B","quantization_level":"Q4_K_M"}}]}"#,
        );
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: base, api_key: None };
        let models = fetch_models(&target).unwrap();
        assert_eq!(models, vec![LocalModel { id: "nemotron-3-ultra:cloud".to_string() }]);
    }

    #[test]
    fn fetch_models_parses_an_openai_shaped_models_response() {
        // The OpenAI-compatible shape LM Studio's `/v1/models` and `/v1/chat/completions`
        // advertise in its own server log.
        let base = serve_once(
            "200 OK",
            r#"{"object":"list","data":[{"id":"google/gemma-4-12b-qat","object":"model","owned_by":"organization"}]}"#,
        );
        let target = HttpTarget { vendor: HttpVendor::LmStudio, base_url: base, api_key: None };
        let models = fetch_models(&target).unwrap();
        assert_eq!(models, vec![LocalModel { id: "google/gemma-4-12b-qat".to_string() }]);
    }

    #[test]
    fn fetch_models_reports_a_connection_failure_plainly() {
        // Nothing is listening on this port — the Connect button's "server not running" path.
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: "http://127.0.0.1:1".to_string(), api_key: None };
        assert!(fetch_models(&target).is_err());
    }

    #[test]
    fn an_https_endpoint_is_reached_over_tls_not_refused_for_its_scheme() {
        // Guards reqwest's `rustls-tls` feature: with no TLS backend compiled in, every
        // `https://` URL fails before a socket is opened with "invalid URL, scheme is not
        // http" — which is what broke the cloud (OpenRouter) seats. Nothing listens on
        // port 1, so the request must still fail; it must fail as a CONNECTION failure.
        let target = HttpTarget {
            vendor: HttpVendor::OpenAiCompatible,
            base_url: "https://127.0.0.1:1".to_string(),
            api_key: Some("sk-test".to_string()),
        };
        let err = format!("{:#}", fetch_models(&target).unwrap_err());
        assert!(!err.contains("scheme is not http"), "no TLS backend compiled in: {err}");
    }

    #[test]
    fn an_lm_studio_base_missing_its_v1_suffix_is_anchored_to_the_openai_surface() {
        // Jake typed `http://10.5.0.2:1234` — the host alone. LM Studio's OpenAI
        // surface is under `/v1`, so without this the GET went to `/models`.
        let bare = HttpTarget {
            vendor: HttpVendor::LmStudio,
            base_url: "http://10.5.0.2:1234".to_string(),
            api_key: None,
        };
        assert_eq!(bare.url("/models"), "http://10.5.0.2:1234/v1/models");
        let already = HttpTarget { base_url: "http://10.5.0.2:1234/v1/".to_string(), ..bare.clone() };
        assert_eq!(already.url("/models"), "http://10.5.0.2:1234/v1/models");
        // A cloud endpoint's prefix differs per provider, so it is never rewritten.
        let cloud = HttpTarget { vendor: HttpVendor::OpenAiCompatible, ..bare };
        assert_eq!(cloud.url("/models"), "http://10.5.0.2:1234/models");
    }

    #[test]
    fn an_error_body_answered_with_200_is_a_failure_not_an_empty_model_list() {
        // Measured against the real LM Studio: a wrong path answers 200 with an
        // error body, which read as "connected, 0 models" — a green light on a
        // request that failed.
        let base = serve_once("200 OK", "{\"error\":\"Unexpected endpoint or method. (GET /models)\"}");
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let err = format!("{:#}", fetch_models(&target).unwrap_err());
        assert!(err.contains("Unexpected endpoint"), "the server's own reason must survive: {err}");
    }

    #[test]
    fn an_openai_shaped_error_object_is_reported_by_its_message() {
        let base = serve_once("200 OK", "{\"error\":{\"message\":\"Invalid API key\",\"code\":401}}");
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let err = format!("{:#}", fetch_models(&target).unwrap_err());
        assert!(err.contains("Invalid API key"), "expected the nested message: {err}");
    }

    #[tokio::test]
    async fn stream_chat_accumulates_ollama_ndjson_deltas() {
        let base = serve_once(
            "200 OK",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"Hel\"},\"done\":false}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":false}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n",
        );
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: base, api_key: None };
        let mut seen = Vec::new();
        let full = stream_chat(&target, "nemotron-3-ultra:cloud", &[json!({"role": "user", "content": "hi"})], None, |_, delta| seen.push(delta.to_string())).await.unwrap();
        assert_eq!(full.0, "Hello");
        assert_eq!(seen, vec!["Hel".to_string(), "lo".to_string()]);
    }

    #[tokio::test]
    async fn stream_chat_accumulates_openai_sse_deltas() {
        let base = serve_once(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\
             data: [DONE]\n",
        );
        let target = HttpTarget { vendor: HttpVendor::LmStudio, base_url: base, api_key: None };
        let full = stream_chat(&target, "some-model", &[json!({"role": "user", "content": "hi"})], None, |_, _| {}).await.unwrap();
        assert_eq!(full.0, "Hello");
    }

    #[tokio::test]
    async fn stream_chat_accumulates_openai_sse_reasoning_and_unspaced_data_prefix() {
        let base = serve_once(
            "200 OK",
            "data:{\"choices\":[{\"delta\":{\"reasoning_content\":\"Think\"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"ing...\"}}]}\n\
             data: [DONE]",
        );
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut seen = Vec::new();
        let full = stream_chat(&target, "some-model", &[json!({"role": "user", "content": "hi"})], None, |d, t| seen.push((d, t.to_string()))).await.unwrap();
        // The thought is PUBLISHED but is not part of the reply — see
        // `a_reasoning_delta_carrying_an_empty_content_still_streams`.
        assert_eq!(full.0, "ing...");
        assert_eq!(
            seen,
            vec![(Doing::Thinking, "Think".to_string()), (Doing::Speaking, "ing...".to_string())]
        );
    }

    #[tokio::test]
    async fn stream_chat_accumulates_ollama_reasoning_deltas() {
        let base = serve_once(
            "200 OK",
            "{\"message\":{\"role\":\"assistant\",\"thinking\":\"Thought\"},\"done\":false}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"Answer\"},\"done\":false}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}",
        );
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: base, api_key: None };
        let full = stream_chat(&target, "deepseek-r1", &[json!({"role": "user", "content": "hi"})], None, |_, _| {}).await.unwrap();
        assert_eq!(full.0, "Answer");
    }

    /// **The OpenRouter streaming bug, captured from the wire on 2026-08-02.**
    /// `nvidia/nemotron-3-ultra-550b-a55b:free` sends its whole reasoning phase as
    /// deltas carrying `"content": ""` — an EMPTY STRING, not `null` — alongside
    /// `reasoning`. `content.as_deref().or_else(reasoning)` therefore short-circuits
    /// on `Some("")` and the `or_else` never runs, so nothing at all was emitted for
    /// the entire thinking phase (17 of that turn's 32 SSE lines): the Live card sat
    /// dead until the answer began. An absent field and an empty one must mean the
    /// same thing here.
    #[tokio::test]
    async fn a_reasoning_delta_carrying_an_empty_content_still_streams() {
        let base = serve_once(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"\",\"role\":\"assistant\",\"reasoning\":\"The\"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"\",\"role\":\"assistant\",\"reasoning\":\" user\"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"BA\",\"role\":\"assistant\"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"NANA\",\"role\":\"assistant\"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"\",\"role\":\"assistant\",\"reasoning\":null},\"finish_reason\":\"stop\"}]}\n\
             data: [DONE]\n",
        );
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut seen = Vec::new();
        let full = stream_chat(&target, "nemotron", &[json!({"role": "user", "content": "hi"})], None, |d, t| seen.push((d, t.to_string()))).await.unwrap();
        assert_eq!(full.0, "BANANA", "the reply is the content, never the chain of thought");
        assert_eq!(
            seen,
            vec![
                (Doing::Thinking, "The".to_string()),
                (Doing::Thinking, " user".to_string()),
                (Doing::Speaking, "BA".to_string()),
                (Doing::Speaking, "NANA".to_string()),
            ],
            "the thinking phase must stream too — that is what the Live card shows"
        );
    }

    /// A model that answers entirely inside its reasoning channel (some `:free`
    /// OpenRouter routes, and `deepseek-r1` on a bad day) would otherwise return an
    /// EMPTY reply, which reads to the swarm as a dead turn rather than as a model
    /// that never left its thought stream. The thought is the only thing it said, so
    /// it is what it gets to say.
    #[tokio::test]
    async fn a_reply_that_is_all_reasoning_falls_back_to_the_thought_stream() {
        let base = serve_once(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"\",\"reasoning\":\"only \"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"\",\"reasoning\":\"thoughts\"}}]}\n\
             data: [DONE]\n",
        );
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let full = stream_chat(&target, "some-model", &[json!({"role": "user", "content": "hi"})], None, |_, _| {}).await.unwrap();
        assert_eq!(full.0, "only thoughts");
    }

    /// Jake's Ollama seat died with a bare `403 Forbidden` that named no reason —
    /// the body says which model or key was refused and we were throwing it away.
    #[tokio::test]
    async fn a_refused_chat_request_reports_the_servers_own_explanation() {
        let base = serve_once("403 Forbidden", r#"{"error":"model 'kimi-k2.7-code:cloud' not found"}"#);
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: base, api_key: None };
        let err = stream_chat(&target, "kimi-k2.7-code:cloud", &[json!({"role": "user", "content": "hi"})], None, |_, _| {}).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("403"), "{msg}");
        assert!(msg.contains("not found"), "the server's own explanation is missing: {msg}");
    }

    #[test]
    fn for_seat_resolves_the_vendors_default_base_url_when_the_seat_names_none() {
        let mut seat = Seat::cli(QuarkId::new("http-ollama"), "ollama", "nemotron-3-ultra:cloud", Flavor::Worker);
        seat.transport = Transport::Http;
        let target = HttpTarget::for_seat(&seat).unwrap();
        assert_eq!(target.vendor, HttpVendor::Ollama);
        assert_eq!(target.base_url, "http://localhost:11434");
    }

    #[test]
    fn for_seat_is_none_for_a_non_http_seat() {
        let seat = Seat::cli(QuarkId::new("cli-agy"), "agy", "gemini", Flavor::Worker);
        assert!(HttpTarget::for_seat(&seat).is_none());
    }

    #[test]
    fn for_seat_with_env_attaches_the_resolved_secret_as_the_api_key() {
        use hadron_lattice::secrets::{MemoryStore, SecretStore};
        let mut seat = Seat::cli(QuarkId::new("http-openrouter"), "openai-compatible", "some-model", Flavor::Worker);
        seat.transport = Transport::Http;
        seat.secret_env = vec!["API_KEY".to_string()];
        let store = MemoryStore::new();
        store.set(&seat.id, "API_KEY", "sk-live-123").unwrap();
        let target = HttpTarget::for_seat_with_env(&seat, &store).unwrap();
        assert_eq!(target.api_key, Some("sk-live-123".to_string()));
    }

    #[test]
    fn for_seat_with_env_is_a_no_op_with_no_declared_secret_env() {
        let mut seat = Seat::cli(QuarkId::new("http-ollama"), "ollama", "nemotron-3-ultra:cloud", Flavor::Worker);
        seat.transport = Transport::Http;
        let store = hadron_lattice::secrets::MemoryStore::new();
        let target = HttpTarget::for_seat_with_env(&seat, &store).unwrap();
        assert_eq!(target.api_key, None);
    }

    /// Not a fixture: hits the real Ollama server running on this box
    /// (`curl http://localhost:11434/api/tags` returned `gemma3:27b` live during
    /// this task). Ignored by default because CI/other boxes have no Ollama.
    #[test]
    #[ignore = "hits the real local Ollama server — run manually with `--ignored`"]
    fn fetch_models_reaches_the_real_local_ollama() {
        let target =
            HttpTarget { vendor: HttpVendor::Ollama, base_url: HttpVendor::Ollama.default_base_url().to_string(), api_key: None };
        let models = fetch_models(&target).unwrap();
        assert!(!models.is_empty(), "expected at least one model from the live Ollama server");
    }

    /// Not a fixture: runs a real chat turn against the live Ollama server on this
    /// box and checks the streamed reply actually answers.
    #[tokio::test]
    #[ignore = "hits the real local Ollama server — run manually with `--ignored`"]
    async fn stream_chat_reaches_the_real_local_ollama() {
        let target =
            HttpTarget { vendor: HttpVendor::Ollama, base_url: HttpVendor::Ollama.default_base_url().to_string(), api_key: None };
        // Ask the server which model it has rather than naming one: the box's
        // Ollama store emptied mid-project and this test hard-coded `gemma3:27b`,
        // so it failed for a missing model rather than for anything it tests.
        let listed = {
            let t = target.clone();
            tokio::task::spawn_blocking(move || fetch_models(&t)).await.unwrap().unwrap()
        };
        let model = listed.first().expect("the live Ollama server has no models pulled").id.clone();
        let mut deltas = 0;
        let (full, _, _) = stream_chat(&target, &model, &[json!({"role": "user", "content": "Reply with exactly one word: pong"})], None, |_, _| deltas += 1)
            .await
            .unwrap();
        assert!(deltas > 0, "expected at least one streamed delta");
        assert!(full.to_lowercase().contains("pong"), "got: {full:?}");
    }

    /// Not a fixture: a real turn against the live OpenRouter seat, on the
    /// reasoning model whose empty-`content` deltas were the bug. Proves the fix
    /// against the wire rather than against our own capture of it — a fixture can
    /// only ever re-assert what we already believed.
    #[tokio::test]
    #[ignore = "hits the real OpenRouter endpoint and spends the seat's key — run manually with `--ignored`"]
    async fn stream_chat_reaches_the_real_openrouter_reasoning_model() {
        use hadron_lattice::secrets::SecretStore;
        let seat = QuarkId::new("http-openai-compatible");
        let key = crate::secrets::KeyringStore::new()
            .get(&seat, "API_KEY")
            .unwrap()
            .expect("the http-openai-compatible seat has no API_KEY in the keyring");
        let target = HttpTarget {
            vendor: HttpVendor::OpenAiCompatible,
            base_url: HttpVendor::OpenAiCompatible.default_base_url().to_string(),
            api_key: Some(key),
        };
        let (mut thoughts, mut says) = (0, 0);
        let (full, _calls, usage) = stream_chat(
            &target,
            "nvidia/nemotron-3-ultra-550b-a55b:free",
            &[json!({ "role": "user", "content": "Say the word BANANA three times, then stop." })],
            None,
            |doing, _| {
                if doing == Doing::Thinking {
                    thoughts += 1
                } else {
                    says += 1
                }
            },
        )
        .await
        .unwrap();
        assert!(thoughts > 0, "the reasoning phase streamed nothing — this is the bug");
        assert!(says > 0, "the answer streamed nothing");
        assert!(full.to_uppercase().contains("BANANA"), "got: {full:?}");
        assert!(!full.to_lowercase().contains("the user wants"), "chain of thought leaked into the reply: {full:?}");
        assert!(usage.spend.output.unwrap_or(0) > 0, "no usage reported: {usage:?}");
    }

    /// Not a fixture: the LM Studio server on the Windows side of this box, reached
    /// from WSL at the address the human's own seat uses — deliberately written
    /// WITHOUT the `/v1` suffix, so a pass proves the normalisation too.
    #[test]
    #[ignore = "hits the real LM Studio server on the Windows host — run manually with `--ignored`"]
    fn fetch_models_reaches_the_real_lm_studio() {
        let target =
            HttpTarget { vendor: HttpVendor::LmStudio, base_url: "http://10.5.0.2:1234".to_string(), api_key: None };
        let models = fetch_models(&target).unwrap();
        assert!(!models.is_empty(), "expected at least one model from the live LM Studio server");
    }

    #[tokio::test]
    #[ignore = "hits the real LM Studio server on the Windows host — run manually with `--ignored`"]
    async fn stream_chat_reaches_the_real_lm_studio() {
        let target =
            HttpTarget { vendor: HttpVendor::LmStudio, base_url: "http://10.5.0.2:1234".to_string(), api_key: None };
        let mut deltas = 0;
        let (full, _, _) = stream_chat(&target, "google/gemma-4-12b-qat", &[json!({"role": "user", "content": "Reply with exactly one word: pong"})], None, |_, _| deltas += 1)
            .await
            .unwrap();
        assert!(deltas > 0, "expected at least one streamed delta");
        assert!(full.to_lowercase().contains("pong"), "got: {full:?}");
    }

    /// Serves `first` on the first connection accepted and `second` on the
    /// second, capturing both raw requests — for asserting that round 2 of the
    /// tool loop actually carries round 1's tool result back.
    fn serve_twice_capturing(
        first: String,
        second: String,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for body in [first, second] {
                let Ok((mut stream, _)) = listener.accept() else { return };
                stream.set_read_timeout(Some(Duration::from_millis(200))).ok();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                while let Ok(n) = stream.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}"), rx)
    }

    /// A `Projection` for a `LocalQuark::excite` test — every field at its
    /// inert value except `cwd` (which the tool loop jails `hadron_forge::Root`
    /// to) and `mode`, which defaults to `Bypass` so a test exercising the tool
    /// loop's mechanics is not incidentally blocked by permission gating; tests
    /// of the gating itself override `mode` explicitly.
    fn tool_loop_turn(cwd: std::path::PathBuf) -> Projection {
        Projection {
            isolated: true,
            task: "list the current directory".into(),
            invariants: String::new(),
            available_invariants: vec![],
            nucleus_digest: String::new(),
            live_activities: vec![],
            roster: vec![],
            field_window: vec![],
            field_truncated: false,
            nucleus_index: String::new(),
            nucleus_index_path: std::path::PathBuf::new(),
            nucleus_index_truncated: false,
            nucleus_index_budget_bytes: hadron_lattice::DEFAULT_NUCLEUS_INDEX_BUDGET_BYTES,
            nucleus_notes_dir: std::path::PathBuf::new(),
            git_diff: String::new(),
            cwd,
            mode: hadron_lattice::Mode::Bypass,
            role_body: None,
            active_skill: None,
            named_specifically: true,
            has_forge_tools: false,
        }
    }

    /// The end-to-end proof that Task 2 actually closes the loop: a real
    /// `LocalQuark::excite` call, given a reply that asks for `list_dir`, must
    /// run it against the real filesystem (jailed to `turn.cwd`) and send the
    /// result back as a `role: "tool"` message on the NEXT request — not just
    /// parse the shape of a canned reply (every other test in this module).
    #[tokio::test]
    async fn the_loop_runs_the_requested_tool_and_sends_its_result_back() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("marker.txt"), "hi").unwrap();

        let first = format!(
            "data: {}\ndata: [DONE]\n",
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "c1", "type": "function", "function": {"name": "list_dir", "arguments": "{\"path\":\".\"}"}}
            ]}}]})
        );
        let second = format!(
            "data: {}\ndata: [DONE]\n",
            json!({"choices": [{"delta": {"content": "there you go"}}]})
        );
        let (base, rx) = serve_twice_capturing(first, second);
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut q = LocalQuark::new(QuarkId::new("t"), Flavor::Worker, "m", target);

        let out = q.excite(tool_loop_turn(dir.path().to_path_buf())).await.expect("turn");
        assert_eq!(out.message.as_deref(), Some("there you go"));

        let _first_request = rx.recv_timeout(Duration::from_secs(2)).expect("round 1 request");
        let second_request = rx.recv_timeout(Duration::from_secs(2)).expect("round 2 request");
        assert!(
            second_request.contains(r#""role":"tool""#),
            "the tool result was never sent back to the model: {second_request}"
        );
        assert!(
            second_request.contains("marker.txt"),
            "the tool result must carry the REAL directory listing (jailed to turn.cwd): {second_request}"
        );
    }

    /// Ollama's native `/api/chat` REJECTS the whole request — `400 Bad
    /// Request`, `{"error":"Value looks like object, but can't find closing
    /// '}' symbol"}` — when the assistant echo carries `arguments` as a
    /// JSON-encoded string instead of an object. Reproduced live against
    /// `http://localhost:11434` on 2026-08-02: the identical request with
    /// `"arguments":{"path":"/tmp"}` answers normally, so round 1 succeeds and
    /// round 2 kills the turn.
    #[tokio::test]
    async fn an_ollama_assistant_echo_carries_its_arguments_as_an_object() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("marker.txt"), "hi").unwrap();

        let first = format!(
            "{}\n",
            json!({"message": {"role": "assistant", "content": "", "tool_calls": [
                {"id": "c1", "function": {"name": "list_dir", "arguments": {"path": "."}}}
            ]}})
        );
        let second = format!("{}\n", json!({"message": {"content": "there you go"}, "done": true}));
        let (base, rx) = serve_twice_capturing(first, second);
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: base, api_key: None };
        let mut q = LocalQuark::new(QuarkId::new("t"), Flavor::Worker, "m", target);

        let out = q.excite(tool_loop_turn(dir.path().to_path_buf())).await.expect("turn");
        assert_eq!(out.message.as_deref(), Some("there you go"));

        let _first_request = rx.recv_timeout(Duration::from_secs(2)).expect("round 1 request");
        let second_request = rx.recv_timeout(Duration::from_secs(2)).expect("round 2 request");
        assert!(
            second_request.contains(r#""arguments":{"path":"."}"#),
            "Ollama needs the echoed arguments as an object, not a string: {second_request}"
        );
    }

    /// The other half of the same rule: an OpenAI-compatible endpoint wants the
    /// string form, so the fix above must not become a blanket change.
    #[tokio::test]
    async fn an_openai_assistant_echo_keeps_its_arguments_as_a_string() {
        let dir = tempfile::tempdir().unwrap();
        let first = format!(
            "data: {}\ndata: [DONE]\n",
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "c1", "type": "function", "function": {"name": "list_dir", "arguments": "{\"path\":\".\"}"}}
            ]}}]})
        );
        let second = format!("data: {}\ndata: [DONE]\n", json!({"choices": [{"delta": {"content": "done"}}]}));
        let (base, rx) = serve_twice_capturing(first, second);
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut q = LocalQuark::new(QuarkId::new("t"), Flavor::Worker, "m", target);

        q.excite(tool_loop_turn(dir.path().to_path_buf())).await.expect("turn");
        let _first = rx.recv_timeout(Duration::from_secs(2)).expect("round 1 request");
        let second_request = rx.recv_timeout(Duration::from_secs(2)).expect("round 2 request");
        assert!(
            second_request.contains(r#""arguments":"{\"path\":\".\"}""#),
            "an OpenAI-compatible echo must keep the JSON-encoded string form: {second_request}"
        );
    }

    /// Positive control for the two negative-shaped tests below: proves
    /// `serve_once_capturing`'s drained request actually carries `"tools"` when
    /// the mode permits declaring them, so an absent key in the Ask test means
    /// the request really omitted it — not that the harness failed to capture it.
    #[tokio::test]
    async fn a_bypass_mode_http_quark_is_offered_every_declared_tool() {
        let dir = tempfile::tempdir().unwrap();
        let (base, rx) =
            serve_once_capturing("200 OK", "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\ndata: [DONE]\n");
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut q = LocalQuark::new(QuarkId::new("t"), Flavor::Worker, "m", target);

        let turn = Projection { mode: hadron_lattice::Mode::Bypass, ..tool_loop_turn(dir.path().to_path_buf()) };
        q.excite(turn).await.expect("turn");
        let request = rx.recv_timeout(Duration::from_secs(2)).expect("request");
        assert!(request.contains(r#""tools":"#), "Bypass must declare tools: {request}");
        assert!(request.contains(r#""name":"exec""#), "Bypass must declare exec too: {request}");
    }

    /// Step 1: `Ask` means "talk, don't act" — the request must not carry a
    /// `tools` key at all, not merely refuse every call once offered. A refused
    /// call still burns a tool round the model has no fallback for.
    #[tokio::test]
    async fn an_ask_mode_http_quark_is_offered_no_tools() {
        let dir = tempfile::tempdir().unwrap();
        let (base, rx) =
            serve_once_capturing("200 OK", "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\ndata: [DONE]\n");
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut q = LocalQuark::new(QuarkId::new("t"), Flavor::Worker, "m", target);

        let turn = Projection { mode: hadron_lattice::Mode::Ask, ..tool_loop_turn(dir.path().to_path_buf()) };
        q.excite(turn).await.expect("turn");
        let request = rx.recv_timeout(Duration::from_secs(2)).expect("request");
        assert!(!request.contains(r#""tools":"#), "Ask must offer no tools at all: {request}");
    }

    /// Step 2: `Write` auto-approves edits but asks for every command — so the
    /// forge tools are declared and `exec` is not.
    #[tokio::test]
    async fn a_write_mode_http_quark_is_offered_forge_but_not_exec() {
        let dir = tempfile::tempdir().unwrap();
        let (base, rx) =
            serve_once_capturing("200 OK", "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\ndata: [DONE]\n");
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut q = LocalQuark::new(QuarkId::new("t"), Flavor::Worker, "m", target);

        let turn = Projection { mode: hadron_lattice::Mode::Write, ..tool_loop_turn(dir.path().to_path_buf()) };
        q.excite(turn).await.expect("turn");
        let request = rx.recv_timeout(Duration::from_secs(2)).expect("request");
        assert!(request.contains(r#""name":"read_file""#), "Write must declare forge tools: {request}");
        assert!(!request.contains(r#""name":"exec""#), "Write must not declare exec: {request}");
    }

    /// Step 5: `mode_guidance(Mode::Auto)` tells the model "ungated shell commands
    /// are not available" while `declarations_for_mode(Auto)` declares `exec` —
    /// a prompt contradicting the wire. The prompt sent at `Auto` must qualify
    /// that `exec` is a jailed allowlist, not the "ungated shell" the authority
    /// note refers to, so the model does not read the two as disagreeing.
    #[tokio::test]
    async fn an_auto_mode_http_quark_is_told_exec_is_jailed_not_ungated() {
        let dir = tempfile::tempdir().unwrap();
        let (base, rx) =
            serve_once_capturing("200 OK", "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\ndata: [DONE]\n");
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut q = LocalQuark::new(QuarkId::new("t"), Flavor::Worker, "m", target);

        let turn = Projection { mode: hadron_lattice::Mode::Auto, ..tool_loop_turn(dir.path().to_path_buf()) };
        q.excite(turn).await.expect("turn");
        let request = rx.recv_timeout(Duration::from_secs(2)).expect("request");
        assert!(request.contains(r#""name":"exec""#), "Auto must still declare exec: {request}");
        assert!(
            request.contains("jailed") && request.contains("cargo"),
            "Auto's prompt must qualify exec as a jailed allowlist, not ungated shell: {request}"
        );
    }

    /// The same contradiction does not exist at `Write` (no `exec` declared) or
    /// `Bypass` (guidance already says "full tool access") — the note would be
    /// noise there, so it must not appear.
    #[tokio::test]
    async fn the_exec_note_only_appears_at_auto() {
        let dir = tempfile::tempdir().unwrap();
        let (base, rx) =
            serve_once_capturing("200 OK", "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\ndata: [DONE]\n");
        let target = HttpTarget { vendor: HttpVendor::OpenAiCompatible, base_url: base, api_key: None };
        let mut q = LocalQuark::new(QuarkId::new("t"), Flavor::Worker, "m", target);

        let turn = Projection { mode: hadron_lattice::Mode::Bypass, ..tool_loop_turn(dir.path().to_path_buf()) };
        q.excite(turn).await.expect("turn");
        let request = rx.recv_timeout(Duration::from_secs(2)).expect("request");
        assert!(!request.contains("jailed"), "Bypass's guidance is already accurate, no note needed: {request}");
    }
}

