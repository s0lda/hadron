//! A quark backed by a keyless local HTTP server — Ollama or LM Studio — over
//! [`hadron_lattice::Transport::Http`]. No subprocess, no protocol handshake: just
//! a GET to list models and a POST to run one turn, both against `localhost`.
//!
//! Wire contracts read from the vendors' own APIs (Zed's checked-in `ollama`/
//! `lmstudio` crates were the reference for the shapes, confirmed live against a
//! running Ollama on this box and LM Studio's own server log):
//! - **Ollama**: `GET {base}/api/tags` lists models; `POST {base}/api/chat` streams
//!   newline-delimited JSON chat chunks.
//! - **LM Studio**: the OpenAI-compatible surface, `GET {base}/models` and
//!   `POST {base}/chat/completions`, streaming Server-Sent-Events `data: {...}`
//!   lines terminated by `data: [DONE]`.
//!
//! Deserialization is deliberately lenient — every struct here names only the
//! field this adapter actually reads, so a vendor's response growing an unrelated
//! field never breaks Connect or a turn.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::StreamExt;
use hadron_lattice::live::{self, Activity};
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, Seat, SeatCommands, Transport, TurnOutcome};
use serde::{Deserialize, Serialize};

use crate::quark::Quark;

/// Which local HTTP provider a [`Transport::Http`] seat speaks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalVendor {
    Ollama,
    LmStudio,
}

impl LocalVendor {
    /// Parse a seat's `vendor` string. `None` for anything else — a
    /// [`Transport::Http`] seat on an unknown vendor has nothing to boot.
    pub fn parse(vendor: &str) -> Option<LocalVendor> {
        match vendor {
            "ollama" => Some(LocalVendor::Ollama),
            "lmstudio" => Some(LocalVendor::LmStudio),
            _ => None,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            LocalVendor::Ollama => "ollama",
            LocalVendor::LmStudio => "lmstudio",
        }
    }

    /// The zero-setup default base URL — what "just works" against the vendor's
    /// own out-of-the-box defaults, with no env var and no config file.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            LocalVendor::Ollama => "http://localhost:11434",
            LocalVendor::LmStudio => "http://localhost:1234/v1",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            LocalVendor::Ollama => "Ollama (local)",
            LocalVendor::LmStudio => "LM Studio (local)",
        }
    }
}

/// How to reach a [`Transport::Http`] seat's server: which vendor's wire shape to
/// speak, and the base URL (the seat's own, or the vendor's default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpTarget {
    pub vendor: LocalVendor,
    pub base_url: String,
}

impl HttpTarget {
    /// The target for a seat, or `None` for a non-`Http` seat or an unknown vendor.
    pub fn for_seat(seat: &Seat) -> Option<HttpTarget> {
        if seat.transport != Transport::Http {
            return None;
        }
        let vendor = LocalVendor::parse(&seat.vendor)?;
        let base_url =
            seat.http_base_url.clone().unwrap_or_else(|| vendor.default_base_url().to_string());
        Some(HttpTarget { vendor, base_url })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url.trim_end_matches('/'))
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
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(5)).build()?;
    match target.vendor {
        LocalVendor::Ollama => {
            let resp = client.get(target.url("/api/tags")).send()?;
            anyhow::ensure!(resp.status().is_success(), "Ollama returned {}", resp.status());
            let body: OllamaTagsResponse = resp.json()?;
            Ok(body.models.into_iter().map(|m| LocalModel { id: m.name }).collect())
        }
        LocalVendor::LmStudio => {
            let resp = client.get(target.url("/models")).send()?;
            anyhow::ensure!(resp.status().is_success(), "LM Studio returned {}", resp.status());
            let body: OpenAiModelsResponse = resp.json()?;
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
}

#[derive(Deserialize)]
struct OllamaChatChunk {
    #[serde(default)]
    message: Option<OllamaChatMessage>,
}

#[derive(Deserialize, Default)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAiChoice {
    #[serde(default)]
    delta: OpenAiDelta,
}

#[derive(Deserialize)]
struct OpenAiChatChunk {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
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
    Ok(())
}

/// Run one chat turn against `target`, streaming each text delta to `on_delta` as
/// it arrives (for live-activity publishing) and returning the full accumulated
/// reply once the stream ends.
async fn stream_chat(
    target: &HttpTarget,
    model: &str,
    prompt: &str,
    mut on_delta: impl FnMut(&str),
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let body = ChatRequest { model, messages: [ChatMessage { role: "user", content: prompt }], stream: true };
    let mut full = String::new();
    match target.vendor {
        LocalVendor::Ollama => {
            let resp = client.post(target.url("/api/chat")).json(&body).send().await?;
            anyhow::ensure!(resp.status().is_success(), "Ollama chat request failed: {}", resp.status());
            for_each_line(resp, |line| {
                if let Ok(chunk) = serde_json::from_str::<OllamaChatChunk>(line) {
                    if let Some(msg) = chunk.message {
                        if !msg.content.is_empty() {
                            full.push_str(&msg.content);
                            on_delta(&msg.content);
                        }
                    }
                }
            })
            .await?;
        }
        LocalVendor::LmStudio => {
            let resp = client.post(target.url("/chat/completions")).json(&body).send().await?;
            anyhow::ensure!(resp.status().is_success(), "LM Studio chat request failed: {}", resp.status());
            for_each_line(resp, |line| {
                let Some(data) = line.strip_prefix("data: ") else { return };
                if data == "[DONE]" {
                    return;
                }
                if let Ok(chunk) = serde_json::from_str::<OpenAiChatChunk>(data) {
                    if let Some(content) = chunk.choices.first().and_then(|c| c.delta.content.as_deref()) {
                        if !content.is_empty() {
                            full.push_str(content);
                            on_delta(content);
                        }
                    }
                }
            })
            .await?;
        }
    }
    Ok(full)
}

/// The minimum gap between two published draft updates — mirrors
/// `adapter::acp::session::LiveFeed::THROTTLE` (private to that module), so a
/// local model streaming a chunk every few tokens doesn't rewrite the live file
/// hundreds of times a turn.
const PUBLISH_THROTTLE: Duration = Duration::from_millis(200);

/// A quark backed by a local HTTP server (Ollama, LM Studio). Single-shot, like
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
        let mut last_publish: Option<Instant> = None;
        let message = stream_chat(&self.target, &self.model, &prompt, |delta| {
            draft.push_str(delta);
            let Some(dir) = &dir else { return };
            let due = last_publish.is_none_or(|t| t.elapsed() >= PUBLISH_THROTTLE);
            if due {
                last_publish = Some(Instant::now());
                let _ = live::publish(dir, &Activity::speaking(quark_id.clone(), &draft));
            }
        })
        .await?;
        if let Some(dir) = &self.live_dir {
            let _ = live::clear(dir, &self.id);
        }
        Ok(TurnOutcome { message: Some(message), ..Default::default() })
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

    #[test]
    fn fetch_models_parses_a_real_ollama_tags_response() {
        // Captured live from `curl http://localhost:11434/api/tags` on this box.
        let base = serve_once(
            "200 OK",
            r#"{"models":[{"name":"gemma3:27b","model":"gemma3:27b","modified_at":"2026-05-24T17:56:53Z","size":1,"digest":"x","details":{"format":"gguf","family":"gemma3","parameter_size":"27.4B","quantization_level":"Q4_K_M"}}]}"#,
        );
        let target = HttpTarget { vendor: LocalVendor::Ollama, base_url: base };
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
        let target = HttpTarget { vendor: LocalVendor::LmStudio, base_url: base };
        let models = fetch_models(&target).unwrap();
        assert_eq!(models, vec![LocalModel { id: "google/gemma-4-12b-qat".to_string() }]);
    }

    #[test]
    fn fetch_models_reports_a_connection_failure_plainly() {
        // Nothing is listening on this port — the Connect button's "server not running" path.
        let target = HttpTarget { vendor: LocalVendor::Ollama, base_url: "http://127.0.0.1:1".to_string() };
        assert!(fetch_models(&target).is_err());
    }

    #[tokio::test]
    async fn stream_chat_accumulates_ollama_ndjson_deltas() {
        let base = serve_once(
            "200 OK",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"Hel\"},\"done\":false}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":false}\n\
             {\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n",
        );
        let target = HttpTarget { vendor: LocalVendor::Ollama, base_url: base };
        let mut seen = Vec::new();
        let full = stream_chat(&target, "gemma3:27b", "hi", |delta| seen.push(delta.to_string())).await.unwrap();
        assert_eq!(full, "Hello");
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
        let target = HttpTarget { vendor: LocalVendor::LmStudio, base_url: base };
        let full = stream_chat(&target, "some-model", "hi", |_| {}).await.unwrap();
        assert_eq!(full, "Hello");
    }

    #[test]
    fn for_seat_resolves_the_vendors_default_base_url_when_the_seat_names_none() {
        let mut seat = Seat::cli(QuarkId::new("http-ollama"), "ollama", "gemma3:27b", Flavor::Worker);
        seat.transport = Transport::Http;
        let target = HttpTarget::for_seat(&seat).unwrap();
        assert_eq!(target.vendor, LocalVendor::Ollama);
        assert_eq!(target.base_url, "http://localhost:11434");
    }

    #[test]
    fn for_seat_is_none_for_a_non_http_seat() {
        let seat = Seat::cli(QuarkId::new("cli-agy"), "agy", "gemini", Flavor::Worker);
        assert!(HttpTarget::for_seat(&seat).is_none());
    }

    /// Not a fixture: hits the real Ollama server running on this box
    /// (`curl http://localhost:11434/api/tags` returned `gemma3:27b` live during
    /// this task). Ignored by default because CI/other boxes have no Ollama.
    #[test]
    #[ignore = "hits the real local Ollama server — run manually with `--ignored`"]
    fn fetch_models_reaches_the_real_local_ollama() {
        let target =
            HttpTarget { vendor: LocalVendor::Ollama, base_url: LocalVendor::Ollama.default_base_url().to_string() };
        let models = fetch_models(&target).unwrap();
        assert!(!models.is_empty(), "expected at least one model from the live Ollama server");
    }

    /// Not a fixture: runs a real chat turn against the live Ollama server on this
    /// box and checks the streamed reply actually answers.
    #[tokio::test]
    #[ignore = "hits the real local Ollama server — run manually with `--ignored`"]
    async fn stream_chat_reaches_the_real_local_ollama() {
        let target =
            HttpTarget { vendor: LocalVendor::Ollama, base_url: LocalVendor::Ollama.default_base_url().to_string() };
        let mut deltas = 0;
        let full = stream_chat(&target, "gemma3:27b", "Reply with exactly one word: pong", |_| deltas += 1)
            .await
            .unwrap();
        assert!(deltas > 0, "expected at least one streamed delta");
        assert!(full.to_lowercase().contains("pong"), "got: {full:?}");
    }
}
