//! Real-time heartbeat telemetry and proactive stall recovery.
//!
//! Tracks fine-grained output stream progress (PTY bytes and tool chunk updates)
//! to detect hung or unresponsive quarks in seconds rather than waiting for 30m turn deadlines.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use hadron_lattice::{live::QuarkLiveStatus, QuarkId};

/// Heartbeat and streaming output telemetry for an active quark turn.
#[derive(Debug, Clone)]
pub struct HeartbeatTelemetry {
    pub last_chunk_at: Instant,
    pub bytes_streamed: u64,
    pub current_tool: Option<String>,
}

impl Default for HeartbeatTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl HeartbeatTelemetry {
    pub fn new() -> Self {
        Self {
            last_chunk_at: Instant::now(),
            bytes_streamed: 0,
            current_tool: None,
        }
    }

    /// Record newly streamed bytes from stdout/stderr or ACP chunks.
    pub fn record_chunk(&mut self, bytes_len: usize) {
        self.last_chunk_at = Instant::now();
        self.bytes_streamed += bytes_len as u64;
    }

    /// Set or clear the actively running tool.
    pub fn set_tool(&mut self, tool_name: Option<String>) {
        self.current_tool = tool_name;
        self.last_chunk_at = Instant::now();
    }

    /// Check if the telemetry has been silent for longer than `silence_threshold`.
    pub fn is_stalled(&self, silence_threshold: Duration) -> bool {
        self.last_chunk_at.elapsed() >= silence_threshold
    }
}

/// Check a slice of quark live statuses and identify any that have exceeded the silence threshold.
pub fn check_stalled_quarks(quarks: &[QuarkLiveStatus], silence_threshold: Duration) -> Vec<QuarkId> {
    let now = chrono::Utc::now();
    let threshold_chrono = chrono::Duration::from_std(silence_threshold)
        .unwrap_or_else(|_| chrono::Duration::seconds(30));

    quarks
        .iter()
        .filter(|q| now.signed_duration_since(q.last_activity) >= threshold_chrono)
        .map(|q| q.quark.clone())
        .collect()
}

/// Real-time heartbeat tracker managing multiple active quark telemetry streams.
#[derive(Debug, Default)]
pub struct HeartbeatTracker {
    telemetry: HashMap<QuarkId, HeartbeatTelemetry>,
}

impl HeartbeatTracker {
    pub fn new() -> Self {
        Self {
            telemetry: HashMap::new(),
        }
    }

    /// Register a newly excited quark with a fresh heartbeat tracker.
    pub fn register(&mut self, quark: QuarkId) {
        self.telemetry.insert(quark, HeartbeatTelemetry::new());
    }

    /// Record output from an active quark.
    pub fn record_output(&mut self, quark: &QuarkId, bytes_len: usize) {
        if let Some(t) = self.telemetry.get_mut(quark) {
            t.record_chunk(bytes_len);
        } else {
            let mut t = HeartbeatTelemetry::new();
            t.record_chunk(bytes_len);
            self.telemetry.insert(quark.clone(), t);
        }
    }

    /// Update the current active tool for a quark.
    pub fn set_tool(&mut self, quark: &QuarkId, tool: Option<String>) {
        if let Some(t) = self.telemetry.get_mut(quark) {
            t.set_tool(tool);
        }
    }

    /// Retrieve telemetry for a quark.
    pub fn get(&self, quark: &QuarkId) -> Option<&HeartbeatTelemetry> {
        self.telemetry.get(quark)
    }

    /// Unregister a quark upon turn completion or failure.
    pub fn unregister(&mut self, quark: &QuarkId) -> Option<HeartbeatTelemetry> {
        self.telemetry.remove(quark)
    }

    /// Identify all quarks that have been silent for longer than `silence_threshold`.
    pub fn find_stalled(&self, silence_threshold: Duration) -> Vec<QuarkId> {
        self.telemetry
            .iter()
            .filter(|(_, t)| t.is_stalled(silence_threshold))
            .map(|(q, _)| q.clone())
            .collect()
    }
}
