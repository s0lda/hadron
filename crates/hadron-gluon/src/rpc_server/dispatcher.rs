use std::path::PathBuf;
use tokio::sync::mpsc::Sender;
use crate::actor::bus::ActorBus;
use hadron_lattice::wire::{JsonRpcRequest, JsonRpcResponse};

pub struct RpcDispatcher {
    actor_bus: ActorBus,
    field_path: Option<PathBuf>,
    wake_tx: Option<Sender<()>>,
}

impl RpcDispatcher {
    pub fn new(actor_bus: ActorBus) -> Self {
        Self {
            actor_bus,
            field_path: None,
            wake_tx: None,
        }
    }

    pub fn with_field_and_wake(mut self, field_path: PathBuf, wake_tx: Sender<()>) -> Self {
        self.field_path = Some(field_path);
        self.wake_tx = Some(wake_tx);
        self
    }

    pub async fn dispatch(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "engine/status" => {
                let quarks = self.actor_bus.active_quarks().await;
                let event_count = self.field_path.as_ref()
                    .and_then(|p| hadron_lattice::io::read_events(p).ok())
                    .map(|e| e.len())
                    .unwrap_or(0);
                JsonRpcResponse::success(
                    &request.id,
                    serde_json::json!({
                        "status": "running",
                        "active_quarks": quarks,
                        "engine": "hadron-gluon-headless",
                        "version": env!("CARGO_PKG_VERSION"),
                        "pid": std::process::id(),
                        "event_count": event_count,
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
            "daemon/wake" => {
                if let Some(ref tx) = self.wake_tx {
                    let _ = tx.try_send(());
                }
                JsonRpcResponse::success(
                    &request.id,
                    serde_json::json!({
                        "woken": true,
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

                if let Some(ref tx) = self.wake_tx {
                    let _ = tx.try_send(());
                }

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
            "field/poll" => {
                let field = match &self.field_path {
                    Some(f) => f.as_path(),
                    None => return JsonRpcResponse::error(&request.id, -32000, "No field_path configured on dispatcher", None),
                };
                let offset = request.params.get("offset").and_then(|o| o.as_u64()).unwrap_or(0);
                let count = request.params.get("count").and_then(|c| c.as_u64()).map(|c| c as usize);

                match hadron_lattice::io::read_new(field, offset) {
                    Ok((mut events, next_offset)) => {
                        let total_count = hadron_lattice::io::read_events(field).map(|e| e.len()).unwrap_or(0);
                        if let Some(limit) = count {
                            if limit < events.len() {
                                events.truncate(limit);
                            }
                        }
                        JsonRpcResponse::success(
                            &request.id,
                            serde_json::json!({
                                "events": events,
                                "next_offset": next_offset,
                                "total_count": total_count,
                            }),
                        )
                    }
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

