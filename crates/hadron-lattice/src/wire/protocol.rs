use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        serde_json::from_str(line.trim())
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
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    let content = std::fs::read_to_string(&target).ok()?;
    serde_json::from_str(&content).ok()
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
}

