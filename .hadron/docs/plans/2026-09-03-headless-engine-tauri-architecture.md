---
author: cli-agy
status: draft
---

# Hadron Next-Gen Architecture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use Swarm Quark Dispatch (recommended) or subagent-driven-development or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple Hadron into a headless Tokio actor daemon engine and a modern Tauri v2 React UI shell with a typed JSON-RPC wire protocol, CoW workspace isolation, and compiler cache protection.

**Architecture:** A phased, contract-driven architecture. Milestone 1 (M1) builds the typed JSON-RPC 2.0 wire protocol in `hadron-lattice` and the headless Tokio actor bus and RPC server in `hadron-gluon`. Milestone 2 (M2) introduces CoW workspace isolation and `sccache` compiler target isolation. Milestone 3 (M3) delivers the decoupled Tauri v2 desktop shell with React 19, `xterm.js`, and virtualized streaming chat.

**Tech Stack:** Rust 2021, `tokio` (v1), `serde`, `serde_json`, `ulid`, `async-trait`, `tokio-util`, `bytes`, `tauri` (v2), React 19, TypeScript, Tailwind CSS, `@tanstack/react-virtual`, `xterm`.

## Global Constraints

- Standard Model Rules 0–11 strictly enforced (SSOT, prove it runs, make invalid states unrepresentable, evidence over adjectives, no unverified claims).
- Process boundaries: The UI runs in an unprivileged webview without direct repository write access; all mutations route through typed JSON-RPC over Unix domain socket / WebSocket.
- Process group teardown: Every child process must spawn in `process_group(0)` and register with `hadron_gluon::proc` for clean exit teardown.
- Turn watchdog silence rule: Silence is tracked via actor heartbeat timestamps, never elapsed wall-clock execution time (`notes/the-turn-watchdog-measures-silence-not-elapsed-time.md`).
- Zero untrusted command leaks: Test runs, gate operations, and ACP runners are strictly bounded by timeouts (`GATE_TEST_DEADLINE`, `GIT_DEADLINE`).
- Centralized memory: Nucleus memory (`.hadron/nucleus/`) is owned and synced atomically by the daemon to prevent worktree doc stranding (`notes/a-hadron-doc-written-from-a-worktree-is-invisible.md`).
- No placeholders, TBDs, or missing test/code blocks in any step.

---

### Task 1: Typed JSON-RPC 2.0 Wire Protocol Envelopes & Codec in `hadron-lattice` - [x] (commit `a5639614`)

**Files:**
- Create: `crates/hadron-lattice/src/wire/protocol.rs`
- Create: `crates/hadron-lattice/src/wire/codec.rs`
- Create: `crates/hadron-lattice/src/wire/mod.rs`
- Modify: `crates/hadron-lattice/src/lib.rs`
- Test: `crates/hadron-lattice/src/wire/tests.rs`

**Interfaces:**
- Consumes: `serde::Serialize`, `serde::Deserialize`, `serde_json::Value`, `ulid::Ulid`, `tokio_util::codec::{Decoder, Encoder}`, `bytes::{Bytes, BytesMut}`
- Produces: `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcNotification`, `JsonRpcError`, `JsonRpcMessage`, `JsonRpcCodec`

- [x] **Step 1: Write failing unit test for wire protocol serialization and framing**

```rust
// crates/hadron-lattice/src/wire/tests.rs
#[cfg(test)]
mod tests {
    use super::super::protocol::*;
    use super::super::codec::*;
    use bytes::BytesMut;
    use tokio_util::codec::{Decoder, Encoder};

    #[test]
    fn test_jsonrpc_request_response_roundtrip() {
        let req = JsonRpcRequest::new("swarm/turn/dispatch", serde_json::json!({
            "quark": "http-ollama",
            "prompt": "Verify health"
        }));
        assert_eq!(req.jsonrpc, "2.0");
        assert!(!req.id.is_empty());

        let json_str = serde_json::to_string(&req).expect("serialize request");
        let parsed: JsonRpcRequest = serde_json::from_str(&json_str).expect("deserialize request");
        assert_eq!(parsed.method, "swarm/turn/dispatch");
        assert_eq!(parsed.id, req.id);

        let res = JsonRpcResponse::success(&req.id, serde_json::json!({"status": "dispatched"}));
        let res_str = serde_json::to_string(&res).expect("serialize response");
        let parsed_res: JsonRpcResponse = serde_json::from_str(&res_str).expect("deserialize response");
        assert_eq!(parsed_res.id, req.id);
        assert!(parsed_res.error.is_none());
        assert_eq!(parsed_res.result.unwrap()["status"], "dispatched");
    }

    #[test]
    fn test_jsonrpc_codec_framing() {
        let mut codec = JsonRpcCodec::new();
        let mut buffer = BytesMut::new();

        let notif = JsonRpcNotification::new("stream/field/event", serde_json::json!({
            "seq": 42,
            "text": "Streaming output"
        }));
        let msg = JsonRpcMessage::Notification(notif);

        codec.encode(msg.clone(), &mut buffer).expect("encode frame");
        assert!(buffer.ends_with(b"\n"));

        let decoded = codec.decode(&mut buffer).expect("decode frame").expect("must produce frame");
        match decoded {
            JsonRpcMessage::Notification(n) => {
                assert_eq!(n.method, "stream/field/event");
                assert_eq!(n.params["seq"], 42);
            }
            _ => panic!("expected notification frame"),
        }
    }
}
```

- [x] **Step 2: Verify test fails**
Run: `cargo test -p hadron-lattice --lib wire::tests`
Expected: FAIL with `cannot find module wire in hadron_lattice`

- [x] **Step 3: Implement minimal code for wire protocol and codec**

```rust
// crates/hadron-lattice/src/wire/protocol.rs
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
```

```rust
// crates/hadron-lattice/src/wire/codec.rs
use bytes::{Buf, BytesMut};
use std::io;
use tokio_util::codec::{Decoder, Encoder};
use super::protocol::JsonRpcMessage;

#[derive(Debug, Default)]
pub struct JsonRpcCodec;

impl JsonRpcCodec {
    pub fn new() -> Self {
        Self
    }
}

impl Decoder for JsonRpcCodec {
    type Item = JsonRpcMessage;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(i) = src.iter().position(|&b| b == b'\n') {
            let line = src.split_to(i);
            src.advance(1); // strip '\n'
            if line.is_empty() {
                return Ok(None);
            }
            let msg: JsonRpcMessage = serde_json::from_slice(&line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(msg))
        } else {
            Ok(None)
        }
    }
}

impl Encoder<JsonRpcMessage> for JsonRpcCodec {
    type Error = io::Error;

    fn encode(&mut self, item: JsonRpcMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let json = serde_json::to_vec(&item)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        dst.extend_from_slice(&json);
        dst.extend_from_slice(b"\n");
        Ok(())
    }
}
```

```rust
// crates/hadron-lattice/src/wire/mod.rs
pub mod protocol;
pub mod codec;
#[cfg(test)]
mod tests;

pub use codec::JsonRpcCodec;
pub use protocol::*;
```

Modify `crates/hadron-lattice/src/lib.rs` to add:
```rust
pub mod wire;
pub use wire::*;
```

- [x] **Step 4: Verify test passes**
Run: `cargo test -p hadron-lattice --lib wire::tests`
Expected: `test result: ok. 2 passed; 0 failed; 0 ignored`

- [x] **Step 5: Commit**
```bash
git add crates/hadron-lattice/src/wire/ crates/hadron-lattice/src/lib.rs
git commit -m "feat(lattice): implement typed JSON-RPC 2.0 protocol envelopes and stream codec"
```

---

### Task 2: Tokio Actor Bus & Mailbox Routing Engine in `hadron-gluon` - [x] (commit `3275c7a1`)

**Files:**
- Create: `crates/hadron-gluon/src/actor/bus.rs`
- Create: `crates/hadron-gluon/src/actor/mailbox.rs`
- Create: `crates/hadron-gluon/src/actor/mod.rs`
- Modify: `crates/hadron-gluon/src/lib.rs`
- Test: `crates/hadron-gluon/src/actor/tests.rs`

**Interfaces:**
- Consumes: `hadron_lattice::wire::*`, `hadron_lattice::QuarkId`, `tokio::sync::mpsc`, `tokio::sync::broadcast`
- Produces: `ActorBus`, `ActorMailbox`, `QuarkMessage`, `ActorEnvelope`, `ActorDispatchResult`

- [x] **Step 1: Write failing test for Actor Mailbox and Concurrent Swarm Bus**

```rust
// crates/hadron-gluon/src/actor/tests.rs
#[cfg(test)]
mod tests {
    use super::bus::*;
    use super::mailbox::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_actor_bus_registration_and_message_delivery() {
        let bus = ActorBus::new(32);
        let (handle, mut rx) = bus.register_quark("http-ollama").await.expect("register quark");

        assert_eq!(bus.active_quarks().await, vec!["http-ollama"]);

        let msg = QuarkMessage::TurnRequest {
            assignment_id: "01HZX0001".into(),
            prompt: "Test prompt".into(),
        };

        bus.send_to_quark("http-ollama", msg.clone()).await.expect("send message");

        let received = timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout waiting for msg")
            .expect("channel active");

        match received {
            QuarkMessage::TurnRequest { assignment_id, prompt } => {
                assert_eq!(assignment_id, "01HZX0001");
                assert_eq!(prompt, "Test prompt");
            }
            _ => panic!("unexpected message variant"),
        }

        // Test broadcast
        let mut broadcast_rx = bus.subscribe_events();
        bus.broadcast_event(SwarmEvent::QuarkStatusChanged {
            quark: "http-ollama".into(),
            state: "Thinking".into(),
        }).await.expect("broadcast");

        let event = timeout(Duration::from_millis(500), broadcast_rx.recv())
            .await
            .expect("timeout waiting for event")
            .expect("broadcast active");

        match event {
            SwarmEvent::QuarkStatusChanged { quark, state } => {
                assert_eq!(quark, "http-ollama");
                assert_eq!(state, "Thinking");
            }
        }
    }
}
```

- [x] **Step 2: Verify test fails**
Run: `cargo test -p hadron-gluon --lib actor::tests`
Expected: FAIL with `cannot find module actor in hadron_gluon`

- [x] **Step 3: Implement minimal code for ActorBus and Mailbox**

```rust
// crates/hadron-gluon/src/actor/mailbox.rs
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuarkMessage {
    TurnRequest {
        assignment_id: String,
        prompt: String,
    },
    CancelTurn {
        assignment_id: String,
    },
    Ping {
        timestamp_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwarmEvent {
    QuarkStatusChanged {
        quark: String,
        state: String,
    },
    FieldAppended {
        sequence: u64,
        author: String,
        summary: String,
    },
    TurnCompleted {
        quark: String,
        assignment_id: String,
        success: bool,
    },
}

pub struct ActorMailbox {
    pub quark_id: String,
    pub sender: mpsc::Sender<QuarkMessage>,
}
```

```rust
// crates/hadron-gluon/src/actor/bus.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use super::mailbox::{ActorMailbox, QuarkMessage, SwarmEvent};

#[derive(Clone)]
pub struct ActorBus {
    mailboxes: Arc<RwLock<HashMap<String, mpsc::Sender<QuarkMessage>>>>,
    event_tx: broadcast::Sender<SwarmEvent>,
    capacity: usize,
}

impl ActorBus {
    pub fn new(capacity: usize) -> Self {
        let (event_tx, _) = broadcast::channel(capacity * 4);
        Self {
            mailboxes: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            capacity,
        }
    }

    pub async fn register_quark(
        &self,
        quark_id: &str,
    ) -> anyhow::Result<(ActorMailbox, mpsc::Receiver<QuarkMessage>)> {
        let (tx, rx) = mpsc::channel(self.capacity);
        let mut map = self.mailboxes.write().await;
        map.insert(quark_id.to_string(), tx.clone());
        Ok((
            ActorMailbox {
                quark_id: quark_id.to_string(),
                sender: tx,
            },
            rx,
        ))
    }

    pub async fn unregister_quark(&self, quark_id: &str) {
        let mut map = self.mailboxes.write().await;
        map.remove(quark_id);
    }

    pub async fn active_quarks(&self) -> Vec<String> {
        let map = self.mailboxes.read().await;
        let mut quarks: Vec<String> = map.keys().cloned().collect();
        quarks.sort();
        quarks
    }

    pub async fn send_to_quark(&self, quark_id: &str, msg: QuarkMessage) -> anyhow::Result<()> {
        let sender = {
            let map = self.mailboxes.read().await;
            map.get(quark_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Quark '{quark_id}' not found on ActorBus"))?
        };
        sender.send(msg).await.map_err(|e| anyhow::anyhow!("Failed to send to quark: {e}"))?;
        Ok(())
    }

    pub async fn broadcast_event(&self, event: SwarmEvent) -> anyhow::Result<()> {
        let _ = self.event_tx.send(event);
        Ok(())
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<SwarmEvent> {
        self.event_tx.subscribe()
    }
}
```

```rust
// crates/hadron-gluon/src/actor/mod.rs
pub mod bus;
pub mod mailbox;
#[cfg(test)]
mod tests;

pub use bus::ActorBus;
pub use mailbox::{ActorMailbox, QuarkMessage, SwarmEvent};
```

Modify `crates/hadron-gluon/src/lib.rs` to add:
```rust
pub mod actor;
pub use actor::*;
```

- [x] **Step 4: Verify test passes**
Run: `cargo test -p hadron-gluon --lib actor::tests`
Expected: `test result: ok. 1 passed; 0 failed; 0 ignored`

- [x] **Step 5: Commit**
```bash
git add crates/hadron-gluon/src/actor/ crates/hadron-gluon/src/lib.rs
git commit -m "feat(gluon): implement Tokio actor bus and concurrent mailbox routing"
```

---

### Task 3: Headless Daemon JSON-RPC Server & Subscription Engine in `hadron-gluon` - [x] (commit `3855e95f`)

**Files:**
- Create: `crates/hadron-gluon/src/rpc_server/mod.rs`
- Create: `crates/hadron-gluon/src/rpc_server/dispatcher.rs`
- Modify: `crates/hadron-gluon/src/lib.rs`
- Test: `crates/hadron-gluon/src/rpc_server/tests.rs`

**Interfaces:**
- Consumes: `hadron_lattice::wire::*`, `ActorBus`, `serde_json::Value`
- Produces: `RpcServer`, `RpcDispatcher`, `handle_rpc_frame`

- [x] **Step 1: Write failing integration test for RPC Dispatcher**

```rust
// crates/hadron-gluon/src/rpc_server/tests.rs
#[cfg(test)]
mod tests {
    use super::dispatcher::*;
    use crate::actor::bus::ActorBus;
    use hadron_lattice::wire::*;

    #[tokio::test]
    async fn test_rpc_dispatcher_engine_status_and_roster() {
        let bus = ActorBus::new(16);
        bus.register_quark("orchestrator").await.unwrap();
        bus.register_quark("agy").await.unwrap();

        let dispatcher = RpcDispatcher::new(bus);

        // Call engine/status
        let req = JsonRpcRequest::new("engine/status", serde_json::json!({}));
        let res = dispatcher.dispatch(req).await;
        assert!(res.error.is_none());
        let result = res.result.expect("status result");
        assert_eq!(result["status"], "running");
        assert_eq!(result["active_quarks"].as_array().unwrap().len(), 2);

        // Call unknown method
        let bad_req = JsonRpcRequest::new("invalid/method", serde_json::json!({}));
        let bad_res = dispatcher.dispatch(bad_req).await;
        assert!(bad_res.error.is_some());
        assert_eq!(bad_res.error.unwrap().code, -32601); // Method not found
    }
}
```

- [x] **Step 2: Verify test fails**
Run: `cargo test -p hadron-gluon --lib rpc_server::tests`
Expected: FAIL with `cannot find module rpc_server in hadron_gluon`

- [x] **Step 3: Implement minimal code for RpcDispatcher**

```rust
// crates/hadron-gluon/src/rpc_server/dispatcher.rs
use crate::actor::bus::ActorBus;
use hadron_lattice::wire::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;

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
```

```rust
// crates/hadron-gluon/src/rpc_server/mod.rs
pub mod dispatcher;
#[cfg(test)]
mod tests;

pub use dispatcher::RpcDispatcher;
```

Modify `crates/hadron-gluon/src/lib.rs` to add:
```rust
pub mod rpc_server;
pub use rpc_server::*;
```

- [x] **Step 4: Verify test passes**
Run: `cargo test -p hadron-gluon --lib rpc_server::tests`
Expected: `test result: ok. 1 passed; 0 failed; 0 ignored`

- [x] **Step 5: Commit**
```bash
git add crates/hadron-gluon/src/rpc_server/ crates/hadron-gluon/src/lib.rs
git commit -m "feat(gluon): implement headless JSON-RPC dispatcher and method router"
```

---

### Task 4: Centralized Nucleus Memory Store in `hadron-gluon` - [x] (commit `b518e857`)

**Files:**
- Create: `crates/hadron-gluon/src/nucleus_store.rs`
- Modify: `crates/hadron-gluon/src/lib.rs`
- Test: `crates/hadron-gluon/tests/nucleus_store_tests.rs`

**Interfaces:**
- Consumes: `std::path::Path`, `hadron_lattice::nucleus::*`
- Produces: `NucleusStore`, `LessonNote`, `sync_worktree_note`, `query_memory_index`

- [x] **Step 1: Write failing test for Nucleus Central Store**

```rust
// crates/hadron-gluon/tests/nucleus_store_tests.rs
use hadron_gluon::nucleus_store::NucleusStore;
use tempfile::tempdir;

#[tokio::test]
async fn test_nucleus_central_store_note_and_index_lifecycle() {
    let dir = tempdir().expect("tempdir");
    let nucleus_dir = dir.path().join(".hadron/nucleus");
    std::fs::create_dir_all(nucleus_dir.join("notes")).expect("create notes dir");

    let store = NucleusStore::new(&nucleus_dir);

    // Write note
    store.write_note(
        "test-lesson",
        "A lesson learned in a worktree",
        "Testing non-obvious invariant",
        "How to apply in practice",
    ).await.expect("write note");

    assert!(nucleus_dir.join("notes/test-lesson.md").is_file());
    assert!(nucleus_dir.join("index.md").is_file());

    let index_content = std::fs::read_to_string(nucleus_dir.join("index.md")).expect("read index");
    assert!(index_content.contains("- [test-lesson](notes/test-lesson.md)"));

    let loaded = store.read_note("test-lesson").await.expect("read note");
    assert!(loaded.contains("A lesson learned in a worktree"));
}
```

- [x] **Step 2: Verify test fails**
Run: `cargo test -p hadron-gluon --test nucleus_store_tests`
Expected: FAIL with `cannot find module nucleus_store in hadron_gluon`

- [x] **Step 3: Implement minimal code for NucleusStore**

```rust
// crates/hadron-gluon/src/nucleus_store.rs
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::sync::RwLock;
use std::sync::Arc;

#[derive(Clone)]
pub struct NucleusStore {
    root_dir: PathBuf,
    lock: Arc<RwLock<()>>,
}

impl NucleusStore {
    pub fn new(root_dir: &Path) -> Self {
        Self {
            root_dir: root_dir.to_path_buf(),
            lock: Arc::new(RwLock::new(())),
        }
    }

    pub async fn write_note(
        &self,
        slug: &str,
        fact: &str,
        why: &str,
        how_to_apply: &str,
    ) -> anyhow::Result<()> {
        let _guard = self.lock.write().await;
        let notes_dir = self.root_dir.join("notes");
        fs::create_dir_all(&notes_dir).await?;

        let note_path = notes_dir.join(format!("{slug}.md"));
        let note_body = format!(
            "---\nname: {slug}\ndescription: {why}\nmetadata:\n  type: project\n---\n\n{fact}\n\n**Why:** {why}\n\n**How to apply:** {how_to_apply}\n"
        );
        fs::write(&note_path, note_body).await?;

        // Update index.md
        let index_path = self.root_dir.join("index.md");
        let mut index = if index_path.exists() {
            fs::read_to_string(&index_path).await?
        } else {
            "# Memory index\n\n## Project Lessons\n\n".to_string()
        };

        let pointer = format!("- [{slug}](notes/{slug}.md) — {why}\n");
        if !index.contains(&format!("[{slug}]")) {
            index.push_str(&pointer);
            fs::write(&index_path, index).await?;
        }

        Ok(())
    }

    pub async fn read_note(&self, slug: &str) -> anyhow::Result<String> {
        let note_path = self.root_dir.join("notes").join(format!("{slug}.md"));
        let content = fs::read_to_string(note_path).await?;
        Ok(content)
    }
}
```

Modify `crates/hadron-gluon/src/lib.rs` to add:
```rust
pub mod nucleus_store;
pub use nucleus_store::*;
```

- [x] **Step 4: Verify test passes**
Run: `cargo test -p hadron-gluon --test nucleus_store_tests`
Expected: `test result: ok. 1 passed; 0 failed; 0 ignored`

- [x] **Step 5: Commit**
```bash
git add crates/hadron-gluon/src/nucleus_store.rs crates/hadron-gluon/src/lib.rs crates/hadron-gluon/tests/nucleus_store_tests.rs
git commit -m "feat(gluon): implement centralized daemon nucleus memory store"
```

---

### Task 5: Milestone 2 Scaffolding — CoW Workspaces & Compiler Cache Guard in `hadron-gluon` - [x] (commit `2663bd8d`)

**Files:**
- Create: `crates/hadron-gluon/src/worktree/cow.rs`
- Create: `crates/hadron-gluon/src/worktree/sccache_guard.rs`
- Modify: `crates/hadron-gluon/src/worktree.rs`
- Test: `crates/hadron-gluon/tests/cow_worktree_tests.rs`

**Interfaces:**
- Consumes: `std::path::Path`, `hadron_lattice::QuarkId`
- Produces: `CowWorkspace`, `SccacheGuard`, `prepare_quark_env`

- [x] **Step 1: Write failing test for CoW workspace preparation and sccache environment config**

```rust
// crates/hadron-gluon/tests/cow_worktree_tests.rs
use hadron_gluon::worktree::cow::CowWorkspace;
use hadron_gluon::worktree::sccache_guard::SccacheGuard;
use tempfile::tempdir;

#[test]
fn test_sccache_env_generation_and_target_isolation() {
    let dir = tempdir().expect("tempdir");
    let guard = SccacheGuard::new(dir.path());
    let env_vars = guard.build_env_for_quark("http-ollama");

    assert_eq!(env_vars.get("RUSTC_WRAPPER"), Some(&"sccache".to_string()));
    let target_dir = env_vars.get("CARGO_TARGET_DIR").expect("must set target dir");
    assert!(target_dir.contains("quarks/http-ollama"));
}
```

- [x] **Step 2: Verify test fails**
Run: `cargo test -p hadron-gluon --test cow_worktree_tests`
Expected: FAIL with `cannot find module cow in worktree`

- [x] **Step 3: Implement minimal code for CowWorkspace and SccacheGuard**

```rust
// crates/hadron-gluon/src/worktree/sccache_guard.rs
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct SccacheGuard {
    base_dir: PathBuf,
}

impl SccacheGuard {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    pub fn build_env_for_quark(&self, quark_id: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("RUSTC_WRAPPER".to_string(), "sccache".to_string());
        let target_dir = self.base_dir.join("target/quarks").join(quark_id);
        map.insert("CARGO_TARGET_DIR".to_string(), target_dir.to_string_lossy().to_string());
        map
    }
}
```

```rust
// crates/hadron-gluon/src/worktree/cow.rs
use std::path::{Path, PathBuf};

pub struct CowWorkspace {
    pub quark_id: String,
    pub path: PathBuf,
}

impl CowWorkspace {
    pub fn create(repo_root: &Path, quark_id: &str) -> anyhow::Result<Self> {
        let path = repo_root.join(".hadron/trees").join(quark_id);
        std::fs::create_dir_all(&path)?;
        Ok(Self {
            quark_id: quark_id.to_string(),
            path,
        })
    }
}
```

Modify `crates/hadron-gluon/src/worktree.rs` to expose `cow` and `sccache_guard`.

- [x] **Step 4: Verify test passes**
Run: `cargo test -p hadron-gluon --test cow_worktree_tests`
Expected: `test result: ok. 1 passed; 0 failed; 0 ignored`

- [x] **Step 5: Commit**
```bash
git add crates/hadron-gluon/src/worktree/cow.rs crates/hadron-gluon/src/worktree/sccache_guard.rs crates/hadron-gluon/src/worktree.rs crates/hadron-gluon/tests/cow_worktree_tests.rs
git commit -m "feat(gluon): scaffold CoW workspace and sccache compiler target isolation"
```

---

### Task 6: Milestone 3 Scaffolding — Tauri v2 Core Crate & React Shell Bridge

**Files:**
- Create: `crates/hadron-tauri/Cargo.toml`
- Create: `crates/hadron-tauri/src/lib.rs`
- Create: `crates/hadron-tauri/src/client.rs`
- Modify: Root `Cargo.toml` (add `crates/hadron-tauri` to workspace members)
- Test: `crates/hadron-tauri/tests/client_tests.rs`

**Interfaces:**
- Consumes: `hadron_lattice::wire::*`
- Produces: `HadronDaemonClient`, `connect_ipc`

- [ ] **Step 1: Write failing test for Tauri Daemon Client**

```rust
// crates/hadron-tauri/tests/client_tests.rs
use hadron_tauri::client::HadronDaemonClient;
use hadron_lattice::wire::JsonRpcRequest;

#[tokio::test]
async fn test_tauri_daemon_client_request_construction() {
    let client = HadronDaemonClient::new("ipc:///tmp/hadron.sock");
    let req = client.build_request("engine/status", serde_json::json!({}));
    assert_eq!(req.method, "engine/status");
    assert_eq!(req.jsonrpc, "2.0");
}
```

- [ ] **Step 2: Verify test fails**
Run: `cargo test -p hadron-tauri --test client_tests`
Expected: FAIL with `could not find package hadron-tauri`

- [ ] **Step 3: Implement minimal code for hadron-tauri crate**

```toml
# crates/hadron-tauri/Cargo.toml
[package]
name = "hadron-tauri"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Tauri v2 Desktop Shell Bridge for Hadron Swarm"

[dependencies]
hadron-lattice = { path = "../hadron-lattice" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"

[dev-dependencies]
tempfile = "3.10"
```

```rust
// crates/hadron-tauri/src/client.rs
use hadron_lattice::wire::JsonRpcRequest;
use serde_json::Value;

pub struct HadronDaemonClient {
    pub endpoint: String,
}

impl HadronDaemonClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn build_request(&self, method: &str, params: Value) -> JsonRpcRequest {
        JsonRpcRequest::new(method, params)
    }
}
```

```rust
// crates/hadron-tauri/src/lib.rs
pub mod client;
pub use client::HadronDaemonClient;
```

Update root `Cargo.toml` `workspace.members` to include `"crates/hadron-tauri"`.

- [ ] **Step 4: Verify test passes**
Run: `cargo test -p hadron-tauri --test client_tests`
Expected: `test result: ok. 1 passed; 0 failed; 0 ignored`

- [ ] **Step 5: Commit**
```bash
git add crates/hadron-tauri/ Cargo.toml
git commit -m "feat(tauri): scaffold hadron-tauri client bridge and wire protocol bindings"
```

---

## Execution Handoff & Swarm Dispatch Options

**Plan complete and saved to `.hadron/docs/plans/2026-09-03-headless-engine-tauri-architecture.md`.**

Execution options:
1. **Swarm Quark Dispatch (Recommended)**: Fan out Task 1 to `@http-ollama` for wire protocol verification, Task 2 to `@cli-agy` for Actor bus integration, review each turn, and gate to base.
2. **Subagent-Driven**: Execute tasks via `subagent-driven-development`.
3. **Inline Execution**: Execute tasks sequentially via `executing-plans`.
