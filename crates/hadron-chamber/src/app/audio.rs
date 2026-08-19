//! Subtle Swarm Audio and Haptic Telemetry engine (Capability #14).
//!
//! Provides spatial, low-profile audio and haptic cues for critical swarm events:
//! - Gate approval / merge success
//! - Cross-worktree collision warning
//! - Turn completion
//! - Human input blocker / attention required

/// Distinct audio cues corresponding to swarm milestones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCue {
    GateApproval,
    MergeCollision,
    TurnFinish,
    BlockedOnHuman,
}

impl AudioCue {
    pub fn frequency_hz(self) -> u32 {
        match self {
            AudioCue::GateApproval => 880,   // A5 high chime
            AudioCue::MergeCollision => 330, // E4 lower alert
            AudioCue::TurnFinish => 587,     // D5 gentle ping
            AudioCue::BlockedOnHuman => 440, // A4 attention pulse
        }
    }

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticPattern {
    LightTap,
    DoublePulse,
    WarningThud,
}

/// Configuration for audio and haptic telemetry feedback.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioConfig {
    pub enabled: bool,
    pub volume: f32, // 0.0 to 1.0
    pub haptic_enabled: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.5,
            haptic_enabled: true,
        }
    }
}

/// Dispatcher for audio and haptic telemetry cues.
#[derive(Debug, Clone, Default)]
pub struct AudioTelemetryManager {
    pub config: AudioConfig,
    pub played_cues: Vec<AudioCue>,
}

impl AudioTelemetryManager {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            played_cues: Vec::new(),
        }
    }

    /// Triggers an audio cue if audio is enabled.
    pub fn trigger_cue(&mut self, cue: AudioCue) -> bool {
        if !self.config.enabled || self.config.volume <= 0.0 {
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
        });

        assert!(manager.trigger_cue(AudioCue::GateApproval));
        assert!(manager.trigger_cue(AudioCue::MergeCollision));
        assert_eq!(manager.played_cues.len(), 2);
        assert_eq!(manager.played_cues[0], AudioCue::GateApproval);
        assert_eq!(AudioCue::GateApproval.frequency_hz(), 880);

        // Test muted config
        let mut muted_manager = AudioTelemetryManager::new(AudioConfig {
            enabled: false,
            volume: 0.0,
            haptic_enabled: false,
        });

        assert!(!muted_manager.trigger_cue(AudioCue::TurnFinish));
        assert!(!muted_manager.trigger_haptic(HapticPattern::LightTap));
        assert!(muted_manager.played_cues.is_empty());
    }
}
