//! Headless Swarm MCP (`hadron-gluon --mcp`) stdio JSON-RPC 2.0 bridge.
//!
//! Exposes a running local `hadron-gluon` daemon's loopback RPC endpoints as an
//! MCP `2024-11-05` server over stdin/stdout so external editors, scripts, and
//! CI pipelines can inspect and drive Hadron headlessly with zero new crate
//! dependencies.
//!
//! Security:
//! - Connects strictly to loopback TCP addresses validated via [`LoopbackRpcAddr`].
//! - Bounds both stdio and loopback TCP NDJSON frames to [`MAX_RPC_FRAME_BYTES`].
//! - Applies a 5-second timeout to every daemon RPC round-trip.
//! - Rejects unknown tools (`-32601`) and malformed parameters (`-32602`).

use std::path::Path;
use std::time::Duration;

use hadron_lattice::wire::{
    read_rpc_metadata, JsonRpcCodec, JsonRpcRequest, LoopbackRpcAddr, MAX_RPC_FRAME_BYTES,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// MCP protocol version negotiated during `initialize`.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Hard timeout applied to every loopback TCP round-trip to `hadron-gluon`.
const RPC_TIMEOUT: Duration = Duration::from_secs(5);

/// Run the headless MCP bridge over process `stdin` / `stdout` targeting the
/// `hadron-gluon` daemon managing `field_path`.
pub async fn run_stdio(field_path: &Path) -> anyhow::Result<()> {
    let field_dir = hadron_lattice::hadron_dir_of(field_path);
    let stdin = BufReader::new(tokio::io::stdin());
    let stdout = tokio::io::stdout();
    run_io(&field_dir, stdin, stdout).await
}

/// Serve NDJSON MCP JSON-RPC 2.0 requests from `reader` and write responses to
/// `writer`, bridging tool calls to the loopback daemon registered in `field_dir`.
pub async fn run_io<R, W>(field_dir: &Path, mut reader: R, mut writer: W) -> anyhow::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut raw = Vec::new();
    loop {
        raw.clear();
        let n = (&mut reader)
            .take((MAX_RPC_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut raw)
            .await?;
        if n == 0 {
            break;
        }
        if raw.len() > MAX_RPC_FRAME_BYTES {
            let err = json_rpc_error(
                Value::Null,
                -32700,
                format!("Parse error: frame exceeds {MAX_RPC_FRAME_BYTES} byte limit"),
            );
            write_json_line(&mut writer, &err).await?;
            // Drain remainder of oversized line so framing stays aligned.
            if raw.last() != Some(&b'\n') {
                let mut discard = Vec::new();
                while reader.read_until(b'\n', &mut discard).await? > 0 {
                    let done = discard.last() == Some(&b'\n');
                    discard.clear();
                    if done {
                        break;
                    }
                }
            }
            continue;
        }
        let line = match std::str::from_utf8(&raw) {
            Ok(s) => s,
            Err(e) => {
                let err = json_rpc_error(Value::Null, -32700, format!("Parse error: invalid UTF-8: {e}"));
                write_json_line(&mut writer, &err).await?;
                continue;
            }
        };
        if let Some(resp) = handle_mcp_message(field_dir, line).await {
            write_json_line(&mut writer, &resp).await?;
        }
    }
    Ok(())
}

async fn write_json_line<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> anyhow::Result<()> {
    let mut encoded = serde_json::to_string(value)?;
    encoded.push('\n');
    writer.write_all(encoded.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

fn json_rpc_success(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn json_rpc_error(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        }
    })
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "hadron_swarm_status",
            "description": "Query the running hadron-gluon daemon status, active quarks, PID, version, and field event count.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "hadron_swarm_roster",
            "description": "List the active quark IDs currently registered on the running hadron-gluon swarm bus.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "hadron_swarm_dispatch",
            "description": "Dispatch a turn prompt to a named quark on the running hadron-gluon daemon.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "quark": {
                        "type": "string",
                        "description": "Target quark ID to excite."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Prompt text for the quark turn."
                    }
                },
                "required": ["quark", "prompt"],
                "additionalProperties": false
            }
        },
        {
            "name": "hadron_field_poll",
            "description": "Read incremental events from the active swarm field.jsonl starting at an optional byte offset.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Byte offset in field.jsonl to read from (default 0)."
                    },
                    "count": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional maximum number of events to return."
                    }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "hadron_daemon_wake",
            "description": "Immediately wake the running hadron-gluon daemon loop to process pending field events.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }
    ])
}

/// Process a single NDJSON MCP JSON-RPC 2.0 message line.
///
/// Returns `None` for blank lines and notifications (such as
/// `notifications/initialized`) that require no JSON-RPC response.
pub async fn handle_mcp_message(field_dir: &Path, line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() > MAX_RPC_FRAME_BYTES {
        return Some(json_rpc_error(
            Value::Null,
            -32700,
            format!("Parse error: frame exceeds {MAX_RPC_FRAME_BYTES} byte limit"),
        ));
    }

    let msg: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(e) => {
            return Some(json_rpc_error(
                Value::Null,
                -32700,
                format!("Parse error: {e}"),
            ));
        }
    };

    let Some(obj) = msg.as_object() else {
        return Some(json_rpc_error(
            Value::Null,
            -32600,
            "Invalid Request: expected JSON object",
        ));
    };

    let id = obj.get("id").cloned();

    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(json_rpc_error(
            id.unwrap_or(Value::Null),
            -32600,
            "Invalid Request: jsonrpc must be \"2.0\"",
        ));
    }

    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Some(json_rpc_error(
            id.unwrap_or(Value::Null),
            -32600,
            "Invalid Request: missing string 'method'",
        ));
    };

    // MCP notifications carry no `id` and expect no reply.
    if method == "notifications/initialized" || (id.is_none() && method.starts_with("notifications/")) {
        return None;
    }

    let req_id = id.unwrap_or(Value::Null);
    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => Some(json_rpc_success(
            req_id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "hadron-gluon",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )),
        "ping" => Some(json_rpc_success(req_id, json!({}))),
        "tools/list" => Some(json_rpc_success(
            req_id,
            json!({
                "tools": tool_definitions()
            }),
        )),
        "tools/call" => Some(handle_tools_call(field_dir, req_id, &params).await),
        other => Some(json_rpc_error(
            req_id,
            -32601,
            format!("Method not found: {other}"),
        )),
    }
}

async fn handle_tools_call(field_dir: &Path, req_id: Value, params: &Value) -> Value {
    let Some(params_obj) = params.as_object() else {
        return json_rpc_error(req_id, -32602, "Invalid params: expected object");
    };

    let Some(name) = params_obj.get("name").and_then(Value::as_str) else {
        return json_rpc_error(req_id, -32602, "Invalid params: missing string 'name'");
    };

    let empty_map = serde_json::Map::new();
    let args = match params_obj.get("arguments") {
        None | Some(Value::Null) => &empty_map,
        Some(Value::Object(map)) => map,
        Some(_) => {
            return json_rpc_error(
                req_id,
                -32602,
                "Invalid params: 'arguments' must be a JSON object",
            );
        }
    };

    let (rpc_method, rpc_params) = match name {
        "hadron_swarm_status" => ("engine/status", json!({})),
        "hadron_swarm_roster" => ("swarm/roster/list", json!({})),
        "hadron_daemon_wake" => ("daemon/wake", json!({})),
        "hadron_swarm_dispatch" => {
            let Some(quark) = args.get("quark").and_then(Value::as_str).map(str::trim) else {
                return json_rpc_error(
                    req_id,
                    -32602,
                    "Invalid params: 'quark' must be a non-empty string",
                );
            };
            if quark.is_empty() {
                return json_rpc_error(
                    req_id,
                    -32602,
                    "Invalid params: 'quark' must be a non-empty string",
                );
            }
            let Some(prompt) = args.get("prompt").and_then(Value::as_str) else {
                return json_rpc_error(
                    req_id,
                    -32602,
                    "Invalid params: 'prompt' must be a string",
                );
            };
            (
                "swarm/turn/dispatch",
                json!({
                    "quark": quark,
                    "prompt": prompt,
                }),
            )
        }
        "hadron_field_poll" => {
            let mut poll_params = serde_json::Map::new();
            if let Some(offset_val) = args.get("offset") {
                let Some(offset) = offset_val.as_u64() else {
                    return json_rpc_error(
                        req_id,
                        -32602,
                        "Invalid params: 'offset' must be a non-negative integer",
                    );
                };
                poll_params.insert("offset".to_string(), json!(offset));
            } else {
                poll_params.insert("offset".to_string(), json!(0u64));
            }
            if let Some(count_val) = args.get("count") {
                let Some(count) = count_val.as_u64() else {
                    return json_rpc_error(
                        req_id,
                        -32602,
                        "Invalid params: 'count' must be a positive integer",
                    );
                };
                poll_params.insert("count".to_string(), json!(count));
            }
            ("field/poll", Value::Object(poll_params))
        }
        other => {
            return json_rpc_error(req_id, -32601, format!("Unknown tool: {other}"));
        }
    };

    match call_daemon_rpc(field_dir, rpc_method, rpc_params).await {
        Ok(result) => {
            let text = serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| result.to_string());
            json_rpc_success(
                req_id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "isError": false
                }),
            )
        }
        Err(err) => json_rpc_success(
            req_id,
            json!({
                "content": [{ "type": "text", "text": err }],
                "isError": true
            }),
        ),
    }
}

/// Perform a single bounded JSON-RPC 2.0 call against the local `hadron-gluon`
/// daemon discovered via `<field_dir>/gluon.rpc`.
pub async fn call_daemon_rpc(
    field_dir: &Path,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let meta = read_rpc_metadata(field_dir).ok_or_else(|| {
        format!(
            "hadron-gluon RPC metadata not found or invalid in {}",
            field_dir.display()
        )
    })?;
    let endpoint = LoopbackRpcAddr::parse(&meta.addr)?;

    tokio::time::timeout(RPC_TIMEOUT, async move {
        let mut stream = tokio::net::TcpStream::connect(endpoint.socket_addr())
            .await
            .map_err(|e| format!("failed to connect to hadron-gluon at {}: {e}", meta.addr))?;
        let _ = stream.set_nodelay(true);

        let req = JsonRpcRequest::new(method, params);
        let mut req_line = serde_json::to_string(&req)
            .map_err(|e| format!("failed to encode RPC request: {e}"))?;
        req_line.push('\n');

        let (reader, mut writer) = stream.split();
        writer
            .write_all(req_line.as_bytes())
            .await
            .map_err(|e| format!("failed to send RPC request: {e}"))?;
        writer
            .flush()
            .await
            .map_err(|e| format!("failed to flush RPC request: {e}"))?;

        let mut buf_reader = BufReader::new(reader);
        let mut raw = Vec::new();
        let n = (&mut buf_reader)
            .take((MAX_RPC_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut raw)
            .await
            .map_err(|e| format!("failed to read RPC response: {e}"))?;
        if n == 0 {
            return Err("hadron-gluon closed RPC connection before responding".to_string());
        }
        if raw.len() > MAX_RPC_FRAME_BYTES || raw.last() != Some(&b'\n') {
            return Err(format!(
                "hadron-gluon RPC response exceeded {MAX_RPC_FRAME_BYTES} byte limit"
            ));
        }
        let line = std::str::from_utf8(&raw)
            .map_err(|e| format!("invalid UTF-8 in RPC response: {e}"))?;
        let resp = JsonRpcCodec::decode_response(line, Some(&req.id))
            .map_err(|e| format!("invalid RPC response: {e}"))?;

        if let Some(err) = resp.error {
            return Err(format!("daemon RPC error ({}): {}", err.code, err.message));
        }
        resp.result
            .ok_or_else(|| "daemon RPC response contained neither result nor error".to_string())
    })
    .await
    .map_err(|_| format!("RPC call '{method}' timed out after {}s", RPC_TIMEOUT.as_secs()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::{mpsc, watch};

    use crate::actor::bus::ActorBus;
    use crate::rpc_server::{RpcDispatcher, RpcServer};

    async fn spawn_test_rpc_server() -> (
        tempfile::TempDir,
        watch::Sender<bool>,
        tokio::task::JoinHandle<()>,
        mpsc::Receiver<()>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let field_dir = dir.path().to_path_buf();
        let field_path = field_dir.join("field.jsonl");
        let ev = hadron_lattice::Event::new(
            hadron_lattice::Actor::Human,
            None,
            hadron_lattice::Kind::Message {
                body: "hello mcp".to_string(),
            },
        );
        hadron_lattice::io::append_event(&field_path, &ev).unwrap();

        let bus = ActorBus::new(16);
        let (_mb, _rx) = bus.register_quark("orchestrator").await.unwrap();
        let (wake_tx, wake_rx) = mpsc::channel(4);
        let dispatcher = Arc::new(
            RpcDispatcher::new(bus).with_field_and_wake(field_path, wake_tx),
        );
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (_addr, handle) = RpcServer::bind_and_serve(&field_dir, dispatcher, shutdown_rx)
            .await
            .expect("bind_and_serve");
        (dir, shutdown_tx, handle, wake_rx)
    }

    #[tokio::test]
    async fn mcp_initialize_ping_and_tools_list() {
        let dir = tempfile::tempdir().unwrap();

        let init = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .await
        .expect("initialize response");
        assert_eq!(init["id"], 1);
        assert_eq!(init["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(init["result"]["serverInfo"]["name"], "hadron-gluon");

        // Notification yields no response.
        let notif = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        )
        .await;
        assert!(notif.is_none());

        let ping = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":"p1","method":"ping"}"#,
        )
        .await
        .expect("ping response");
        assert_eq!(ping["id"], "p1");
        assert_eq!(ping["result"], json!({}));

        let list = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        )
        .await
        .expect("tools/list response");
        let tools = list["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "hadron_swarm_status",
                "hadron_swarm_roster",
                "hadron_swarm_dispatch",
                "hadron_field_poll",
                "hadron_daemon_wake",
            ]
        );
    }

    #[tokio::test]
    async fn mcp_tools_call_bridges_to_live_loopback_rpc_server() {
        let (dir, shutdown_tx, handle, mut wake_rx) = spawn_test_rpc_server().await;

        // 1. hadron_swarm_status
        let status_resp = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"hadron_swarm_status","arguments":{}}}"#,
        )
        .await
        .unwrap();
        assert_eq!(status_resp["result"]["isError"], false);
        let status_payload: Value =
            serde_json::from_str(status_resp["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(status_payload["status"], "running");
        assert_eq!(status_payload["event_count"], 1);

        // 2. hadron_swarm_roster
        let roster_resp = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"hadron_swarm_roster"}}"#,
        )
        .await
        .unwrap();
        assert_eq!(roster_resp["result"]["isError"], false);
        let roster_payload: Value =
            serde_json::from_str(roster_resp["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(roster_payload["quarks"], json!(["orchestrator"]));

        // 3. hadron_field_poll
        let poll_resp = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"hadron_field_poll","arguments":{"offset":0,"count":5}}}"#,
        )
        .await
        .unwrap();
        assert_eq!(poll_resp["result"]["isError"], false);
        let poll_payload: Value =
            serde_json::from_str(poll_resp["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(poll_payload["total_count"], 1);
        assert_eq!(poll_payload["events"].as_array().unwrap().len(), 1);

        // 4. hadron_daemon_wake
        let wake_resp = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"hadron_daemon_wake","arguments":{}}}"#,
        )
        .await
        .unwrap();
        assert_eq!(wake_resp["result"]["isError"], false);
        assert!(wake_rx.try_recv().is_ok());

        // 5. hadron_swarm_dispatch (even if mailbox receiver dropped, daemon returns structured error -> isError: true)
        let dispatch_resp = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"hadron_swarm_dispatch","arguments":{"quark":"orchestrator","prompt":"run check"}}}"#,
        )
        .await
        .unwrap();
        assert!(dispatch_resp["result"]["content"][0]["text"].is_string());

        shutdown_tx.send(true).unwrap();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn mcp_rejects_unknown_tools_and_invalid_params() {
        let dir = tempfile::tempdir().unwrap();

        let unknown = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"no_such_tool","arguments":{}}}"#,
        )
        .await
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);

        let missing_quark = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"hadron_swarm_dispatch","arguments":{"quark":"  ","prompt":"hi"}}}"#,
        )
        .await
        .unwrap();
        assert_eq!(missing_quark["error"]["code"], -32602);

        let bad_offset = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"hadron_field_poll","arguments":{"offset":"not-a-number"}}}"#,
        )
        .await
        .unwrap();
        assert_eq!(bad_offset["error"]["code"], -32602);

        let bad_args = handle_mcp_message(
            dir.path(),
            r#"{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"hadron_swarm_status","arguments":[1,2]}}"#,
        )
        .await
        .unwrap();
        assert_eq!(bad_args["error"]["code"], -32602);

        let bad_json = handle_mcp_message(dir.path(), "{not valid json").await.unwrap();
        assert_eq!(bad_json["error"]["code"], -32700);
    }

    #[tokio::test]
    async fn run_io_roundtrips_stdio_frames() {
        let dir = tempfile::tempdir().unwrap();
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            "\n"
        );
        let mut output = Vec::new();
        run_io(dir.path(), BufReader::new(input.as_bytes()), &mut output)
            .await
            .unwrap();

        let text = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "notification must not emit a line: {text}");
        let r1: Value = serde_json::from_str(lines[0]).unwrap();
        let r2: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(r1["id"], 1);
        assert_eq!(r1["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(r2["id"], 2);
        assert_eq!(r2["result"], json!({}));
    }
}
