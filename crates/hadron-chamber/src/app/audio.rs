//! Subtle Swarm Audio and Haptic Telemetry engine (Capability #14).
//!
//! Provides spatial, low-profile audio and haptic cues for critical swarm events:
//! - Gate approval / merge success
//! - Cross-worktree collision warning
//! - Turn completion
//! - Human input blocker / attention required

use crate::config::SoundTheme;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_volume() -> f32 {
    0.5
}

/// Distinct audio cues corresponding to swarm milestones and chat interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum AudioCue {
    GateApproval,
    MergeCollision,
    TurnFinish,
    BlockedOnHuman,
    MessageReceived,
    MessageSent,
}

impl AudioCue {
    #[allow(dead_code)]
    pub fn frequency_hz(self, theme: SoundTheme) -> u32 {
        match theme {
            SoundTheme::Classic => match self {
                AudioCue::GateApproval => 880,   // A5 high crystal chime
                AudioCue::MergeCollision => 330, // E4 lower alert
                AudioCue::TurnFinish => 587,     // D5 gentle ping
                AudioCue::BlockedOnHuman => 440, // A4 attention pulse
                AudioCue::MessageReceived => 659,// E5 message arrival
                AudioCue::MessageSent => 784,    // G5 swoosh confirmation
            },
            SoundTheme::Synth => match self {
                AudioCue::GateApproval => 1046,  // C6 bright synth chime
                AudioCue::MergeCollision => 220, // A3 deep pulse
                AudioCue::TurnFinish => 740,     // F#5 FM ping
                AudioCue::BlockedOnHuman => 494, // B4 attention blip
                AudioCue::MessageReceived => 830,// G#5 electronic chirp
                AudioCue::MessageSent => 988,    // B5 crisp snap
            },
            SoundTheme::Minimal => match self {
                AudioCue::GateApproval => 1200,  // Soft high click
                AudioCue::MergeCollision => 350, // Low wooden tap
                AudioCue::TurnFinish => 900,     // Muted pop
                AudioCue::BlockedOnHuman => 600, // Focused acoustic tap
                AudioCue::MessageReceived => 1000,// Subtle water drop
                AudioCue::MessageSent => 1100,   // Light snap
            },
            SoundTheme::Retro8Bit => match self {
                AudioCue::GateApproval => 1318,  // E6 coin chime
                AudioCue::MergeCollision => 164, // E3 buzzer
                AudioCue::TurnFinish => 987,     // B5 victory blip
                AudioCue::BlockedOnHuman => 523, // C5 power alert
                AudioCue::MessageReceived => 1174,// D6 power-up tone
                AudioCue::MessageSent => 1396,   // F6 laser pip
            },
        }
    }

    #[allow(dead_code)]
    pub fn duration_ms(self, theme: SoundTheme) -> u32 {
        match theme {
            SoundTheme::Classic => match self {
                AudioCue::GateApproval => 120,
                AudioCue::MergeCollision => 200,
                AudioCue::TurnFinish => 80,
                AudioCue::BlockedOnHuman => 150,
                AudioCue::MessageReceived => 90,
                AudioCue::MessageSent => 60,
            },
            SoundTheme::Synth => match self {
                AudioCue::GateApproval => 140,
                AudioCue::MergeCollision => 220,
                AudioCue::TurnFinish => 90,
                AudioCue::BlockedOnHuman => 160,
                AudioCue::MessageReceived => 100,
                AudioCue::MessageSent => 70,
            },
            SoundTheme::Minimal => match self {
                AudioCue::GateApproval => 40,
                AudioCue::MergeCollision => 80,
                AudioCue::TurnFinish => 35,
                AudioCue::BlockedOnHuman => 50,
                AudioCue::MessageReceived => 30,
                AudioCue::MessageSent => 25,
            },
            SoundTheme::Retro8Bit => match self {
                AudioCue::GateApproval => 160,
                AudioCue::MergeCollision => 250,
                AudioCue::TurnFinish => 110,
                AudioCue::BlockedOnHuman => 180,
                AudioCue::MessageReceived => 100,
                AudioCue::MessageSent => 80,
            },
        }
    }

    #[allow(dead_code)]
    pub fn haptic_pattern(self) -> HapticPattern {
        match self {
            AudioCue::GateApproval => HapticPattern::DoublePulse,
            AudioCue::MergeCollision => HapticPattern::WarningThud,
            AudioCue::TurnFinish => HapticPattern::LightTap,
            AudioCue::BlockedOnHuman => HapticPattern::WarningThud,
            AudioCue::MessageReceived => HapticPattern::MessagePulse,
            AudioCue::MessageSent => HapticPattern::LightTap,
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
    MessagePulse,
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
    #[serde(default = "default_true")]
    pub cue_message_received: bool,
    #[serde(default = "default_true")]
    pub cue_message_sent: bool,
    #[serde(default)]
    pub sound_theme: SoundTheme,
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
            cue_message_received: true,
            cue_message_sent: true,
            sound_theme: SoundTheme::Classic,
        }
    }
}

/// Dispatcher for audio and haptic telemetry cues.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct AudioTelemetryManager {
    pub config: AudioConfig,
    pub played_cues: Vec<AudioCue>,
    pub played_haptics: Vec<HapticPattern>,
}

#[allow(dead_code)]
impl AudioTelemetryManager {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            played_cues: Vec::new(),
            played_haptics: Vec::new(),
        }
    }

    /// Whether an individual cue is enabled by user configuration.
    pub fn is_cue_enabled(&self, cue: AudioCue) -> bool {
        match cue {
            AudioCue::GateApproval => self.config.cue_gate_approval,
            AudioCue::MergeCollision => self.config.cue_merge_collision,
            AudioCue::TurnFinish => self.config.cue_turn_finish,
            AudioCue::BlockedOnHuman => self.config.cue_blocked_on_human,
            AudioCue::MessageReceived => self.config.cue_message_received,
            AudioCue::MessageSent => self.config.cue_message_sent,
        }
    }

    /// Triggers an audio cue if audio is enabled and the cue toggle is active.
    pub fn trigger_cue(&mut self, cue: AudioCue) -> bool {
        if !self.config.enabled || self.config.volume <= 0.0 || !self.is_cue_enabled(cue) {
            return false;
        }

        self.played_cues.push(cue);
        let freq = cue.frequency_hz(self.config.sound_theme);
        let dur = cue.duration_ms(self.config.sound_theme);
        let vol = self.config.volume;

        crate::sys::play_audio_tone(freq, dur, vol);
        if self.config.haptic_enabled {
            self.trigger_haptic(cue.haptic_pattern());
        }
        true
    }

    /// Triggers a haptic feedback pulse.
    pub fn trigger_haptic(&mut self, pattern: HapticPattern) -> bool {
        if self.config.haptic_enabled {
            self.played_haptics.push(pattern);
            true
        } else {
            false
        }
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
            cue_message_received: true,
            cue_message_sent: true,
            sound_theme: SoundTheme::Classic,
        });

        assert!(manager.trigger_cue(AudioCue::GateApproval));
        assert!(manager.trigger_cue(AudioCue::MessageReceived));
        assert_eq!(manager.played_cues.len(), 2);
        assert_eq!(manager.played_cues[0], AudioCue::GateApproval);
        assert_eq!(manager.played_cues[1], AudioCue::MessageReceived);
        assert_eq!(AudioCue::GateApproval.frequency_hz(SoundTheme::Classic), 880);
        assert_eq!(AudioCue::MessageReceived.frequency_hz(SoundTheme::Classic), 659);

        // Test theme frequency variation
        assert_eq!(AudioCue::GateApproval.frequency_hz(SoundTheme::Synth), 1046);
        assert_eq!(AudioCue::GateApproval.frequency_hz(SoundTheme::Minimal), 1200);
        assert_eq!(AudioCue::GateApproval.frequency_hz(SoundTheme::Retro8Bit), 1318);

        // Test individual cue muting
        manager.config.cue_message_received = false;
        assert!(!manager.trigger_cue(AudioCue::MessageReceived));

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
            cue_message_received: true,
            cue_message_sent: false,
            sound_theme: SoundTheme::Synth,
        };

        let json = serde_json::to_string(&cfg).expect("serialize audio config");
        let parsed: AudioConfig = serde_json::from_str(&json).expect("deserialize audio config");
        assert_eq!(cfg, parsed);
    }
}
