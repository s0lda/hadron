//! Synchronous Fast-Path Peer Micro-RPC Bus.
//!
//! Provides point-to-point synchronous query/response routing between active quarks,
//! enabling sub-second validation and coordination without full daemon turn handoffs.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcEnvelope {
    pub id: String,
    pub sender: String,
    pub recipient: String,
    pub method: String,
    pub payload: serde_json::Value,
}

type RpcHandler = Arc<dyn Fn(RpcEnvelope) -> Result<serde_json::Value, String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct MicroRpcBus {
    handlers: Arc<RwLock<HashMap<String, HashMap<String, RpcHandler>>>>,
}

impl MicroRpcBus {
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers an RPC method handler for a specific peer name.
    pub fn register_method<F>(&self, peer_name: &str, method: &str, handler: F)
    where
        F: Fn(RpcEnvelope) -> Result<serde_json::Value, String> + Send + Sync + 'static,
    {
        let mut map = self.handlers.write().unwrap();
        let peer_map = map.entry(peer_name.to_string()).or_default();
        peer_map.insert(method.to_string(), Arc::new(handler));
    }

    /// Invokes a synchronous micro-RPC call to a peer.
    pub fn call_peer(
        &self,
        sender: &str,
        recipient: &str,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let envelope = RpcEnvelope {
            id: ulid::Ulid::new().to_string(),
            sender: sender.to_string(),
            recipient: recipient.to_string(),
            method: method.to_string(),
            payload,
        };

        let handler = {
            let map = self.handlers.read().unwrap();
            let peer_map = map
                .get(recipient)
                .ok_or_else(|| format!("Recipient peer '{}' not found on RPC bus", recipient))?;
            peer_map
                .get(method)
                .cloned()
                .ok_or_else(|| format!("Method '{}' not implemented on peer '{}'", method, recipient))?
        };

        handler(envelope)
    }

    /// Unregisters all handlers for a peer.
    pub fn unregister_peer(&self, peer_name: &str) {
        let mut map = self.handlers.write().unwrap();
        map.remove(peer_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_micro_rpc_call_and_response() {
        let bus = MicroRpcBus::new();

        // Register peer @http-ollama with method 'validate_model'
        bus.register_method("http-ollama", "validate_model", |env| {
            let model_name = env.payload.get("model").and_then(|m| m.as_str()).unwrap_or("");
            if model_name == "qwen2.5-coder:32b" {
                Ok(serde_json::json!({ "status": "ready", "vram_mb": 19500 }))
            } else {
                Err("unknown model".into())
            }
        });

        // Call success
        let res = bus
            .call_peer(
                "agy",
                "http-ollama",
                "validate_model",
                serde_json::json!({ "model": "qwen2.5-coder:32b" }),
            )
            .unwrap();
        assert_eq!(res["status"], "ready");
        assert_eq!(res["vram_mb"], 19500);

        // Call failure (unknown model)
        let err = bus.call_peer(
            "agy",
            "http-ollama",
            "validate_model",
            serde_json::json!({ "model": "nonexistent" }),
        );
        assert!(err.is_err());

        // Call failure (unknown peer)
        let err2 = bus.call_peer("agy", "ghost", "ping", serde_json::json!({}));
        assert!(err2.unwrap_err().contains("not found on RPC bus"));
    }
}
