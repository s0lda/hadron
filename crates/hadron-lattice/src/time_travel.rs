use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ulid::Ulid;

use crate::{Event, Kind};

/// Metadata and state snapshot for a forked swarm session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFork {
    pub fork_id: String,
    pub source_session_id: String,
    pub fork_turn: Option<Ulid>,
    pub fork_timestamp: DateTime<Utc>,
    pub event_count: usize,
    pub target_commit: Option<String>,
    pub name: String,
}

/// Comparative diff analysis between two session event streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionDiff {
    pub events_a_count: usize,
    pub events_b_count: usize,
    pub common_events: usize,
    pub divergent_events_a: usize,
    pub divergent_events_b: usize,
    pub touched_files_a: Vec<String>,
    pub touched_files_b: Vec<String>,
    pub token_spend_a: u64,
    pub token_spend_b: u64,
    pub token_spend_delta: i64,
}

/// Stepped playback state machine for replaying an event stream.
#[derive(Debug, Clone, PartialEq)]
pub struct EventPlayback {
    events: Vec<Event>,
    cursor: usize,
}

impl EventPlayback {
    pub fn new(events: Vec<Event>) -> Self {
        Self { events, cursor: 0 }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn total(&self) -> usize {
        self.events.len()
    }

    pub fn is_finished(&self) -> bool {
        self.cursor >= self.events.len()
    }

    pub fn current(&self) -> Option<&Event> {
        self.events.get(self.cursor)
    }

    pub fn step_forward(&mut self) -> Option<&Event> {
        if self.cursor < self.events.len() {
            let ev = &self.events[self.cursor];
            self.cursor += 1;
            Some(ev)
        } else {
            None
        }
    }

    pub fn step_backward(&mut self) -> Option<&Event> {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.events.get(self.cursor)
        } else {
            None
        }
    }

    pub fn seek(&mut self, index: usize) -> Option<&Event> {
        if index <= self.events.len() {
            self.cursor = index;
            self.events.get(self.cursor)
        } else {
            None
        }
    }

    pub fn progress_pct(&self) -> f32 {
        if self.events.is_empty() {
            100.0
        } else {
            (self.cursor as f32 / self.events.len() as f32) * 100.0
        }
    }
}

/// Time travel engine for Lattice session management, rewinding, forking, and diffing.
pub struct TimeTravelEngine;

impl TimeTravelEngine {
    /// Rewind an event list back to a specific target turn ULID.
    /// Returns the truncated event log and the last event preserved.
    pub fn rewind_to_turn(events: &[Event], target_turn: Ulid) -> (Vec<Event>, Option<Event>) {
        let mut preserved = Vec::new();
        for ev in events {
            preserved.push(ev.clone());
            if ev.turn == Some(target_turn) || ev.id == target_turn {
                break;
            }
        }
        let last = preserved.last().cloned();
        (preserved, last)
    }

    /// Rewind an event list to include only events on or before a given timestamp.
    pub fn rewind_to_timestamp(events: &[Event], target_ts: DateTime<Utc>) -> Vec<Event> {
        events
            .iter()
            .filter(|ev| ev.ts <= target_ts)
            .cloned()
            .collect()
    }

    /// Fork a new session from an existing event history up to an optional turn ULID.
    pub fn fork_session(
        source_session_id: &str,
        events: &[Event],
        at_turn: Option<Ulid>,
        new_session_name: &str,
    ) -> (SessionFork, Vec<Event>) {
        let forked_events = if let Some(turn) = at_turn {
            Self::rewind_to_turn(events, turn).0
        } else {
            events.to_vec()
        };

        // Locate latest git snapshot commit in the forked history if available
        let mut target_commit = None;
        for ev in forked_events.iter().rev() {
            if let Kind::Snapshot { git, .. } = &ev.kind {
                target_commit = Some(git.clone());
                break;
            } else if let Kind::Edit { git, .. } = &ev.kind {
                if !git.is_empty() {
                    target_commit = Some(git.clone());
                    break;
                }
            }
        }

        let fork_id = format!("fork_{}", Ulid::new());
        let fork_meta = SessionFork {
            fork_id,
            source_session_id: source_session_id.to_string(),
            fork_turn: at_turn,
            fork_timestamp: Utc::now(),
            event_count: forked_events.len(),
            target_commit,
            name: new_session_name.to_string(),
        };

        (fork_meta, forked_events)
    }

    /// Calculate diff metrics between two session histories.
    pub fn diff_sessions(session_a: &[Event], session_b: &[Event]) -> SessionDiff {
        let mut common_count = 0;
        let min_len = session_a.len().min(session_b.len());
        for i in 0..min_len {
            if session_a[i].id == session_b[i].id {
                common_count += 1;
            } else {
                break;
            }
        }

        let mut files_a = HashSet::new();
        let mut tokens_a: u64 = 0;
        for ev in session_a {
            if let Kind::Edit { paths, .. } = &ev.kind {
                for p in paths {
                    files_a.insert(p.clone());
                }
            }
            if let Some(usage) = &ev.usage {
                if let Some(fresh) = usage.spend.fresh() {
                    tokens_a += fresh as u64;
                }
            }
        }

        let mut files_b = HashSet::new();
        let mut tokens_b: u64 = 0;
        for ev in session_b {
            if let Kind::Edit { paths, .. } = &ev.kind {
                for p in paths {
                    files_b.insert(p.clone());
                }
            }
            if let Some(usage) = &ev.usage {
                if let Some(fresh) = usage.spend.fresh() {
                    tokens_b += fresh as u64;
                }
            }
        }

        let mut touched_files_a: Vec<String> = files_a.into_iter().collect();
        touched_files_a.sort();
        let mut touched_files_b: Vec<String> = files_b.into_iter().collect();
        touched_files_b.sort();

        SessionDiff {
            events_a_count: session_a.len(),
            events_b_count: session_b.len(),
            common_events: common_count,
            divergent_events_a: session_a.len().saturating_sub(common_count),
            divergent_events_b: session_b.len().saturating_sub(common_count),
            touched_files_a,
            touched_files_b,
            token_spend_a: tokens_a,
            token_spend_b: tokens_b,
            token_spend_delta: tokens_b as i64 - tokens_a as i64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, Event, Kind, TokenSpend, Usage};

    #[test]
    fn test_rewind_and_playback() {
        let turn1 = Ulid::new();
        let turn2 = Ulid::new();

        let e1 = Event::new(Actor::Human, None, Kind::Message { body: "hello".into() });
        let mut e2 = Event::new(Actor::Gluon, None, Kind::Message { body: "ack".into() });
        e2.turn = Some(turn1);
        let mut e3 = Event::new(Actor::Human, None, Kind::Message { body: "do task".into() });
        e3.turn = Some(turn2);

        let events = vec![e1.clone(), e2.clone(), e3.clone()];

        let (rewound, last) = TimeTravelEngine::rewind_to_turn(&events, turn1);
        assert_eq!(rewound.len(), 2);
        assert_eq!(last.unwrap().id, e2.id);

        let mut playback = EventPlayback::new(events);
        assert_eq!(playback.total(), 3);
        assert_eq!(playback.cursor(), 0);

        let step1 = playback.step_forward().unwrap();
        assert_eq!(step1.id, e1.id);
        assert_eq!(playback.cursor(), 1);

        let step2 = playback.step_forward().unwrap();
        assert_eq!(step2.id, e2.id);

        let prev = playback.step_backward().unwrap();
        assert_eq!(prev.id, e2.id);
        let prev_first = playback.step_backward().unwrap();
        assert_eq!(prev_first.id, e1.id);
    }

    #[test]
    fn test_session_fork_and_diff() {
        let turn1 = Ulid::new();
        let e1 = Event::new(Actor::Human, None, Kind::Message { body: "start".into() });
        let mut e2 = Event::new(
            Actor::Human,
            None,
            Kind::Edit {
                paths: vec!["src/main.rs".into()],
                git: "commit123".into(),
                summary: "edit".into(),
            },
        );
        e2.turn = Some(turn1);
        e2.usage = Some(Usage {
            spend: TokenSpend {
                input: Some(100),
                output: Some(50),
                cache_read: None,
                cache_write: None,
            },
            context: Default::default(),
            model: None,
            quota: Vec::new(),
        });

        let events = vec![e1.clone(), e2.clone()];
        let (fork, forked_events) =
            TimeTravelEngine::fork_session("sess_main", &events, Some(turn1), "alternative_branch");

        assert_eq!(fork.source_session_id, "sess_main");
        assert_eq!(fork.name, "alternative_branch");
        assert_eq!(fork.target_commit.as_deref(), Some("commit123"));
        assert_eq!(forked_events.len(), 2);

        let diff = TimeTravelEngine::diff_sessions(&events, &forked_events);
        assert_eq!(diff.common_events, 2);
        assert_eq!(diff.divergent_events_a, 0);
        assert_eq!(diff.divergent_events_b, 0);
        assert_eq!(diff.token_spend_a, 150);
        assert_eq!(diff.token_spend_delta, 0);
    }
}
