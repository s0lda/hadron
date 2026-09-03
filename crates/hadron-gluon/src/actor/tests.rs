use super::bus::*;
use super::mailbox::*;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_actor_bus_registration_and_message_delivery() {
    let bus = ActorBus::new(32);
    let (_handle, mut rx) = bus.register_quark("http-ollama").await.expect("register quark");

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
        _ => panic!("unexpected event variant"),
    }
}
