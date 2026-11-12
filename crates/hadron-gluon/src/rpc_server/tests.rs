use std::sync::Arc;
use super::dispatcher::*;
use super::server::RpcServer;
use crate::actor::bus::ActorBus;
use hadron_lattice::wire::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};

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
    assert_eq!(result["event_count"], 0);

    // Call unknown method
    let bad_req = JsonRpcRequest::new("invalid/method", serde_json::json!({}));
    let bad_res = dispatcher.dispatch(bad_req).await;
    assert!(bad_res.error.is_some());
    assert_eq!(bad_res.error.unwrap().code, -32601); // Method not found
}

#[tokio::test]
async fn test_rpc_dispatcher_wake_and_poll() {
    let bus = ActorBus::new(16);
    let tmp = tempfile::tempdir().unwrap();
    let field_path = tmp.path().join("field.jsonl");

    let ev1 = hadron_lattice::Event::new(
        hadron_lattice::Actor::Human,
        None,
        hadron_lattice::Kind::Message { body: "hello 1".into() },
    );
    hadron_lattice::io::append_event(&field_path, &ev1).unwrap();

    let (wake_tx, mut wake_rx) = mpsc::channel(1);
    let dispatcher = RpcDispatcher::new(bus).with_field_and_wake(field_path.clone(), wake_tx);

    // Test daemon/wake
    let wake_req = JsonRpcRequest::new("daemon/wake", serde_json::json!({}));
    let wake_res = dispatcher.dispatch(wake_req).await;
    assert!(wake_res.error.is_none());
    assert_eq!(wake_res.result.unwrap()["woken"], true);
    assert!(wake_rx.try_recv().is_ok());

    // Test field/poll
    let poll_req = JsonRpcRequest::new("field/poll", serde_json::json!({ "offset": 0 }));
    let poll_res = dispatcher.dispatch(poll_req).await;
    assert!(poll_res.error.is_none());
    let poll_result = poll_res.result.unwrap();
    assert_eq!(poll_result["total_count"], 1);
    let events = poll_result["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    let next_off = poll_result["next_offset"].as_u64().unwrap();
    assert!(next_off > 0);

    // Poll at next_offset should yield 0 new events
    let poll2_req = JsonRpcRequest::new("field/poll", serde_json::json!({ "offset": next_off }));
    let poll2_res = dispatcher.dispatch(poll2_req).await;
    assert_eq!(poll2_res.result.unwrap()["events"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_rpc_server_live_tcp_loopback() {
    let bus = ActorBus::new(16);
    bus.register_quark("orchestrator").await.unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let field_dir = tmp.path().to_path_buf();
    let field_path = field_dir.join("field.jsonl");
    let (wake_tx, mut wake_rx) = mpsc::channel(1);

    let dispatcher = Arc::new(RpcDispatcher::new(bus).with_field_and_wake(field_path, wake_tx));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let (addr, server_handle) = RpcServer::bind_and_serve(&field_dir, dispatcher, shutdown_rx)
        .await
        .expect("bind_and_serve");

    // Verify gluon.rpc metadata file written
    let meta = read_rpc_metadata(&field_dir).expect("metadata should exist");
    assert_eq!(meta.addr, addr.to_string());
    assert_eq!(meta.pid, std::process::id());

    // Connect via TCP client
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (reader, mut writer) = stream.split();
    let mut buf_reader = BufReader::new(reader);

    // Send engine/status request
    let req = JsonRpcRequest::new("engine/status", serde_json::json!({}));
    let req_line = format!("{}\n", serde_json::to_string(&req).unwrap());
    writer.write_all(req_line.as_bytes()).await.unwrap();

    let mut response_line = String::new();
    buf_reader.read_line(&mut response_line).await.unwrap();
    let res: JsonRpcResponse = serde_json::from_str(response_line.trim()).unwrap();
    assert_eq!(res.id, req.id);
    assert_eq!(res.result.unwrap()["status"], "running");

    // Send daemon/wake request
    let wake_req = JsonRpcRequest::new("daemon/wake", serde_json::json!({}));
    let wake_line = format!("{}\n", serde_json::to_string(&wake_req).unwrap());
    writer.write_all(wake_line.as_bytes()).await.unwrap();

    response_line.clear();
    buf_reader.read_line(&mut response_line).await.unwrap();
    let wake_res: JsonRpcResponse = serde_json::from_str(response_line.trim()).unwrap();
    assert_eq!(wake_res.id, wake_req.id);
    assert_eq!(wake_res.result.unwrap()["woken"], true);
    assert!(wake_rx.try_recv().is_ok());

    // Trigger shutdown
    shutdown_tx.send(true).unwrap();
    let _ = server_handle.await;

    // Verify metadata removed
    assert!(read_rpc_metadata(&field_dir).is_none());
}

#[tokio::test]
async fn test_rpc_server_daemon_subscribe_push_stream_and_disconnect() {
    let bus = ActorBus::new(16);
    bus.register_quark("orchestrator").await.unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let field_dir = tmp.path().to_path_buf();
    let field_path = field_dir.join("field.jsonl");
    let (wake_tx, _wake_rx) = mpsc::channel(4);

    let dispatcher = Arc::new(RpcDispatcher::new(bus).with_field_and_wake(field_path, wake_tx));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let (addr, server_handle) =
        RpcServer::bind_and_serve(&field_dir, Arc::clone(&dispatcher), shutdown_rx)
            .await
            .expect("bind_and_serve");

    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (reader, mut writer) = stream.split();
    let mut buf_reader = BufReader::new(reader);

    // 1. Send daemon/subscribe request and verify initial success ack
    let sub_req = JsonRpcRequest::new(DAEMON_SUBSCRIBE_METHOD, serde_json::json!({}));
    let sub_line = format!("{}\n", serde_json::to_string(&sub_req).unwrap());
    writer.write_all(sub_line.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();

    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        buf_reader.read_line(&mut line),
    )
    .await
    .expect("subscribe ack timeout")
    .expect("read subscribe ack");
    let ack: JsonRpcResponse = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(ack.id, sub_req.id);
    assert_eq!(ack.result.unwrap()["subscribed"], true);

    // 2. Publish push events on dispatcher and verify NDJSON notifications arrive immediately
    let pushed = vec![
        RpcPushEvent::FieldAppended {
            next_offset: 512,
            count: 2,
        },
        RpcPushEvent::LiveUpdated {
            quark: "acp-agy".to_string(),
        },
        RpcPushEvent::GateUpdated {
            quark: "cli-agy-2".to_string(),
            passed: Some(false),
        },
    ];
    for ev in &pushed {
        dispatcher.publish(ev.clone());
    }

    for expected in &pushed {
        line.clear();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            buf_reader.read_line(&mut line),
        )
        .await
        .expect("push event timeout")
        .expect("read push event");
        let decoded = JsonRpcCodec::decode_push_event(&line).expect("decode daemon/event");
        assert_eq!(&decoded, expected);
    }

    // 3. Clean client disconnect must not break subsequent publishes or server shutdown
    drop(buf_reader);
    drop(stream);
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    dispatcher.publish(RpcPushEvent::FieldAppended {
        next_offset: 1024,
        count: 1,
    });

    shutdown_tx.send(true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), server_handle)
        .await
        .expect("server shutdown timeout")
        .expect("server join");
}

