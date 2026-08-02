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
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 1],
    stream: bool,
}

#[derive(Deserialize, Default)]
struct OllamaChatMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAiChoice {
    #[serde(default)]
    delta: OpenAiDelta,
}

#[derive(Deserialize, Default)]
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
    prompt: &str,
    mut on_delta: impl FnMut(Doing, &str),
) -> anyhow::Result<(String, hadron_lattice::Usage)> {
    let client = reqwest::Client::new();
    let body = ChatRequest { model, messages: [ChatMessage { role: "user", content: prompt }], stream: true };
    let mut full = String::new();
    let mut thought = String::new();
    let mut usage = hadron_lattice::Usage::default();
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
                    if let Some(c) = chunk.choices.first() {
                        if let Some(content) = present(&c.delta.content) {
                            full.push_str(content);
                            on_delta(Doing::Speaking, content);
                        } else if let Some(t) = present(&c.delta.reasoning_content)
                            .or_else(|| present(&c.delta.reasoning))
                        {
                            thought.push_str(t);
                            on_delta(Doing::Thinking, t);
                        }
                    }
                }
            })
            .await?;
        }
    }
    // A model that never left its reasoning channel said exactly one thing; an
    // empty reply would report that turn to the swarm as silence.
    if full.is_empty() {
        full = thought;
    }
    Ok((full, usage))
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
        let prompt = crate::adapter::prompt::build(&turn, &self.id);
        let quark_id = self.id.clone();
        let dir = self.live_dir.clone();
        let mut draft = String::new();
        let mut thought = String::new();
        let mut last_publish: Option<Instant> = None;
        let (message, usage) = stream_chat(&self.target, &self.model, &prompt, |doing, delta| {
            // Two buffers, because they are two different things: the draft is the
            // reply the chat will show, the thought is only ever the Live card's
            // "thinking" line. Mirrors `adapter::acp::session`'s split.
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
        if let Some(dir) = &self.live_dir {
            let _ = live::clear(dir, &self.id);
        }
        Ok(TurnOutcome { message: Some(message), usage, ..Default::default() })
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
    /// for asserting on a header (`Authorization: Bearer …`, or its absence).
    fn serve_once_capturing(status: &'static str, body: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}"), rx)
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
        stream_chat(&target, "some-model", "hi", |_, _| {}).await.unwrap();
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
            r#"{"models":[{"name":"gemma3:27b","model":"gemma3:27b","modified_at":"2026-05-24T17:56:53Z","size":1,"digest":"x","details":{"format":"gguf","family":"gemma3","parameter_size":"27.4B","quantization_level":"Q4_K_M"}}]}"#,
        );
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: base, api_key: None };
        let models = fetch_models(&target).unwrap();
        assert_eq!(models, vec![LocalModel { id: "gemma3:27b".to_string() }]);
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
        let full = stream_chat(&target, "gemma3:27b", "hi", |_, delta| seen.push(delta.to_string())).await.unwrap();
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
        let full = stream_chat(&target, "some-model", "hi", |_, _| {}).await.unwrap();
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
        let full = stream_chat(&target, "some-model", "hi", |d, t| seen.push((d, t.to_string()))).await.unwrap();
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
        let full = stream_chat(&target, "deepseek-r1", "hi", |_, _| {}).await.unwrap();
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
        let full = stream_chat(&target, "nemotron", "hi", |d, t| seen.push((d, t.to_string()))).await.unwrap();
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
        let full = stream_chat(&target, "some-model", "hi", |_, _| {}).await.unwrap();
        assert_eq!(full.0, "only thoughts");
    }

    /// Jake's Ollama seat died with a bare `403 Forbidden` that named no reason —
    /// the body says which model or key was refused and we were throwing it away.
    #[tokio::test]
    async fn a_refused_chat_request_reports_the_servers_own_explanation() {
        let base = serve_once("403 Forbidden", r#"{"error":"model 'kimi-k2.7-code:cloud' not found"}"#);
        let target = HttpTarget { vendor: HttpVendor::Ollama, base_url: base, api_key: None };
        let err = stream_chat(&target, "kimi-k2.7-code:cloud", "hi", |_, _| {}).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("403"), "{msg}");
        assert!(msg.contains("not found"), "the server's own explanation is missing: {msg}");
    }

    #[test]
    fn for_seat_resolves_the_vendors_default_base_url_when_the_seat_names_none() {
        let mut seat = Seat::cli(QuarkId::new("http-ollama"), "ollama", "gemma3:27b", Flavor::Worker);
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
        let mut seat = Seat::cli(QuarkId::new("http-ollama"), "ollama", "gemma3:27b", Flavor::Worker);
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
        let (full, _) = stream_chat(&target, &model, "Reply with exactly one word: pong", |_, _| deltas += 1)
            .await
            .unwrap();
        assert!(deltas > 0, "expected at least one streamed delta");
        assert!(full.to_lowercase().contains("pong"), "got: {full:?}");
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
        let (full, _) = stream_chat(&target, "google/gemma-4-12b-qat", "Reply with exactly one word: pong", |_, _| deltas += 1)
            .await
            .unwrap();
        assert!(deltas > 0, "expected at least one streamed delta");
        assert!(full.to_lowercase().contains("pong"), "got: {full:?}");
    }
}
