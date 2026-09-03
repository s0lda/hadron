//! Sub-Channel Intercom & Voice Conversational Bridge.
//!
//! Provides low-latency audio queueing, voice activity detection (VAD) state machine,
//! and bidirectional audio packet dispatch between quarks and the human operator.

use std::collections::VecDeque;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntercomStatus {
    Idle,
    Listening,
    Speaking(String),
    Muted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioPacket {
    pub quark_id: String,
    pub timestamp_ms: u64,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

#[allow(dead_code)]
#[derive(Debug, Default, Clone)]
pub struct AudioIntercomBridge {
    status: IntercomStatus,
    audio_queue: VecDeque<AudioPacket>,
}

impl Default for IntercomStatus {
    fn default() -> Self {
        Self::Idle
    }
}

impl AudioIntercomBridge {
    pub fn new() -> Self {
        Self {
            status: IntercomStatus::Idle,
            audio_queue: VecDeque::new(),
        }
    }

    pub fn status(&self) -> &IntercomStatus {
        &self.status
    }

    pub fn set_status(&mut self, status: IntercomStatus) {
        self.status = status;
    }

    pub fn is_speaking(&self) -> bool {
        matches!(self.status, IntercomStatus::Speaking(_))
    }

    pub fn enqueue_audio(&mut self, packet: AudioPacket) {
        self.audio_queue.push_back(packet);
    }

    pub fn pop_audio(&mut self) -> Option<AudioPacket> {
        self.audio_queue.pop_front()
    }

    pub fn queue_len(&self) -> usize {
        self.audio_queue.len()
    }

    pub fn clear_queue(&mut self) {
        self.audio_queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intercom_lifecycle() {
        let mut bridge = AudioIntercomBridge::new();
        assert_eq!(*bridge.status(), IntercomStatus::Idle);
        assert!(!bridge.is_speaking());

        bridge.set_status(IntercomStatus::Speaking("agy".to_string()));
        assert!(bridge.is_speaking());

        let packet = AudioPacket {
            quark_id: "agy".to_string(),
            timestamp_ms: 1000,
            sample_rate: 16000,
            samples: vec![0.1, 0.2, -0.1],
        };

        bridge.enqueue_audio(packet.clone());
        assert_eq!(bridge.queue_len(), 1);

        let popped = bridge.pop_audio().unwrap();
        assert_eq!(popped, packet);
        assert_eq!(bridge.queue_len(), 0);
    }
}
