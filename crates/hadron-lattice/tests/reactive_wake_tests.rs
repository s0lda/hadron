use hadron_lattice::{emit_wakeup, subscribe_wakeups, LatticeWakeup};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_reactive_wakeup_signal_delivery() {
    let mut rx = subscribe_wakeups();

    let signal = LatticeWakeup::TaskReady("task-1.2".to_string());
    emit_wakeup(signal.clone()).expect("Failed to emit wakeup");

    let received = timeout(Duration::from_millis(50), rx.recv())
        .await
        .expect("Signal delivery exceeded deadline")
        .expect("Failed to receive wakeup");

    assert_eq!(received, signal);
}

#[tokio::test]
async fn test_all_wakeup_variants() {
    let mut rx = subscribe_wakeups();

    let events = vec![
        LatticeWakeup::TaskReady("task-1".into()),
        LatticeWakeup::GateFinished { branch: "feat/foo".into(), passed: true },
        LatticeWakeup::ToolBlocked { quark: "cli-agy".into(), reason: "file lock".into() },
        LatticeWakeup::HeartbeatStall { quark: "http-ollama".into() },
    ];

    for ev in &events {
        emit_wakeup(ev.clone()).unwrap();
    }

    for expected in events {
        let got = rx.recv().await.unwrap();
        assert_eq!(got, expected);
    }
}
