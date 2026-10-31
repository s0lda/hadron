//! Synchronous, bounded-timeout JSON-RPC 2.0 loopback client for `hadron-chamber`.
//!
//! Connects to the local `hadron-gluon` daemon advertised in `.hadron/gluon.rpc`
//! (`127.0.0.1:<port>`) to deliver `<1ms` wake notifications on user message submission
//! and optional incremental field polling without blocking the GPUI render thread.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use hadron_lattice::wire::{
    JsonRpcRequest, JsonRpcResponse, RpcEndpointMetadata, read_rpc_metadata,
};

fn resolve_rpc_dir(path: &Path) -> PathBuf {
    if path.file_name().is_some_and(|n| n == "field.jsonl") || path.is_file() {
        if let Some(parent) = path.parent() {
            if parent.join("gluon.rpc").is_file() {
                return parent.to_path_buf();
            }
        }
        hadron_lattice::hadron_dir_of(path)
    } else if path.join("gluon.rpc").is_file() {
        path.to_path_buf()
    } else if path.join(".hadron").is_dir() {
        path.join(".hadron")
    } else {
        path.to_path_buf()
    }
}

/// Read and validate the active `gluon.rpc` endpoint metadata for `field_dir`.
pub fn active_endpoint(field_dir: &Path) -> Option<RpcEndpointMetadata> {
    let dir = resolve_rpc_dir(field_dir);
    let meta = read_rpc_metadata(&dir)?;
    if meta.addr.parse::<SocketAddr>().is_err() {
        return None;
    }
    Some(meta)
}

/// Execute a single newline-delimited JSON-RPC 2.0 request over loopback TCP with a strict timeout.
pub fn call_rpc(
    addr: &str,
    method: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> anyhow::Result<JsonRpcResponse> {
    let sock_addr: SocketAddr = addr.parse()?;
    let mut stream = TcpStream::connect_timeout(&sock_addr, timeout)?;
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let req = JsonRpcRequest::new(method, params);
    let mut payload = serde_json::to_string(&req)?;
    payload.push('\n');
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        anyhow::bail!("empty JSON-RPC response from {addr}");
    }
    let resp: JsonRpcResponse = serde_json::from_str(line.trim())?;
    Ok(resp)
}

/// Immediately wake the `hadron-gluon` daemon loop over JSON-RPC (`"daemon/wake"`) if `gluon.rpc` is present.
/// Uses a tight 35ms timeout and returns `false` on any error so callers fall back to file watching seamlessly.
pub fn notify_daemon_wake(field_dir: &Path) -> bool {
    let Some(meta) = active_endpoint(field_dir) else {
        return false;
    };
    call_rpc(
        &meta.addr,
        "daemon/wake",
        serde_json::json!({}),
        Duration::from_millis(35),
    )
    .is_ok_and(|resp| resp.error.is_none())
}

/// Poll incremental `field.jsonl` events from `hadron-gluon` over `"field/poll"` JSON-RPC.
pub fn poll_field_via_rpc(
    field_dir: &Path,
    offset: u64,
    count: usize,
) -> Option<(Vec<hadron_lattice::Event>, u64)> {
    let meta = active_endpoint(field_dir)?;
    let resp = call_rpc(
        &meta.addr,
        "field/poll",
        serde_json::json!({
            "offset": offset,
            "count": count,
        }),
        Duration::from_millis(75),
    )
    .ok()?;
    if resp.error.is_some() {
        return None;
    }
    let result = resp.result?;
    let next_offset = result.get("next_offset")?.as_u64()?;
    let events_val = result.get("events")?.clone();
    let events: Vec<hadron_lattice::Event> = serde_json::from_value(events_val).ok()?;
    Some((events, next_offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::wire::{JsonRpcCodec, remove_rpc_metadata, write_rpc_metadata};
    use std::net::TcpListener;

    #[test]
    fn active_endpoint_returns_none_when_missing_and_some_when_valid() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(active_endpoint(tmp.path()).is_none());

        let meta = RpcEndpointMetadata {
            addr: "127.0.0.1:41234".to_string(),
            pid: 999,
            version: "0.33.27".to_string(),
            platform: "linux".to_string(),
        };
        write_rpc_metadata(tmp.path(), &meta).expect("write gluon.rpc");
        assert_eq!(active_endpoint(tmp.path()), Some(meta.clone()));
        assert_eq!(
            active_endpoint(&tmp.path().join("field.jsonl")),
            Some(meta)
        );

        remove_rpc_metadata(tmp.path()).expect("remove gluon.rpc");
        assert!(active_endpoint(tmp.path()).is_none());
    }

    #[test]
    fn notify_daemon_wake_and_poll_field_round_trip_over_loopback_tcp() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local_addr").to_string();

        let meta = RpcEndpointMetadata {
            addr: addr.clone(),
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: std::env::consts::OS.to_string(),
        };
        write_rpc_metadata(tmp.path(), &meta).expect("write gluon.rpc");

        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
                let mut line = String::new();
                reader.read_line(&mut line).expect("read request");
                let req = JsonRpcCodec::decode_request(line.trim()).expect("decode request");
                let resp = match req.method.as_str() {
                    "daemon/wake" => JsonRpcResponse::success(
                        &req.id,
                        serde_json::json!({ "woken": true }),
                    ),
                    "field/poll" => {
                        let ev = hadron_lattice::Event::new(
                            hadron_lattice::Actor::Human,
                            None,
                            hadron_lattice::Kind::Message {
                                body: "hello over rpc".to_string(),
                            },
                        );
                        JsonRpcResponse::success(
                            &req.id,
                            serde_json::json!({
                                "events": [ev],
                                "next_offset": 128u64,
                            }),
                        )
                    }
                    other => JsonRpcResponse::error(
                        &req.id,
                        -32601,
                        format!("unknown: {other}"),
                        None,
                    ),
                };
                let encoded = JsonRpcCodec::encode_response(&resp).expect("encode response");
                stream.write_all(encoded.as_bytes()).expect("write response");
                stream.flush().expect("flush");
            }
        });

        assert!(notify_daemon_wake(tmp.path()));
        let (events, next_offset) =
            poll_field_via_rpc(tmp.path(), 0, 0).expect("poll_field_via_rpc");
        assert_eq!(next_offset, 128);
        assert_eq!(events.len(), 1);

        server.join().expect("server thread");
        remove_rpc_metadata(tmp.path()).expect("remove gluon.rpc");
    }
}
