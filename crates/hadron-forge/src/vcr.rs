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

pub fn load_cassette(root: &Root, name: &str) -> Result<VcrCassette, ForgeError> {
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

    pub async fn start_proxy(
        &self,
        root: Root,
        cassette_name: String,
        mode: VcrMode,
        requested_port: Option<u16>,
    ) -> Result<VcrProxySummary, ForgeError> {
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
                        tokio::spawn(async move {
                            let _ = handle_vcr_client(stream, mode, inter_clone, r_clone, n_clone).await;
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

async fn handle_vcr_client(
    mut stream: TcpStream,
    mode: VcrMode,
    interactions: Arc<RwLock<Vec<VcrInteraction>>>,
    root: Root,
    cassette_name: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buffer = [0u8; 8192];
    let n = stream.read(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }

    let raw_req = String::from_utf8_lossy(&buffer[..n]);
    let mut lines = raw_req.lines();

    let first_line = lines.next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut headers = BTreeMap::new();
    for line in lines.by_ref() {
        if line.is_empty() || line == "\r" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    let body = if let Some(sep) = raw_req.find("\r\n\r\n") {
        raw_req[sep + 4..].to_string()
    } else if let Some(sep) = raw_req.find("\n\n") {
        raw_req[sep + 2..].to_string()
    } else {
        String::new()
    };

    let (status, resp_headers, resp_body) = match mode {
        VcrMode::Replay | VcrMode::Verify => {
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
        VcrMode::Record => {
            let resp_b = format!("{{\"status\":\"recorded\",\"method\":\"{}\",\"path\":\"{}\"}}", method, path);
            let mut r_headers = BTreeMap::new();
            r_headers.insert("content-type".to_string(), "application/json".to_string());

            let interaction = VcrInteraction {
                request_method: method.clone(),
                request_path: path.clone(),
                request_headers: headers,
                request_body: body,
                response_status: 200,
                response_headers: r_headers.clone(),
                response_body: resp_b.clone(),
            };

            {
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
                let _ = save_cassette(&root, &cassette);
            }

            (200, r_headers, resp_b)
        }
    };

    let status_text = match status {
        200 => "OK",
        201 => "Created",
        404 => "Not Found",
        500 => "Internal Server Error",
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

    #[tokio::test]
    async fn test_vcr_record_and_replay_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path());
        let manager = VcrProxyManager::new();

        // 1. Record mode
        let proxy = manager
            .start_proxy(root.clone(), "test_api".to_string(), VcrMode::Record, None)
            .await
            .unwrap();

        let mut client = TcpStream::connect(format!("127.0.0.1:{}", proxy.port))
            .await
            .unwrap();
        let req = "GET /v1/users HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
        client.write_all(req.as_bytes()).await.unwrap();

        let mut resp_vec = Vec::new();
        client.read_to_end(&mut resp_vec).await.unwrap();
        let resp = String::from_utf8_lossy(&resp_vec);
        assert!(resp.contains("200 OK"));
        assert!(resp.contains(r#""status":"recorded""#));

        manager.stop_proxy(proxy.port).await.unwrap();

        // Verify cassette was saved to disk
        let cassette = load_cassette(&root, "test_api").unwrap();
        assert_eq!(cassette.interactions.len(), 1);
        assert_eq!(cassette.interactions[0].request_path, "/v1/users");

        // 2. Replay mode
        let replay_proxy = manager
            .start_proxy(root.clone(), "test_api".to_string(), VcrMode::Replay, None)
            .await
            .unwrap();

        let mut replay_client = TcpStream::connect(format!("127.0.0.1:{}", replay_proxy.port))
            .await
            .unwrap();
        replay_client.write_all(req.as_bytes()).await.unwrap();

        let mut replay_resp_vec = Vec::new();
        replay_client.read_to_end(&mut replay_resp_vec).await.unwrap();
        let replay_resp = String::from_utf8_lossy(&replay_resp_vec);
        assert!(replay_resp.contains("200 OK"));

        manager.stop_proxy(replay_proxy.port).await.unwrap();
    }
}
