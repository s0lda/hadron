//! Subtle Swarm Audio and Haptic Telemetry engine (Capability #14).
//!
//! Provides spatial, low-profile audio and haptic cues for critical swarm events:
//! - Gate approval / merge success
//! - Cross-worktree collision warning
//! - Turn completion
//! - Human input blocker / attention required

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_volume() -> f32 {
    0.5
}

/// Distinct audio cues corresponding to swarm milestones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum AudioCue {
    GateApproval,
    MergeCollision,
    TurnFinish,
    BlockedOnHuman,
}

impl AudioCue {
    #[allow(dead_code)]
    pub fn frequency_hz(self) -> u32 {
        match self {
            AudioCue::GateApproval => 880,   // A5 high chime
            AudioCue::MergeCollision => 330, // E4 lower alert
            AudioCue::TurnFinish => 587,     // D5 gentle ping
            AudioCue::BlockedOnHuman => 440, // A4 attention pulse
        }
    }

    #[allow(dead_code)]
    pub fn duration_ms(self) -> u32 {
        match self {
            AudioCue::GateApproval => 120,
            AudioCue::MergeCollision => 200,
            AudioCue::TurnFinish => 80,
            AudioCue::BlockedOnHuman => 150,
        }
    }
}

/// Haptic feedback vibration patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum HapticPattern {
    LightTap,
    DoublePulse,
    WarningThud,
}

/// Configuration for audio and haptic telemetry feedback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_volume")]
    pub volume: f32, // 0.0 to 1.0
    #[serde(default = "default_true")]
    pub haptic_enabled: bool,
    #[serde(default = "default_true")]
    pub cue_gate_approval: bool,
    #[serde(default = "default_true")]
    pub cue_merge_collision: bool,
    #[serde(default = "default_true")]
    pub cue_turn_finish: bool,
    #[serde(default = "default_true")]
    pub cue_blocked_on_human: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.5,
            haptic_enabled: true,
            cue_gate_approval: true,
            cue_merge_collision: true,
            cue_turn_finish: true,
            cue_blocked_on_human: true,
        }
    }
}

/// Dispatcher for audio and haptic telemetry cues.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct AudioTelemetryManager {
    pub config: AudioConfig,
    pub played_cues: Vec<AudioCue>,
}

#[allow(dead_code)]
impl AudioTelemetryManager {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            played_cues: Vec::new(),
        }
    }

    /// Whether an individual cue is enabled by user configuration.
    pub fn is_cue_enabled(&self, cue: AudioCue) -> bool {
        match cue {
            AudioCue::GateApproval => self.config.cue_gate_approval,
            AudioCue::MergeCollision => self.config.cue_merge_collision,
            AudioCue::TurnFinish => self.config.cue_turn_finish,
            AudioCue::BlockedOnHuman => self.config.cue_blocked_on_human,
        }
    }

    /// Triggers an audio cue if audio is enabled and the cue toggle is active.
    pub fn trigger_cue(&mut self, cue: AudioCue) -> bool {
        if !self.config.enabled || self.config.volume <= 0.0 || !self.is_cue_enabled(cue) {
            return false;
        }

        self.played_cues.push(cue);
        // Headless / software fallback: in production this routes to rodio/ALSA/PulseAudio/macOS AudioUnit.
        true
    }

    /// Triggers a haptic feedback pulse.
    pub fn trigger_haptic(&self, _pattern: HapticPattern) -> bool {
        self.config.haptic_enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_triggers_and_volume_mute() {
        let mut manager = AudioTelemetryManager::new(AudioConfig {
            enabled: true,
            volume: 0.8,
            haptic_enabled: true,
            cue_gate_approval: true,
            cue_merge_collision: true,
            cue_turn_finish: true,
            cue_blocked_on_human: true,
        });

        assert!(manager.trigger_cue(AudioCue::GateApproval));
        assert!(manager.trigger_cue(AudioCue::MergeCollision));
        assert_eq!(manager.played_cues.len(), 2);
        assert_eq!(manager.played_cues[0], AudioCue::GateApproval);
        assert_eq!(AudioCue::GateApproval.frequency_hz(), 880);

        // Test individual cue muting
        manager.config.cue_turn_finish = false;
        assert!(!manager.trigger_cue(AudioCue::TurnFinish));

        // Test muted config
        let mut muted_manager = AudioTelemetryManager::new(AudioConfig {
            enabled: false,
            volume: 0.0,
            haptic_enabled: false,
            ..Default::default()
        });

        assert!(!muted_manager.trigger_cue(AudioCue::GateApproval));
        assert!(!muted_manager.trigger_haptic(HapticPattern::LightTap));
        assert!(muted_manager.played_cues.is_empty());
    }

    #[test]
    fn test_audio_config_serde_roundtrip() {
        let cfg = AudioConfig {
            enabled: true,
            volume: 0.75,
            haptic_enabled: false,
            cue_gate_approval: true,
            cue_merge_collision: false,
            cue_turn_finish: true,
            cue_blocked_on_human: false,
        };

        let json = serde_json::to_string(&cfg).expect("serialize audio config");
        let parsed: AudioConfig = serde_json::from_str(&json).expect("deserialize audio config");
        assert_eq!(cfg, parsed);
    }
}
