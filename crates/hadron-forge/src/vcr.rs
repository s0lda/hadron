//! Pure and async logic for the `vcr` (HTTP/RPC record-replay) tool family.
//! Transparent cassette recording and deterministic offline replay proxy jailed under `.hadron/cassettes/`.

use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, RwLock};

use crate::file::{resolve_jailed_path, ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VcrMode {
    Record,
    Replay,
    Verify,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VcrInteraction {
    pub request_method: String,
    pub request_path: String,
    pub request_headers: BTreeMap<String, String>,
    pub request_body: String,
    pub response_status: u16,
    pub response_headers: BTreeMap<String, String>,
    pub response_body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VcrCassette {
    pub name: String,
    pub created_at_ms: u64,
    pub interactions: Vec<VcrInteraction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VcrCassetteSummary {
    pub name: String,
    pub interactions_count: usize,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcrProxySummary {
    pub port: u16,
    pub url: String,
    pub cassette_name: String,
    pub mode: VcrMode,
    pub interactions_recorded: usize,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcrReport {
    pub action: String,
    pub proxy: Option<VcrProxySummary>,
    pub cassettes: Vec<VcrCassetteSummary>,
    pub cassette: Option<VcrCassette>,
    pub summary: String,
}

#[allow(dead_code)]
struct VcrInstance {
    port: u16,
    cassette_name: String,
    mode: VcrMode,
    interactions: Arc<RwLock<Vec<VcrInteraction>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Default)]
pub struct VcrProxyManager {
    proxies: Arc<RwLock<BTreeMap<u16, Arc<RwLock<VcrInstance>>>>>,
}

/// The upstream a `Record` proxy forwards to, parsed from `http://host[:port][/prefix]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcrUpstream {
    pub host: String,
    pub port: u16,
    /// Path prefix prepended to every proxied request path (no trailing `/`).
    pub prefix: String,
}

impl VcrUpstream {
    /// Parse a plain-HTTP upstream URL. `https://` is rejected: the proxy speaks no TLS.
    pub fn parse(url: &str) -> Result<Self, ForgeError> {
        let rest = url.trim().strip_prefix("http://").ok_or_else(|| {
            ForgeError::Rejected(format!(
                "target_url {url:?} must start with http:// (the VCR proxy does not speak TLS)"
            ))
        })?;
        let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (
                h,
                p.parse::<u16>()
                    .map_err(|_| ForgeError::Rejected(format!("bad port in target_url {url:?}")))?,
            ),
            None => (authority, 80),
        };
        if host.is_empty() {
            return Err(ForgeError::Rejected(format!("target_url {url:?} has no host")));
        }
        let prefix = path.trim_end_matches('/');
        Ok(Self {
            host: host.to_string(),
            port,
            prefix: if prefix.is_empty() { String::new() } else { format!("/{prefix}") },
        })
    }

    fn host_header(&self) -> String {
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// A cassette name becomes a file name under `.hadron/cassettes/`, so it may not carry
/// path separators or `..` — that would write outside the cassettes directory.
fn validate_cassette_name(name: &str) -> Result<(), ForgeError> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(ForgeError::Rejected(format!(
            "invalid cassette name {name:?}: use letters, digits, '-', '_' or '.', not starting with '.'"
        )))
    }
}

pub fn load_cassette(root: &Root, name: &str) -> Result<VcrCassette, ForgeError> {
    validate_cassette_name(name)?;
    let cassettes_dir = resolve_jailed_path(root, ".hadron/cassettes")?;
    let path = cassettes_dir.join(format!("{}.json", name));
    if !path.exists() {
        return Err(ForgeError::Rejected(format!("Cassette '{}' not found", name)));
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| ForgeError::Io(format!("Failed reading cassette {name}: {e}")))?;
    serde_json::from_str::<VcrCassette>(&content)
        .map_err(|e| ForgeError::Rejected(format!("Failed parsing cassette {name}: {e}")))
}

pub fn save_cassette(root: &Root, cassette: &VcrCassette) -> Result<(), ForgeError> {
    validate_cassette_name(&cassette.name)?;
    let cassettes_dir = resolve_jailed_path(root, ".hadron/cassettes")?;
    fs::create_dir_all(&cassettes_dir)
        .map_err(|e| ForgeError::Io(format!("Failed creating cassettes directory: {e}")))?;
    let path = cassettes_dir.join(format!("{}.json", cassette.name));
    let content = serde_json::to_string_pretty(cassette)
        .map_err(|e| ForgeError::Io(format!("Failed serializing cassette: {e}")))?;
    fs::write(path, content)
        .map_err(|e| ForgeError::Io(format!("Failed writing cassette: {e}")))?;
    Ok(())
}

pub fn list_cassettes(root: &Root) -> Result<Vec<VcrCassetteSummary>, ForgeError> {
    let cassettes_dir = resolve_jailed_path(root, ".hadron/cassettes")?;
    if !cassettes_dir.exists() {
        return Ok(Vec::new());
    }
    let mut list = Vec::new();
    let Ok(entries) = fs::read_dir(&cassettes_dir) else {
        return Ok(Vec::new());
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() && p.extension().map_or(false, |ext| ext == "json") {
            if let Ok(content) = fs::read_to_string(&p) {
                if let Ok(c) = serde_json::from_str::<VcrCassette>(&content) {
                    list.push(VcrCassetteSummary {
                        name: c.name,
                        interactions_count: c.interactions.len(),
                        created_at_ms: c.created_at_ms,
                    });
                }
            }
        }
    }
    list.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    Ok(list)
}

impl VcrProxyManager {
    pub fn new() -> Self {
        Self {
            proxies: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Start a proxy on `127.0.0.1`. `Record` forwards every request to `target_url`
    /// (required, `http://host[:port][/prefix]`) and records the real exchange;
    /// `Replay`/`Verify` answer from the saved cassette and ignore `target_url`.
    pub async fn start_proxy(
        &self,
        root: Root,
        cassette_name: String,
        mode: VcrMode,
        requested_port: Option<u16>,
        target_url: Option<String>,
    ) -> Result<VcrProxySummary, ForgeError> {
        validate_cassette_name(&cassette_name)?;
        let upstream = match (mode, target_url.as_deref()) {
            (VcrMode::Record, Some(url)) => Some(VcrUpstream::parse(url)?),
            (VcrMode::Record, None) => {
                return Err(ForgeError::Rejected(
                    "record mode needs target_url (http://host[:port][/prefix]) to forward requests to".into(),
                ))
            }
            _ => None,
        };

        let port = requested_port.unwrap_or(0);
        let bind_addr = format!("127.0.0.1:{port}");

        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
            ForgeError::Io(format!("Failed to bind VCR proxy to {bind_addr}: {e}"))
        })?;

        let actual_port = listener.local_addr().map_err(|e| {
            ForgeError::Io(format!("Failed to determine VCR proxy port: {e}"))
        })?.port();

        // Load existing cassette if replay/verify or recording onto existing
        let initial_interactions = if mode == VcrMode::Replay || mode == VcrMode::Verify {
            load_cassette(&root, &cassette_name)?.interactions
        } else {
            load_cassette(&root, &cassette_name)
                .map(|c| c.interactions)
                .unwrap_or_default()
        };

        let interactions = Arc::new(RwLock::new(initial_interactions));
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let interactions_clone = Arc::clone(&interactions);
        let root_clone = root.clone();
        let name_clone = cassette_name.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    Ok((stream, _)) = listener.accept() => {
                        let inter_clone = Arc::clone(&interactions_clone);
                        let r_clone = root_clone.clone();
                        let n_clone = name_clone.clone();
                        let up_clone = upstream.clone();
                        tokio::spawn(async move {
                            let _ = handle_vcr_client(stream, up_clone, inter_clone, r_clone, n_clone).await;
                        });
                    }
                }
            }
        });

        let instance = Arc::new(RwLock::new(VcrInstance {
            port: actual_port,
            cassette_name: cassette_name.clone(),
            mode,
            interactions,
            shutdown_tx: Some(shutdown_tx),
        }));

        let mut map = self.proxies.write().await;
        map.insert(actual_port, instance);

        Ok(VcrProxySummary {
            port: actual_port,
            url: format!("http://127.0.0.1:{actual_port}"),
            cassette_name,
            mode,
            interactions_recorded: 0,
            running: true,
        })
    }

    pub async fn stop_proxy(&self, port: u16) -> Result<bool, ForgeError> {
        let mut map = self.proxies.write().await;
        if let Some(inst_lock) = map.remove(&port) {
            let mut inst = inst_lock.write().await;
            if let Some(tx) = inst.shutdown_tx.take() {
                let _ = tx.send(());
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Upper bound on a proxied request or response, so a client cannot exhaust memory.
const MAX_HTTP_MESSAGE: usize = 32 * 1024 * 1024;

/// Read one HTTP/1.x request from `stream`: the head (up to `\r\n\r\n`) plus a body of
/// `Content-Length` bytes. Returns `(start_line, lowercased headers, body)`.
async fn read_http_request(
    stream: &mut TcpStream,
) -> std::io::Result<Option<(String, BTreeMap<String, String>, Vec<u8>)>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_HTTP_MESSAGE {
            return Err(std::io::Error::other("request head too large"));
        }
    };
    let (start_line, headers) = parse_head(&String::from_utf8_lossy(&buf[..head_end]));
    let body_len = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_HTTP_MESSAGE);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < body_len {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(body_len);
    Ok(Some((start_line, headers, body)))
}

/// Split an HTTP head into its start line and lowercased header map.
fn parse_head(head: &str) -> (String, BTreeMap<String, String>) {
    let mut lines = head.split("\r\n");
    let start_line = lines.next().unwrap_or("").to_string();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect();
    (start_line, headers)
}

/// Headers that describe one hop, not the message: never forwarded or recorded.
fn is_hop_header(name: &str) -> bool {
    matches!(
        name,
        "host" | "connection" | "content-length" | "transfer-encoding" | "keep-alive"
            | "proxy-connection" | "proxy-authorization" | "upgrade" | "te" | "trailer"
    )
}

/// Forward one request to `upstream` and return its `(status, headers, body)`.
///
/// Sent as HTTP/1.0 with `Connection: close`, so the upstream answers with a plain
/// (never chunked) body that ends at EOF — the body we read is the body we record.
async fn forward_upstream(
    upstream: &VcrUpstream,
    method: &str,
    path: &str,
    headers: &BTreeMap<String, String>,
    body: &[u8],
) -> std::io::Result<(u16, BTreeMap<String, String>, String)> {
    let mut conn = TcpStream::connect((upstream.host.as_str(), upstream.port)).await?;
    let mut req = format!("{method} {}{path} HTTP/1.0\r\nHost: {}\r\n", upstream.prefix, upstream.host_header());
    for (k, v) in headers.iter().filter(|(k, _)| !is_hop_header(k)) {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut wire = req.into_bytes();
    wire.extend_from_slice(body);
    conn.write_all(&wire).await?;
    conn.flush().await?;

    let mut raw = Vec::new();
    (&mut conn).take(MAX_HTTP_MESSAGE as u64).read_to_end(&mut raw).await?;
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| std::io::Error::other("upstream sent no complete HTTP head"))?;
    let (status_line, mut resp_headers) = parse_head(&String::from_utf8_lossy(&raw[..head_end]));
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| std::io::Error::other(format!("bad upstream status line {status_line:?}")))?;
    let mut resp_body = raw[head_end + 4..].to_vec();
    if let Some(len) = resp_headers.get("content-length").and_then(|v| v.parse::<usize>().ok()) {
        resp_body.truncate(len);
    }
    resp_headers.retain(|k, _| !is_hop_header(k));
    Ok((status, resp_headers, String::from_utf8_lossy(&resp_body).into_owned()))
}

async fn handle_vcr_client(
    mut stream: TcpStream,
    upstream: Option<VcrUpstream>,
    interactions: Arc<RwLock<Vec<VcrInteraction>>>,
    root: Root,
    cassette_name: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some((first_line, headers, body)) = read_http_request(&mut stream).await? else {
        return Ok(());
    };
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let body = String::from_utf8_lossy(&body).into_owned();

    let (status, mut resp_headers, resp_body) = match &upstream {
        None => {
            let list = interactions.read().await;
            if let Some(found) = list.iter().find(|i| {
                i.request_method.eq_ignore_ascii_case(&method)
                    && (i.request_path == path || path.starts_with(&i.request_path))
            }) {
                (found.response_status, found.response_headers.clone(), found.response_body.clone())
            } else {
                (
                    404,
                    BTreeMap::new(),
                    format!("{{\"error\":\"VCR cassette '{}' has no matching interaction for {} {}\"}}", cassette_name, method, path),
                )
            }
        }
        Some(up) => match forward_upstream(up, &method, &path, &headers, body.as_bytes()).await {
            Err(e) => {
                let mut h = BTreeMap::new();
                h.insert("content-type".to_string(), "application/json".to_string());
                let msg = serde_json::json!({ "error": format!("VCR upstream {}:{} failed: {e}", up.host, up.port) });
                (502, h, msg.to_string())
            }
            Ok((status, resp_headers, resp_body)) => {
                let interaction = VcrInteraction {
                    request_method: method.clone(),
                    request_path: path.clone(),
                    request_headers: headers.into_iter().filter(|(k, _)| !is_hop_header(k)).collect(),
                    request_body: body,
                    response_status: status,
                    response_headers: resp_headers.clone(),
                    response_body: resp_body.clone(),
                };
                let mut out_headers = resp_headers;
                let mut list = interactions.write().await;
                list.push(interaction);
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let cassette = VcrCassette {
                    name: cassette_name.clone(),
                    created_at_ms: now_ms,
                    interactions: list.clone(),
                };
                // The client still gets the real upstream answer, but a cassette that
                // failed to save must not look recorded.
                if let Err(e) = save_cassette(&root, &cassette) {
                    out_headers.insert("x-vcr-record-error".to_string(), e.to_string());
                }
                (status, out_headers, resp_body)
            }
        },
    };
    resp_headers.retain(|k, _| !is_hop_header(k));

    let status_text = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "Status",
    };

    let body_bytes = resp_body.as_bytes();
    let mut response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body_bytes.len()
    );

    for (k, v) in resp_headers {
        response.push_str(&format!("{k}: {v}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(&resp_body);

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-shot loopback HTTP upstream: answers each connection with `body` and
    /// hands the raw request it received back through the channel.
    async fn spawn_upstream(body: &'static str) -> (u16, tokio::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap();
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).into_owned()).await;
                let resp = format!(
                    "HTTP/1.0 201 Created\r\nContent-Type: application/json\r\nX-Upstream: real\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(resp.as_bytes()).await.unwrap();
            }
        });
        (port, rx)
    }

    async fn roundtrip(port: u16, req: &str) -> String {
        let mut client = TcpStream::connect(format!("127.0.0.1:{port}")).await.unwrap();
        client.write_all(req.as_bytes()).await.unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        String::from_utf8_lossy(&resp).into_owned()
    }

    #[tokio::test]
    async fn test_vcr_record_forwards_to_upstream_then_replays_offline() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path());
        let manager = VcrProxyManager::new();
        let (up_port, mut seen) = spawn_upstream(r#"{"users":["ada"]}"#).await;

        // 1. Record: the request reaches the real upstream (with the prefix and a
        //    rewritten Host) and the client gets the upstream's real answer.
        let proxy = manager
            .start_proxy(
                root.clone(),
                "test_api".to_string(),
                VcrMode::Record,
                None,
                Some(format!("http://127.0.0.1:{up_port}/api")),
            )
            .await
            .unwrap();
        let req = "POST /v1/users HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
        let resp = roundtrip(proxy.port, req).await;
        assert!(resp.contains("201 Created"), "{resp}");
        assert!(resp.contains(r#"{"users":["ada"]}"#), "{resp}");
        assert!(resp.contains("x-upstream: real"), "{resp}");
        let upstream_req = seen.recv().await.unwrap();
        assert!(upstream_req.starts_with("POST /api/v1/users HTTP/1.0\r\n"), "{upstream_req}");
        assert!(upstream_req.contains(&format!("Host: 127.0.0.1:{up_port}\r\n")), "{upstream_req}");
        assert!(upstream_req.ends_with("hello"), "{upstream_req}");
        manager.stop_proxy(proxy.port).await.unwrap();

        let cassette = load_cassette(&root, "test_api").unwrap();
        assert_eq!(cassette.interactions.len(), 1);
        let rec = &cassette.interactions[0];
        assert_eq!((rec.request_method.as_str(), rec.request_path.as_str()), ("POST", "/v1/users"));
        assert_eq!(rec.request_body, "hello");
        assert_eq!(rec.response_status, 201);
        assert_eq!(rec.response_body, r#"{"users":["ada"]}"#);

        // 2. Replay: answered from the cassette with no upstream involved.
        let replay_proxy = manager
            .start_proxy(root.clone(), "test_api".to_string(), VcrMode::Replay, None, None)
            .await
            .unwrap();
        let replayed = roundtrip(replay_proxy.port, req).await;
        assert!(replayed.contains("201 Created"), "{replayed}");
        assert!(replayed.contains(r#"{"users":["ada"]}"#), "{replayed}");
        assert!(seen.try_recv().is_err(), "replay must not touch the upstream");
        manager.stop_proxy(replay_proxy.port).await.unwrap();
    }

    #[tokio::test]
    async fn test_vcr_record_requires_target_and_reports_dead_upstream() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path());
        let manager = VcrProxyManager::new();
        assert!(manager
            .start_proxy(root.clone(), "x".into(), VcrMode::Record, None, None)
            .await
            .is_err());

        // A port nobody listens on: 502, and nothing is recorded.
        let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead_port = dead.local_addr().unwrap().port();
        drop(dead);
        let proxy = manager
            .start_proxy(root.clone(), "x".into(), VcrMode::Record, None, Some(format!("http://127.0.0.1:{dead_port}")))
            .await
            .unwrap();
        let resp = roundtrip(proxy.port, "GET / HTTP/1.1\r\nHost: a\r\n\r\n").await;
        assert!(resp.contains("502 Bad Gateway"), "{resp}");
        assert!(load_cassette(&root, "x").is_err());
    }

    #[test]
    fn vcr_upstream_parse_and_cassette_names() {
        assert_eq!(
            VcrUpstream::parse("http://localhost:8080/api/").unwrap(),
            VcrUpstream { host: "localhost".into(), port: 8080, prefix: "/api".into() }
        );
        assert_eq!(
            VcrUpstream::parse("http://example.com").unwrap(),
            VcrUpstream { host: "example.com".into(), port: 80, prefix: String::new() }
        );
        assert!(VcrUpstream::parse("https://example.com").is_err());
        assert!(VcrUpstream::parse("http://:80").is_err());
        assert!(VcrUpstream::parse("http://h:notaport").is_err());

        assert!(validate_cassette_name("api_v1.2-x").is_ok());
        for bad in ["", "../escape", "a/b", r"a\b", ".hidden"] {
            assert!(validate_cassette_name(bad).is_err(), "{bad:?} must be rejected");
        }
    }
}
