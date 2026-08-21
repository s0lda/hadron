use std::time::Duration;
use hadron_gluon::engine::heartbeat::{check_stalled_quarks, HeartbeatTelemetry, HeartbeatTracker};
use hadron_lattice::{live::QuarkLiveStatus, Doing, QuarkId};

#[test]
fn test_heartbeat_silence_detection_vs_active_streaming() {
    let mut telemetry = HeartbeatTelemetry::new();
    assert_eq!(telemetry.bytes_streamed, 0);
    assert!(!telemetry.is_stalled(Duration::from_millis(50)));

    // Record streaming output
    telemetry.record_chunk(128);
    telemetry.record_chunk(256);
    assert_eq!(telemetry.bytes_streamed, 384);
    assert!(!telemetry.is_stalled(Duration::from_millis(100)));

    // Set tool
    telemetry.set_tool(Some("bash_exec".to_string()));
    assert_eq!(telemetry.current_tool.as_deref(), Some("bash_exec"));

    // Check stall detection
    std::thread::sleep(Duration::from_millis(60));
    assert!(telemetry.is_stalled(Duration::from_millis(50)));
}

#[test]
fn test_check_stalled_quarks() {
    let now = chrono::Utc::now();
    let q1 = QuarkId::new("worker-1");
    let q2 = QuarkId::new("worker-2");

    let statuses = vec![
        QuarkLiveStatus {
            quark: q1.clone(),
            doing: Doing::Working,
            detail: "running test".into(),
            last_activity: now - chrono::Duration::seconds(5), // active
            bytes_streamed: 1024,
            current_tool: Some("bash_exec".into()),
        },
        QuarkLiveStatus {
            quark: q2.clone(),
            doing: Doing::Working,
            detail: "stuck on tool".into(),
            last_activity: now - chrono::Duration::seconds(45), // stalled
            bytes_streamed: 50,
            current_tool: Some("fetch".into()),
        },
    ];

    let stalled = check_stalled_quarks(&statuses, Duration::from_secs(30));
    assert_eq!(stalled, vec![q2]);
}

#[test]
fn test_heartbeat_tracker() {
    let mut tracker = HeartbeatTracker::new();
    let q1 = QuarkId::new("worker-1");
    let q2 = QuarkId::new("worker-2");

    tracker.register(q1.clone());
    tracker.register(q2.clone());

    tracker.record_output(&q1, 512);
    tracker.set_tool(&q1, Some("view_file".into()));

    assert_eq!(tracker.get(&q1).unwrap().bytes_streamed, 512);
    assert_eq!(tracker.get(&q1).unwrap().current_tool.as_deref(), Some("view_file"));

    std::thread::sleep(Duration::from_millis(60));
    let stalled = tracker.find_stalled(Duration::from_millis(50));
    assert_eq!(stalled.len(), 2);
}
