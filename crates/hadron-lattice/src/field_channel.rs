//! Field event channel classification and window filtering.
//!
//! Separates primary conversation stream from ephemeral tool execution bursts,
//! diagnostics, and side-channel events to keep prompt projections within budget.

use serde::{Deserialize, Serialize};
use crate::event::{Event, Kind};

/// Logical channel for field event filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Channel {
    /// Standard conversation, handoffs, and status changes.
    #[default]
    Main,
    /// High-frequency tool execution bursts (commands, edits, snapshots, energy telemetry).
    ToolBurst,
    /// Ephemeral diagnostic traces or temporary heartbeats.
    Ephemeral,
}

impl Channel {
    /// Classify an event into a logical channel based on its properties and Kind.
    pub fn classify(event: &Event) -> Self {
        match &event.kind {
            Kind::Command { .. } | Kind::Edit { .. } | Kind::Snapshot { .. } | Kind::EnergyReport { .. } => {
                Channel::ToolBurst
            }
            Kind::Message { .. }
            | Kind::Assign { .. }
            | Kind::Status { .. }
            | Kind::PermissionReq { .. }
            | Kind::PermissionGrant { .. }
            | Kind::ModeSet { .. }
            | Kind::ModeClear
            | Kind::Reboot
            | Kind::SessionName { .. } => Channel::Main,
            Kind::Unknown { kind, .. } if kind == "trace" || kind == "ephemeral" => Channel::Ephemeral,
            _ => Channel::Main,
        }
    }
}

/// Filter events for a prompt window projection.
/// When `include_bursts` is false, `ToolBurst` and `Ephemeral` events are excluded.
pub fn filter_field_events(events: &[Event], include_bursts: bool) -> Vec<Event> {
    if include_bursts {
        events.to_vec()
    } else {
        events
            .iter()
            .filter(|e| Channel::classify(e) == Channel::Main)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Actor;
    use crate::QuarkId;

    #[test]
    fn channel_classifies_commands_and_edits_as_tool_burst() {
        let cmd_event = Event::new(
            Actor::Quark(QuarkId::new("worker")),
            None,
            Kind::Command {
                cmd: "cargo test".to_string(),
                exit: 0,
                out_summary: "ok".to_string(),
            },
        );
        assert_eq!(Channel::classify(&cmd_event), Channel::ToolBurst);

        let edit_event = Event::new(
            Actor::Quark(QuarkId::new("worker")),
            None,
            Kind::Edit {
                paths: vec!["src/lib.rs".to_string()],
                git: "diff".to_string(),
                summary: "modified lib".to_string(),
            },
        );
        assert_eq!(Channel::classify(&edit_event), Channel::ToolBurst);

        let msg_event = Event::new(
            Actor::Human,
            None,
            Kind::Message {
                body: "Hello swarm".to_string(),
            },
        );
        assert_eq!(Channel::classify(&msg_event), Channel::Main);
    }

    #[test]
    fn filter_field_events_removes_bursts_when_requested() {
        let msg = Event::new(Actor::Human, None, Kind::Message { body: "Work".to_string() });
        let cmd = Event::new(
            Actor::Quark(QuarkId::new("worker")),
            None,
            Kind::Command {
                cmd: "cargo build".to_string(),
                exit: 0,
                out_summary: "ok".to_string(),
            },
        );
        let events = vec![msg.clone(), cmd];

        let filtered = filter_field_events(&events, false);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, msg.id);

        let full = filter_field_events(&events, true);
        assert_eq!(full.len(), 2);
    }
}
