//! Headless event replay and projection invariant simulation harness.
//!
//! Enforces Standard Model Invariants:
//! - "A Field Swap Resets Every List Cache"
//! - Synchronized indexing between `messages`, `chat_message_indices`, and `chat_list_state`
//! - Deterministic state projection without display server dependencies.

use hadron_lattice::{Actor, Event, Kind, Mode, QuarkId, QuarkState, Team};
use crate::model::{self, ChamberView, MessageRow, chat_message_indices};

/// Headless simulation session that replays event logs and validates UI projection invariants.
#[derive(Debug, Clone)]
pub struct HeadlessReplaySession {
    pub events: Vec<Event>,
    pub team: Team,
    pub global: Team,
    pub view: ChamberView,
    pub chat_ixs: Vec<usize>,
    pub total_chat_count: usize,
    pub total_log_count: usize,
}

impl HeadlessReplaySession {
    pub fn new() -> Self {
        let team = Team::default();
        let global = Team::default();
        let view = model::project_with_team(&[], &team, &global);
        let chat_ixs = chat_message_indices(&view.messages);
        let total_chat_count = chat_ixs.len();
        let total_log_count = view.messages.len();

        Self {
            events: Vec::new(),
            team,
            global,
            view,
            chat_ixs,
            total_chat_count,
            total_log_count,
        }
    }

    pub fn with_teams(team: Team, global: Team) -> Self {
        let mut session = Self::new();
        session.team = team;
        session.global = global;
        session.resync();
        session
    }

    /// Push an event and re-sync list indices.
    pub fn push_event(&mut self, event: Event) {
        self.events.push(event);
        self.resync();
    }

    /// Replay a full stream of events.
    pub fn replay_all(&mut self, events: Vec<Event>) {
        self.events = events;
        self.resync();
    }

    /// Clear session events (simulating /clear or /resume).
    pub fn clear(&mut self) {
        self.events.clear();
        self.resync();
    }

    /// Resync projection and all cached list indices (mirroring Chamber::resync_lists_to_projection).
    pub fn resync(&mut self) {
        self.view = model::project_with_team(&self.events, &self.team, &self.global);
        self.chat_ixs = chat_message_indices(&self.view.messages);
        self.total_chat_count = self.chat_ixs.len();
        self.total_log_count = self.view.messages.len();
    }

    /// Verify projection invariants:
    /// 1. Exactly one message row per event in view.messages.
    /// 2. `chat_ixs` contains strictly indices where `row.is_chat()`.
    /// 3. `total_chat_count == chat_ixs.len()` and `total_log_count == view.messages.len()`.
    /// 4. Every index in `chat_ixs` is within bounds of `view.messages`.
    pub fn assert_invariants(&self) -> Result<(), String> {
        if self.view.messages.len() != self.events.len() {
            return Err(format!(
                "Invariant violation: messages.len ({}) != events.len ({})",
                self.view.messages.len(),
                self.events.len()
            ));
        }

        if self.total_log_count != self.view.messages.len() {
            return Err(format!(
                "Invariant violation: total_log_count ({}) != messages.len ({})",
                self.total_log_count,
                self.view.messages.len()
            ));
        }

        if self.total_chat_count != self.chat_ixs.len() {
            return Err(format!(
                "Invariant violation: total_chat_count ({}) != chat_ixs.len ({})",
                self.total_chat_count,
                self.chat_ixs.len()
            ));
        }

        for &ix in &self.chat_ixs {
            if ix >= self.view.messages.len() {
                return Err(format!("Invariant violation: chat_ix {ix} out of bounds"));
            }
            if !self.view.messages[ix].is_chat() {
                return Err(format!("Invariant violation: message at index {ix} is not a chat row"));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_session_maintains_list_caches_on_append_and_clear() {
        let mut session = HeadlessReplaySession::new();
        assert_eq!(session.total_chat_count, 0);
        assert_eq!(session.total_log_count, 0);
        assert!(session.assert_invariants().is_ok());

        // Append user chat message
        session.push_event(Event::new(
            Actor::Human,
            None,
            Kind::Message {
                body: "Hello swarm".to_string(),
            },
        ));

        assert_eq!(session.total_chat_count, 1);
        assert_eq!(session.total_log_count, 1);
        assert!(session.assert_invariants().is_ok());

        // Append non-chat status event (visible in log, not in chat)
        session.push_event(Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::Status {
                state: QuarkState::Excited,
            },
        ));

        assert_eq!(session.total_chat_count, 1);
        assert_eq!(session.total_log_count, 2);
        assert!(session.assert_invariants().is_ok());

        // Append quark response message
        session.push_event(Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::Message {
                body: "Ready to work".to_string(),
            },
        ));

        assert_eq!(session.total_chat_count, 2);
        assert_eq!(session.total_log_count, 3);
        assert!(session.assert_invariants().is_ok());

        // Clear session (reproject empty)
        session.clear();
        assert_eq!(session.total_chat_count, 0);
        assert_eq!(session.total_log_count, 0);
        assert!(session.assert_invariants().is_ok());
    }
}
