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

pub fn remove_rpc_metadata(field_dir: &std::path::Path) {
    let target = rpc_metadata_path(field_dir);
    let _ = std::fs::remove_file(target);
    let tmp = field_dir.join(".gluon.rpc.tmp");
    let _ = std::fs::remove_file(tmp);
}
