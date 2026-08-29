//! Ephemeral Quark-to-Quark Sub-Channels.
//!
//! Private peer-to-peer discussion threads for worker-reviewer pair programming
//! and specialist consultations. Sub-channel traffic is isolated from the main field
//! to prevent noise, bubbling up only synthesized checkpoints and final resolutions.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Role assumed by a quark participant within an ephemeral subchannel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SubChannelRole {
    Worker,
    Reviewer,
    Specialist,
    Auditor,
    Observer,
}

/// A participant registered in the subchannel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChannelParticipant {
    pub quark_id: String,
    pub role: SubChannelRole,
    pub joined_at: i64,
}

/// An individual message exchanged inside an isolated subchannel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChannelMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub timestamp: i64,
    pub is_milestone: bool,
}

/// Status of the ephemeral subchannel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubChannelStatus {
    Active,
    CheckpointSynthesized,
    Closed { closed_at: i64 },
}

/// Structured summary of subchannel progress bubbled up to the main field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChannelCheckpoint {
    pub subchannel_id: String,
    pub topic: String,
    pub participants: Vec<String>,
    pub message_count: usize,
    pub milestones: Vec<String>,
    pub summary: String,
    pub timestamp: i64,
}

/// An ephemeral peer-to-peer discussion channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChannel {
    pub id: String,
    pub topic: String,
    pub participants: HashMap<String, SubChannelParticipant>,
    pub messages: Vec<SubChannelMessage>,
    pub status: SubChannelStatus,
    pub created_at: i64,
}

impl SubChannel {
    pub fn new(id: &str, topic: &str, members: &[(&str, SubChannelRole)]) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        let mut participants = HashMap::new();

        for (quark, role) in members {
            participants.insert(
                quark.to_string(),
                SubChannelParticipant {
                    quark_id: quark.to_string(),
                    role: *role,
                    joined_at: now,
                },
            );
        }

        Self {
            id: id.to_string(),
            topic: topic.to_string(),
            participants,
            messages: Vec::new(),
            status: SubChannelStatus::Active,
            created_at: now,
        }
    }

    /// Post a standard message to the subchannel. Fails if sender is not an enrolled participant.
    pub fn post_message(&mut self, sender: &str, content: &str) -> Result<String, String> {
        if matches!(self.status, SubChannelStatus::Closed { .. }) {
            return Err(format!("Cannot post to closed subchannel {}", self.id));
        }

        if !self.participants.contains_key(sender) {
            return Err(format!("Quark '{}' is not a participant in subchannel {}", sender, self.id));
        }

        let msg_id = format!("msg-{}", self.messages.len() + 1);
        self.messages.push(SubChannelMessage {
            id: msg_id.clone(),
            sender: sender.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            is_milestone: false,
        });

        Ok(msg_id)
    }

    /// Post a checkpoint milestone (key decision or approval) to the subchannel.
    pub fn post_milestone(&mut self, sender: &str, content: &str) -> Result<String, String> {
        if matches!(self.status, SubChannelStatus::Closed { .. }) {
            return Err(format!("Cannot post milestone to closed subchannel {}", self.id));
        }

        if !self.participants.contains_key(sender) {
            return Err(format!("Quark '{}' is not a participant in subchannel {}", sender, self.id));
        }

        let msg_id = format!("milestone-{}", self.messages.len() + 1);
        self.messages.push(SubChannelMessage {
            id: msg_id.clone(),
            sender: sender.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            is_milestone: true,
        });

        Ok(msg_id)
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Synthesize an aggregated checkpoint from the channel history.
    pub fn synthesize_checkpoint(&mut self) -> SubChannelCheckpoint {
        let milestones: Vec<String> = self
            .messages
            .iter()
            .filter(|m| m.is_milestone)
            .map(|m| format!("- [@{}] {}", m.sender, m.content))
            .collect();

        let participants_list: Vec<String> = self.participants.keys().cloned().collect();

        let summary = format!(
            "Sub-channel '{}' (Topic: '{}') exchanged {} messages across {} participants.",
            self.id,
            self.topic,
            self.messages.len(),
            self.participants.len()
        );

        self.status = SubChannelStatus::CheckpointSynthesized;

        SubChannelCheckpoint {
            subchannel_id: self.id.clone(),
            topic: self.topic.clone(),
            participants: participants_list,
            message_count: self.messages.len(),
            milestones,
            summary,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Format a clean, human-readable synthesis notice suitable for the main field.
    pub fn format_field_synthesis(&mut self) -> String {
        let checkpoint = self.synthesize_checkpoint();
        let mut out = String::new();

        out.push_str(&format!(
            "💬 **Ephemeral Sub-Channel Checkpoint** (`{}`)\n\n",
            checkpoint.subchannel_id
        ));
        out.push_str(&format!("**Topic**: {}\n", checkpoint.topic));
        out.push_str(&format!(
            "**Participants**: {}\n",
            checkpoint
                .participants
                .iter()
                .map(|p| format!("`@{}`", p))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        out.push_str(&format!("**Exchanged Messages**: {}\n\n", checkpoint.message_count));

        if !checkpoint.milestones.is_empty() {
            out.push_str("### Key Synthesized Decisions & Checkpoints\n");
            for m in &checkpoint.milestones {
                out.push_str(&format!("{}\n", m));
            }
            out.push('\n');
        }

        out
    }

    /// Close the subchannel.
    pub fn close(&mut self) {
        self.status = SubChannelStatus::Closed {
            closed_at: chrono::Utc::now().timestamp_millis(),
        };
    }
}

/// Manager maintaining ephemeral peer-to-peer subchannels across the swarm.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubChannelManager {
    channels: HashMap<String, SubChannel>,
}

impl SubChannelManager {
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    /// Create and register a new ephemeral subchannel.
    pub fn create_channel(
        &mut self,
        id: &str,
        topic: &str,
        members: &[(&str, SubChannelRole)],
    ) -> Result<&mut SubChannel, String> {
        if self.channels.contains_key(id) {
            return Err(format!("Subchannel with ID '{}' already exists", id));
        }

        let channel = SubChannel::new(id, topic, members);
        self.channels.insert(id.to_string(), channel);
        Ok(self.channels.get_mut(id).unwrap())
    }

    pub fn get_channel(&self, id: &str) -> Option<&SubChannel> {
        self.channels.get(id)
    }

    pub fn get_channel_mut(&mut self, id: &str) -> Option<&mut SubChannel> {
        self.channels.get_mut(id)
    }

    pub fn post_to_channel(&mut self, channel_id: &str, sender: &str, content: &str) -> Result<String, String> {
        let channel = self
            .channels
            .get_mut(channel_id)
            .ok_or_else(|| format!("Subchannel '{}' not found", channel_id))?;
        channel.post_message(sender, content)
    }

    pub fn post_milestone_to_channel(&mut self, channel_id: &str, sender: &str, content: &str) -> Result<String, String> {
        let channel = self
            .channels
            .get_mut(channel_id)
            .ok_or_else(|| format!("Subchannel '{}' not found", channel_id))?;
        channel.post_milestone(sender, content)
    }

    /// Synthesize checkpoint and close channel, returning checkpoint and field message.
    pub fn synthesize_and_close(&mut self, channel_id: &str) -> Result<(SubChannelCheckpoint, String), String> {
        let channel = self
            .channels
            .get_mut(channel_id)
            .ok_or_else(|| format!("Subchannel '{}' not found", channel_id))?;
        let field_notice = channel.format_field_synthesis();
        let checkpoint = channel.synthesize_checkpoint();
        channel.close();
        Ok((checkpoint, field_notice))
    }

    /// List active (non-closed) channels.
    pub fn active_channels(&self) -> Vec<&SubChannel> {
        self.channels
            .values()
            .filter(|c| !matches!(c.status, SubChannelStatus::Closed { .. }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subchannel_lifecycle_and_checkpoint_synthesis() {
        let mut manager = SubChannelManager::new();
        manager
            .create_channel(
                "pair-dag-merge",
                "Review DAG Barrier Scheduler AST refactor",
                &[
                    ("worker-alpha", SubChannelRole::Worker),
                    ("reviewer-beta", SubChannelRole::Reviewer),
                ],
            )
            .expect("Channel must be created");

        // Worker sends message
        manager
            .post_to_channel(
                "pair-dag-merge",
                "worker-alpha",
                "Refactored Kahn cycle detection in dag_scheduler.rs",
            )
            .unwrap();

        // Reviewer responds
        manager
            .post_to_channel(
                "pair-dag-merge",
                "reviewer-beta",
                "Checked line 150. Memory barrier looks safe.",
            )
            .unwrap();

        // Reviewer posts approval milestone
        manager
            .post_milestone_to_channel(
                "pair-dag-merge",
                "reviewer-beta",
                "Verified zero regressions. Invariants respected.",
            )
            .unwrap();

        // Non-participant cannot post
        let err = manager.post_to_channel("pair-dag-merge", "uninvited-quark", "Hello");
        assert!(err.is_err());

        // Synthesize and close
        let (checkpoint, field_notice) = manager
            .synthesize_and_close("pair-dag-merge")
            .expect("Synthesis must succeed");

        assert_eq!(checkpoint.message_count, 3);
        assert_eq!(checkpoint.milestones.len(), 1);
        assert!(checkpoint.milestones[0].contains("Verified zero regressions"));
        assert!(field_notice.contains("Ephemeral Sub-Channel Checkpoint"));
        assert!(field_notice.contains("worker-alpha"));
        assert!(field_notice.contains("reviewer-beta"));

        // Cannot post to closed channel
        let post_err = manager.post_to_channel("pair-dag-merge", "worker-alpha", "Another message");
        assert!(post_err.is_err());
    }
}
