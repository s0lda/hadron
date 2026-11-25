use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum permitted byte length for a single newline-delimited JSON-RPC 2.0 frame (`16 MiB`).
pub const MAX_RPC_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Maximum permitted byte length for `.hadron/gluon.rpc` endpoint metadata (`4 KiB`).
pub const MAX_RPC_METADATA_BYTES: u64 = 4096;

/// Validated loopback-only TCP socket address (`127.0.0.0/8` or `::1`, non-zero port)
/// for `hadron-gluon` IPC, making remote or port-0 RPC endpoints unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopbackRpcAddr(std::net::SocketAddr);

impl LoopbackRpcAddr {
    pub fn parse(addr: &str) -> Result<Self, String> {
        let sock: std::net::SocketAddr = addr
            .parse()
            .map_err(|e| format!("invalid RPC socket address '{addr}': {e}"))?;
        if !sock.ip().is_loopback() {
            return Err(format!("refusing non-loopback RPC endpoint '{addr}'"));
        }
        if sock.port() == 0 {
            return Err(format!("refusing port-0 RPC endpoint '{addr}'"));
        }
        Ok(Self(sock))
    }

    pub fn socket_addr(self) -> std::net::SocketAddr {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcRequest {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: ulid::Ulid::new().to_string(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: &str, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.to_string(),
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: &str, code: i32, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }
}

pub const DAEMON_SUBSCRIBE_METHOD: &str = "daemon/subscribe";
pub const DAEMON_EVENT_METHOD: &str = "daemon/event";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RpcPushEvent {
    FieldAppended {
        next_offset: u64,
        count: usize,
    },
    LiveUpdated {
        quark: String,
    },
    GateUpdated {
        quark: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passed: Option<bool>,
    },
    Heartbeat {
        pid: u32,
    },
    /// One line of a quark's in-flight merge-gate test output, with the running
    /// pass/fail counts parsed so far (see [`parse_cargo_test_line`]).
    GateOutput {
        quark: String,
        stream: GateStream,
        line: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passed: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failed: Option<usize>,
    },
}

/// Which pipe of the gate's test process a [`RpcPushEvent::GateOutput`] line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateTestOutcome {
    Ok,
    Failed,
    Ignored,
}

/// A structured event recognised in one line of libtest (`cargo test`) output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GateTestEvent {
    Case { name: String, outcome: GateTestOutcome },
    Summary { passed: usize, failed: usize, ignored: usize },
}

/// The single parser for libtest lines: `test <name> ... ok|FAILED|ignored` and
/// `test result: <status>. <P> passed; <F> failed; <I> ignored; ...`. Anything else
/// (compiler output, panics, `running N tests`) is `None`.
pub fn parse_cargo_test_line(line: &str) -> Option<GateTestEvent> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("test result: ") {
        let (_, counts) = rest.split_once(". ")?;
        let count = |label: &str| {
            counts.split(';').find_map(|part| {
                let (n, l) = part.trim().split_once(' ')?;
                (l == label).then(|| n.parse::<usize>().ok()).flatten()
            })
        };
        return Some(GateTestEvent::Summary {
            passed: count("passed")?,
            failed: count("failed")?,
            ignored: count("ignored")?,
        });
    }
    let rest = line.strip_prefix("test ")?;
    let (name, verdict) = rest.rsplit_once(" ... ")?;
    let outcome = match verdict.split_whitespace().next()? {
        "ok" => GateTestOutcome::Ok,
        "FAILED" => GateTestOutcome::Failed,
        "ignored" | "ignored," => GateTestOutcome::Ignored,
        _ => return None,
    };
    let name = name.trim();
    (!name.is_empty()).then(|| GateTestEvent::Case { name: name.to_string(), outcome })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }

    pub fn daemon_event(event: &RpcPushEvent) -> Self {
        let params = serde_json::to_value(event).unwrap_or(Value::Null);
        Self::new(DAEMON_EVENT_METHOD, params)
    }

    pub fn push_event(&self) -> Option<RpcPushEvent> {
        if self.method != DAEMON_EVENT_METHOD {
            return None;
        }
        serde_json::from_value(self.params.clone()).ok()
    }
}

impl super::codec::JsonRpcCodec {
    pub fn encode_notification(notif: &JsonRpcNotification) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string(notif)?;
        s.push('\n');
        Ok(s)
    }

    pub fn decode_notification(line: &str) -> Result<JsonRpcNotification, serde_json::Error> {
        let trimmed = line.trim();
        if trimmed.len() > MAX_RPC_FRAME_BYTES {
            return Err(serde::de::Error::custom(format!(
                "JSON-RPC notification exceeds {MAX_RPC_FRAME_BYTES} byte limit"
            )));
        }
        let notif: JsonRpcNotification = serde_json::from_str(trimmed)?;
        if notif.jsonrpc != "2.0" {
            return Err(serde::de::Error::custom(format!(
                "unsupported jsonrpc version: {}",
                notif.jsonrpc
            )));
        }
        Ok(notif)
    }

    pub fn encode_push_event(event: &RpcPushEvent) -> Result<String, serde_json::Error> {
        Self::encode_notification(&JsonRpcNotification::daemon_event(event))
    }

    pub fn decode_push_event(line: &str) -> Result<RpcPushEvent, serde_json::Error> {
        let notif = Self::decode_notification(line)?;
        if notif.method != DAEMON_EVENT_METHOD {
            return Err(serde::de::Error::custom(format!(
                "unexpected notification method: {}",
                notif.method
            )));
        }
        serde_json::from_value(notif.params)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcEndpointMetadata {
    pub addr: String,
    pub pid: u32,
    pub version: String,
    pub platform: String,
}

pub fn rpc_metadata_path(field_dir: &std::path::Path) -> std::path::PathBuf {
    field_dir.join("gluon.rpc")
}

pub fn write_rpc_metadata(field_dir: &std::path::Path, meta: &RpcEndpointMetadata) -> std::io::Result<()> {
    let target = rpc_metadata_path(field_dir);
    let tmp = field_dir.join(".gluon.rpc.tmp");
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &target)?;
    Ok(())
}

pub fn read_rpc_metadata(field_dir: &std::path::Path) -> Option<RpcEndpointMetadata> {
    let target = rpc_metadata_path(field_dir);
    let file_meta = std::fs::metadata(&target).ok()?;
    if !file_meta.is_file() || file_meta.len() > MAX_RPC_METADATA_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(&target).ok()?;
    let meta: RpcEndpointMetadata = serde_json::from_str(&content).ok()?;
    LoopbackRpcAddr::parse(&meta.addr).ok()?;
    Some(meta)
}

pub fn remove_rpc_metadata(field_dir: &std::path::Path) -> std::io::Result<()> {
    let target = rpc_metadata_path(field_dir);
    match std::fs::remove_file(target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let tmp = field_dir.join(".gluon.rpc.tmp");
    match std::fs::remove_file(tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    use super::super::codec::JsonRpcCodec;

    #[test]
    fn push_event_encodes_and_decodes_as_daemon_event_notification() {
        let events = vec![
            RpcPushEvent::FieldAppended {
                next_offset: 4096,
                count: 3,
            },
            RpcPushEvent::LiveUpdated {
                quark: "acp-agy".to_string(),
            },
            RpcPushEvent::GateUpdated {
                quark: "cli-agy-2".to_string(),
                passed: Some(true),
            },
            RpcPushEvent::Heartbeat { pid: 9876 },
            RpcPushEvent::GateOutput {
                quark: "cli-agy-2".to_string(),
                stream: GateStream::Stdout,
                line: "test a::b ... ok".to_string(),
                passed: Some(1),
                failed: Some(0),
            },
            RpcPushEvent::GateOutput {
                quark: "cli-agy-2".to_string(),
                stream: GateStream::Stderr,
                line: "   Compiling hadron v0.33.54".to_string(),
                passed: None,
                failed: None,
            },
        ];

        for ev in events {
            let line = JsonRpcCodec::encode_push_event(&ev).expect("encode push event");
            assert!(line.ends_with('\n'));
            let notif = JsonRpcCodec::decode_notification(&line).expect("decode notification");
            assert_eq!(notif.jsonrpc, "2.0");
            assert_eq!(notif.method, DAEMON_EVENT_METHOD);
            assert_eq!(notif.push_event().as_ref(), Some(&ev));
            let roundtrip = JsonRpcCodec::decode_push_event(&line).expect("decode push event");
            assert_eq!(roundtrip, ev);
        }
    }

    #[test]
    fn gate_output_wire_shape_is_snake_case_and_omits_absent_counts() {
        let ev = RpcPushEvent::GateOutput {
            quark: "q".into(),
            stream: GateStream::Stderr,
            line: "x".into(),
            passed: None,
            failed: None,
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "gate_output", "quark": "q", "stream": "stderr", "line": "x"}));
    }

    #[test]
    fn parse_cargo_test_line_recognises_cases_and_summaries() {
        let case = |name: &str, outcome| Some(GateTestEvent::Case { name: name.into(), outcome });
        assert_eq!(parse_cargo_test_line("test a::b ... ok"), case("a::b", GateTestOutcome::Ok));
        assert_eq!(
            parse_cargo_test_line("test wire::t::x ... FAILED"),
            case("wire::t::x", GateTestOutcome::Failed)
        );
        assert_eq!(parse_cargo_test_line("test slow ... ignored"), case("slow", GateTestOutcome::Ignored));
        assert_eq!(
            parse_cargo_test_line("test slow ... ignored, needs network"),
            case("slow", GateTestOutcome::Ignored)
        );
        assert_eq!(
            parse_cargo_test_line(
                "test result: FAILED. 41 passed; 2 failed; 3 ignored; 0 measured; 0 filtered out; finished in 0.52s"
            ),
            Some(GateTestEvent::Summary { passed: 41, failed: 2, ignored: 3 })
        );
        assert_eq!(
            parse_cargo_test_line("test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured"),
            Some(GateTestEvent::Summary { passed: 7, failed: 0, ignored: 0 })
        );
        for noise in ["running 7 tests", "   Compiling hadron v1", "test result: garbage", "test  ... ok", ""] {
            assert_eq!(parse_cargo_test_line(noise), None, "{noise:?}");
        }
    }
}

