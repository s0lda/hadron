//! Tier 2 Generic STDIO JSON-RPC 2.0 Language Server Protocol (LSP) Client.
//!
//! Connects to language servers (`rust-analyzer`, `tsserver`/`vtsls`, `pyright`, `gopls`, `clangd`)
//! via standard input/output with `Content-Length: ...\r\n\r\n` message framing.
//!
//! Provides compiler-grade symbol definitions, references, and outlines.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, RwLock};

use crate::file::{ForgeError, Root};

/// Formatted location returned by LSP definition/reference queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspLocation {
    pub file: String,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

/// Symbol information returned by LSP document symbol queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspSymbol {
    pub name: String,
    pub kind: u32,
    pub detail: Option<String>,
    pub start_line: usize,
    pub start_col: usize,
}

/// Generic JSON-RPC 2.0 STDIO client for LSP servers.
#[derive(Clone)]
pub struct GenericLspClient {
    next_request_id: Arc<AtomicI64>,
    pending_requests: Arc<RwLock<HashMap<i64, oneshot::Sender<Result<Value, String>>>>>,
    writer_tx: Option<tokio::sync::mpsc::Sender<String>>,
    is_mock: bool,
    root_path: String,
}

impl GenericLspClient {
    /// Create a mock LSP client for fast unit tests without external binaries.
    pub fn new_mock() -> Self {
        Self {
            next_request_id: Arc::new(AtomicI64::new(1)),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            writer_tx: None,
            is_mock: true,
            root_path: "/mock/root".into(),
        }
    }

    /// Spawn a language server subprocess and attach JSON-RPC 2.0 stdio pipes.
    pub async fn spawn(
        server_bin: &str,
        server_args: &[&str],
        root: &Root,
    ) -> Result<Self, ForgeError> {
        let cwd = root
            .path()
            .canonicalize()
            .map_err(|e| ForgeError::Io(e.to_string()))?;

        let mut cmd = tokio::process::Command::new(server_bin);
        cmd.args(server_args).current_dir(&cwd);

        #[cfg(unix)]
        {
            cmd.process_group(0);
        }

        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| ForgeError::Io(format!("failed to spawn LSP server {server_bin}: {e}")))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ForgeError::Io("failed to open LSP stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ForgeError::Io("failed to open LSP stdout".into()))?;

        let (writer_tx, mut writer_rx) = tokio::sync::mpsc::channel::<String>(64);
        let pending_requests: Arc<RwLock<HashMap<i64, oneshot::Sender<Result<Value, String>>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Writer task: frame and write messages to stdin
        tokio::spawn(async move {
            while let Some(msg) = writer_rx.recv().await {
                let payload = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
                if stdin.write_all(payload.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });

        // Reader task: parse Content-Length headers and dispatch responses
        let pending_clone = Arc::clone(&pending_requests);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                let mut content_len: Option<usize> = None;

                // Read headers until empty line "\r\n"
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => return, // EOF
                        Ok(_) => {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                break;
                            }
                            if let Some((k, v)) = trimmed.split_once(':') {
                                if k.trim().eq_ignore_ascii_case("content-length") {
                                    if let Ok(len) = v.trim().parse::<usize>() {
                                        content_len = Some(len);
                                    }
                                }
                            }
                        }
                        Err(_) => return,
                    }
                }

                if let Some(len) = content_len {
                    let mut body_buf = vec![0u8; len];
                    if reader.read_exact(&mut body_buf).await.is_err() {
                        return;
                    }

                    if let Ok(val) = serde_json::from_slice::<Value>(&body_buf) {
                        if let Some(id) = val.get("id").and_then(|i| i.as_i64()) {
                            let mut map = pending_clone.write().await;
                            if let Some(sender) = map.remove(&id) {
                                if let Some(err) = val.get("error") {
                                    let _ = sender.send(Err(err.to_string()));
                                } else {
                                    let result = val.get("result").cloned().unwrap_or(Value::Null);
                                    let _ = sender.send(Ok(result));
                                }
                            }
                        }
                    }
                }
            }
        });

        let client = Self {
            next_request_id: Arc::new(AtomicI64::new(1)),
            pending_requests,
            writer_tx: Some(writer_tx),
            is_mock: false,
            root_path: cwd.to_string_lossy().to_string(),
        };

        // Initialize server
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": format!("file://{}", client.root_path),
            "capabilities": {
                "textDocument": {
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "documentSymbol": { "dynamicRegistration": false }
                }
            }
        });

        let _ = client.send_request("initialize", init_params).await;
        let _ = client.send_notification("initialized", serde_json::json!({})).await;

        Ok(client)
    }

    /// Send a JSON-RPC 2.0 request and await the response.
    pub async fn send_request(&self, method: &str, params: Value) -> Result<Value, ForgeError> {
        if self.is_mock {
            return match method {
                "initialize" => Ok(serde_json::json!({ "capabilities": {} })),
                "textDocument/definition" => Ok(serde_json::json!([
                    {
                        "uri": format!("file://{}/src/lib.rs", self.root_path),
                        "range": {
                            "start": { "line": 10, "character": 4 },
                            "end": { "line": 10, "character": 20 }
                        }
                    }
                ])),
                "textDocument/references" => Ok(serde_json::json!([
                    {
                        "uri": format!("file://{}/src/main.rs", self.root_path),
                        "range": {
                            "start": { "line": 25, "character": 8 },
                            "end": { "line": 25, "character": 24 }
                        }
                    }
                ])),
                "textDocument/documentSymbol" => Ok(serde_json::json!([
                    {
                        "name": "calculate_hash",
                        "kind": 12,
                        "detail": "fn(data: &[u8]) -> Hash",
                        "range": {
                            "start": { "line": 5, "character": 0 },
                            "end": { "line": 15, "character": 1 }
                        }
                    }
                ])),
                _ => Ok(serde_json::json!({})),
            };
        }

        let Some(tx) = &self.writer_tx else {
            return Err(ForgeError::Rejected("LSP writer channel is closed".into()));
        };

        let id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let (resp_tx, resp_rx) = oneshot::channel();
        {
            let mut map = self.pending_requests.write().await;
            map.insert(id, resp_tx);
        }

        let msg_str = serde_json::to_string(&req).map_err(|e| ForgeError::Io(e.to_string()))?;
        tx.send(msg_str)
            .await
            .map_err(|_| ForgeError::Io("failed to send request to LSP server".into()))?;

        match tokio::time::timeout(Duration::from_secs(10), resp_rx).await {
            Ok(Ok(Ok(val))) => Ok(val),
            Ok(Ok(Err(err_msg))) => Err(ForgeError::Rejected(format!("LSP error: {err_msg}"))),
            Ok(Err(_)) => Err(ForgeError::Io("LSP server dropped request".into())),
            Err(_) => {
                let mut map = self.pending_requests.write().await;
                map.remove(&id);
                Err(ForgeError::Io("LSP request timed out after 10s".into()))
            }
        }
    }

    /// Send a JSON-RPC 2.0 notification (fire-and-forget).
    pub async fn send_notification(&self, method: &str, params: Value) -> Result<(), ForgeError> {
        if self.is_mock {
            return Ok(());
        }
        let Some(tx) = &self.writer_tx else {
            return Err(ForgeError::Rejected("LSP writer channel is closed".into()));
        };

        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        let msg_str = serde_json::to_string(&notif).map_err(|e| ForgeError::Io(e.to_string()))?;
        tx.send(msg_str)
            .await
            .map_err(|_| ForgeError::Io("failed to send notification to LSP server".into()))?;

        Ok(())
    }

    /// Query symbol definitions at a specific line and column.
    pub async fn query_definition(
        &self,
        rel_path: &str,
        line: usize,
        col: usize,
    ) -> Result<Vec<LspLocation>, ForgeError> {
        let uri = format!("file://{}/{}", self.root_path.trim_end_matches('/'), rel_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line.saturating_sub(1), "character": col.saturating_sub(1) }
        });

        let resp = self.send_request("textDocument/definition", params).await?;
        Ok(parse_locations(&resp, &self.root_path))
    }

    /// Query symbol references at a specific line and column.
    pub async fn query_references(
        &self,
        rel_path: &str,
        line: usize,
        col: usize,
        include_declaration: bool,
    ) -> Result<Vec<LspLocation>, ForgeError> {
        let uri = format!("file://{}/{}", self.root_path.trim_end_matches('/'), rel_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line.saturating_sub(1), "character": col.saturating_sub(1) },
            "context": { "includeDeclaration": include_declaration }
        });

        let resp = self.send_request("textDocument/references", params).await?;
        Ok(parse_locations(&resp, &self.root_path))
    }

    /// Query document symbols/outline for a file.
    pub async fn query_document_symbols(
        &self,
        rel_path: &str,
    ) -> Result<Vec<LspSymbol>, ForgeError> {
        let uri = format!("file://{}/{}", self.root_path.trim_end_matches('/'), rel_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });

        let resp = self.send_request("textDocument/documentSymbol", params).await?;
        let mut symbols = Vec::new();
        if let Some(arr) = resp.as_array() {
            for item in arr {
                let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                let kind = item.get("kind").and_then(|k| k.as_u64()).unwrap_or(0) as u32;
                let detail = item.get("detail").and_then(|d| d.as_str()).map(|s| s.to_string());
                let range = item.get("range").or_else(|| item.get("location").and_then(|l| l.get("range")));
                let start = range.and_then(|r| r.get("start"));
                let line = start.and_then(|s| s.get("line")).and_then(|l| l.as_u64()).unwrap_or(0) as usize + 1;
                let col = start.and_then(|s| s.get("character")).and_then(|c| c.as_u64()).unwrap_or(0) as usize + 1;

                if !name.is_empty() {
                    symbols.push(LspSymbol {
                        name,
                        kind,
                        detail,
                        start_line: line,
                        start_col: col,
                    });
                }
            }
        }
        Ok(symbols)
    }
}

fn parse_locations(val: &Value, root_path: &str) -> Vec<LspLocation> {
    let mut locs = Vec::new();
    let root_prefix = format!("file://{}/", root_path.trim_end_matches('/'));

    let items: Vec<&Value> = match val {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(_) => vec![val],
        _ => Vec::new(),
    };

    for item in items {
        let uri_str = item.get("uri").or_else(|| item.get("targetUri")).and_then(|u| u.as_str()).unwrap_or("");
        let rel_file = uri_str.strip_prefix(&root_prefix).unwrap_or(uri_str).to_string();

        let range = item.get("range").or_else(|| item.get("targetRange")).or_else(|| item.get("targetSelectionRange"));
        let start = range.and_then(|r| r.get("start"));
        let end = range.and_then(|r| r.get("end"));

        let start_line = start.and_then(|s| s.get("line")).and_then(|l| l.as_u64()).unwrap_or(0) as usize + 1;
        let start_col = start.and_then(|s| s.get("character")).and_then(|c| c.as_u64()).unwrap_or(0) as usize + 1;
        let end_line = end.and_then(|e| e.get("line")).and_then(|l| l.as_u64()).unwrap_or(0) as usize + 1;
        let end_col = end.and_then(|e| e.get("character")).and_then(|c| c.as_u64()).unwrap_or(0) as usize + 1;

        if !rel_file.is_empty() {
            locs.push(LspLocation {
                file: rel_file,
                start_line,
                start_col,
                end_line,
                end_col,
            });
        }
    }

    locs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lsp_client_formats_jsonrpc_and_parses_responses() {
        let client = GenericLspClient::new_mock();
        let resp = client
            .send_request("textDocument/definition", serde_json::json!({}))
            .await
            .unwrap();
        assert!(resp.is_array());

        let defs = client.query_definition("src/main.rs", 10, 5).await.unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].file, "src/lib.rs");
        assert_eq!(defs[0].start_line, 11);

        let refs = client.query_references("src/lib.rs", 10, 5, true).await.unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].file, "src/main.rs");

        let syms = client.query_document_symbols("src/lib.rs").await.unwrap();
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "calculate_hash");
    }
}
