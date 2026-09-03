use crate::actor::bus::ActorBus;
use hadron_lattice::wire::{JsonRpcRequest, JsonRpcResponse};

pub struct RpcDispatcher {
    actor_bus: ActorBus,
}

impl RpcDispatcher {
    pub fn new(actor_bus: ActorBus) -> Self {
        Self { actor_bus }
    }

    pub async fn dispatch(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "engine/status" => {
                let quarks = self.actor_bus.active_quarks().await;
                JsonRpcResponse::success(
                    &request.id,
                    serde_json::json!({
                        "status": "running",
                        "active_quarks": quarks,
                        "engine": "hadron-gluon-headless",
                        "version": env!("CARGO_PKG_VERSION"),
                    }),
                )
            }
            "swarm/roster/list" => {
                let quarks = self.actor_bus.active_quarks().await;
                JsonRpcResponse::success(
                    &request.id,
                    serde_json::json!({
                        "quarks": quarks,
                    }),
                )
            }
            "swarm/turn/dispatch" => {
                let quark = request.params.get("quark").and_then(|q| q.as_str()).unwrap_or("");
                let prompt = request.params.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
                if quark.is_empty() {
                    return JsonRpcResponse::error(&request.id, -32602, "Missing 'quark' param", None);
                }
                let assignment_id = ulid::Ulid::new().to_string();
                let send_res = self.actor_bus.send_to_quark(
                    quark,
                    crate::actor::QuarkMessage::TurnRequest {
                        assignment_id: assignment_id.clone(),
                        prompt: prompt.to_string(),
                    },
                ).await;

                match send_res {
                    Ok(_) => JsonRpcResponse::success(
                        &request.id,
                        serde_json::json!({
                            "dispatched": true,
                            "assignment_id": assignment_id,
                        }),
                    ),
                    Err(e) => JsonRpcResponse::error(&request.id, -32000, e.to_string(), None),
                }
            }
            _ => JsonRpcResponse::error(
                &request.id,
                -32601,
                format!("Method not found: {}", request.method),
                None,
            ),
        }
    }
}
