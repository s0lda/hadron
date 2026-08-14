//! In-process mock HTTP and WebSocket server for jailed agents.
//!
//! Provides isolated local mock servers bound exclusively to `127.0.0.1` for webhook testing,
//! API contract verification, and request journaling with assertion utilities.
//!
//! **Invariants:**
//! 1. Loopback only: Servers strictly bind to `127.0.0.1`. Binding to public interfaces is rejected.
//! 2. Request journaling: All incoming requests are captured in-memory for verification.
//! 3. Deterministic teardown: Stopping a mock server terminates the listener and drops open sockets.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, RwLock};

use crate::file::ForgeError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MockRoute {
    /// HTTP method: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, or `*` for any method.
    pub method: String,
    /// Path pattern or exact path to match (e.g. `/api/v1/webhook`).
    pub path: String,
    /// HTTP status code to return (default: 200).
    pub status: u16,
    /// Custom response headers.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Response body payload.
    #[serde(default)]
    pub body: String,
    /// Optional artificial response delay in milliseconds.
    #[serde(default)]
    pub delay_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedRequest {
    pub timestamp_ms: u64,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockServerSummary {
    pub port: u16,
    pub url: String,
    pub routes_count: usize,
    pub requests_count: usize,
    pub running: bool,
}

struct MockInstance {
    port: u16,
    routes: Arc<RwLock<Vec<MockRoute>>>,
    requests: Arc<RwLock<Vec<RecordedRequest>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Default)]
pub struct MockServerManager {
    servers: Arc<RwLock<HashMap<u16, Arc<RwLock<MockInstance>>>>>,
}

impl MockServerManager {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a new mock server instance on `127.0.0.1`.
    pub async fn start(&self, requested_port: Option<u16>) -> Result<MockServerSummary, ForgeError> {
        let port = requested_port.unwrap_or(0);
        let bind_addr = format!("127.0.0.1:{port}");

        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
            ForgeError::Io(format!("failed to bind mock server to {bind_addr}: {e}"))
        })?;

        let actual_port = listener.local_addr().map_err(|e| {
            ForgeError::Io(format!("failed to determine mock server port: {e}"))
        })?.port();

        let routes = Arc::new(RwLock::new(Vec::new()));
        let requests = Arc::new(RwLock::new(Vec::new()));
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let routes_clone = Arc::clone(&routes);
        let requests_clone = Arc::clone(&requests);

        // Background server loop
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    Ok((stream, _)) = listener.accept() => {
                        let r_clone = Arc::clone(&routes_clone);
                        let req_clone = Arc::clone(&requests_clone);
                        tokio::spawn(async move {
                            let _ = handle_client(stream, r_clone, req_clone).await;
                        });
                    }
                }
            }
        });

        let instance = Arc::new(RwLock::new(MockInstance {
            port: actual_port,
            routes,
            requests,
            shutdown_tx: Some(shutdown_tx),
        }));

        let mut map = self.servers.write().await;
        map.insert(actual_port, instance);

        Ok(MockServerSummary {
            port: actual_port,
            url: format!("http://127.0.0.1:{actual_port}"),
            routes_count: 0,
            requests_count: 0,
            running: true,
        })
    }

    /// Add a route rule to an active mock server.
    pub async fn add_route(&self, port: u16, route: MockRoute) -> Result<(), ForgeError> {
        let map = self.servers.read().await;
        let inst_lock = map.get(&port).ok_or(ForgeError::NotFound)?;
        let inst = inst_lock.read().await;
        let mut r = inst.routes.write().await;
        r.push(route);
        Ok(())
    }

    /// List recorded requests on a mock server.
    pub async fn list_requests(
        &self,
        port: u16,
        limit: Option<usize>,
    ) -> Result<Vec<RecordedRequest>, ForgeError> {
        let map = self.servers.read().await;
        let inst_lock = map.get(&port).ok_or(ForgeError::NotFound)?;
        let inst = inst_lock.read().await;
        let reqs = inst.requests.read().await;
        let total = reqs.len();
        let skip = if let Some(lim) = limit {
            total.saturating_sub(lim)
        } else {
            0
        };
        Ok(reqs[skip..].to_vec())
    }

    /// Assert that a request matching specific criteria arrived.
    pub async fn assert_request(
        &self,
        port: u16,
        method: Option<&str>,
        path_contains: Option<&str>,
        body_contains: Option<&str>,
    ) -> Result<bool, ForgeError> {
        let map = self.servers.read().await;
        let inst_lock = map.get(&port).ok_or(ForgeError::NotFound)?;
        let inst = inst_lock.read().await;
        let reqs = inst.requests.read().await;

        for req in reqs.iter() {
            if let Some(m) = method {
                if !req.method.eq_ignore_ascii_case(m) {
                    continue;
                }
            }
            if let Some(p) = path_contains {
                if !req.path.contains(p) {
                    continue;
                }
            }
            if let Some(b) = body_contains {
                if !req.body.contains(b) {
                    continue;
                }
            }
            return Ok(true);
        }

        Ok(false)
    }

    /// Stop an active mock server.
    pub async fn stop(&self, port: u16) -> Result<bool, ForgeError> {
        let mut map = self.servers.write().await;
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

    /// List all running mock servers.
    pub async fn list(&self) -> Vec<MockServerSummary> {
        let map = self.servers.read().await;
        let mut list = Vec::new();
        for inst_lock in map.values() {
            let inst = inst_lock.read().await;
            let routes_cnt = inst.routes.read().await.len();
            let reqs_cnt = inst.requests.read().await.len();
            list.push(MockServerSummary {
                port: inst.port,
                url: format!("http://127.0.0.1:{}", inst.port),
                routes_count: routes_cnt,
                requests_count: reqs_cnt,
                running: inst.shutdown_tx.is_some(),
            });
        }
        list.sort_by_key(|s| s.port);
        list
    }
}

async fn handle_client(
    mut stream: TcpStream,
    routes: Arc<RwLock<Vec<MockRoute>>>,
    requests: Arc<RwLock<Vec<RecordedRequest>>>,
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
    let full_path = parts.next().unwrap_or("/").to_string();

    let (path, query) = if let Some((p, q)) = full_path.split_once('?') {
        (p.to_string(), Some(q.to_string()))
    } else {
        (full_path.clone(), None)
    };

    let mut headers = HashMap::new();
    for line in lines.by_ref() {
        if line.is_empty() || line == "\r" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    // Extract body if any
    let body = if let Some(sep) = raw_req.find("\r\n\r\n") {
        raw_req[sep + 4..].to_string()
    } else if let Some(sep) = raw_req.find("\n\n") {
        raw_req[sep + 2..].to_string()
    } else {
        String::new()
    };

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    // Record request
    {
        let mut req_list = requests.write().await;
        req_list.push(RecordedRequest {
            timestamp_ms: now_ms,
            method: method.clone(),
            path: path.clone(),
            query,
            headers: headers.clone(),
            body: body.clone(),
        });
    }

    // Check matching route
    let matched = {
        let r_list = routes.read().await;
        r_list
            .iter()
            .find(|r| {
                (r.method == "*" || r.method.eq_ignore_ascii_case(&method))
                    && (r.path == "*" || r.path == path || path.starts_with(&r.path))
            })
            .cloned()
    };

    let (status, resp_headers, resp_body, delay_ms) = if let Some(r) = matched {
        (r.status, r.headers, r.body, r.delay_ms)
    } else {
        (
            200,
            HashMap::new(),
            r#"{"status":"ok","mock":true}"#.to_string(),
            None,
        )
    };

    if let Some(delay) = delay_ms {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

    let status_text = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    };

    let body_bytes = resp_body.as_bytes();
    let mut response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n",
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
    async fn mock_server_routes_requests_and_records_journal() {
        let manager = MockServerManager::new();

        let summary = manager.start(None).await.unwrap();
        assert!(summary.port > 0);

        // Add custom route
        let route = MockRoute {
            method: "POST".to_string(),
            path: "/webhook".to_string(),
            status: 201,
            headers: HashMap::new(),
            body: r#"{"accepted":true}"#.to_string(),
            delay_ms: None,
        };
        manager.add_route(summary.port, route).await.unwrap();

        // Send HTTP request to mock server via TcpStream
        let addr = format!("127.0.0.1:{}", summary.port);
        let mut client = TcpStream::connect(&addr).await.unwrap();

        let req = "POST /webhook HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"event\":\"test\"}";
        client.write_all(req.as_bytes()).await.unwrap();

        let mut resp_vec = Vec::new();
        client.read_to_end(&mut resp_vec).await.unwrap();
        let resp = String::from_utf8_lossy(&resp_vec);

        assert!(resp.contains("201 Created"));
        assert!(resp.contains(r#"{"accepted":true}"#));

        // Check journal
        let reqs = manager.list_requests(summary.port, None).await.unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].method, "POST");
        assert_eq!(reqs[0].path, "/webhook");
        assert!(reqs[0].body.contains(r#"{"event":"test"}"#));

        // Assert request
        assert!(manager
            .assert_request(
                summary.port,
                Some("POST"),
                Some("/webhook"),
                Some("event")
            )
            .await
            .unwrap());

        // Stop server
        assert!(manager.stop(summary.port).await.unwrap());
    }
}
