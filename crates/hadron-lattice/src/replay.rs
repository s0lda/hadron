//! Headless Turn Replay & Event Bisection (Capability #5).
//!
//! Provides deterministic offline session playback and automated bisection over event streams.

use crate::Event;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BisectResult {
    pub culprit_index: usize,
    pub culprit_event: Event,
    pub iterations: usize,
}

#[derive(Debug, Clone)]
pub struct HeadlessReplayer {
    events: Vec<Event>,
    cursor: usize,
}

impl HeadlessReplayer {
    pub fn new(events: Vec<Event>) -> Self {
        Self { events, cursor: 0 }
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    pub fn next_event(&mut self) -> Option<&Event> {
        if self.cursor < self.events.len() {
            let e = &self.events[self.cursor];
            self.cursor += 1;
            Some(e)
        } else {
            None
        }
    }

    pub fn events_up_to_cursor(&self) -> &[Event] {
        &self.events[..self.cursor]
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Bisects the event stream using a predicate function to pinpoint the first event introducing a fault.
    pub fn bisect<F>(&self, mut test_fn: F) -> Option<BisectResult>
    where
        F: FnMut(&[Event]) -> bool, // returns true if good (pass), false if bad (fail)
    {
        if self.events.is_empty() {
            return None;
        }

        // If the entire stream passes, no culprit exists
        if test_fn(&self.events) {
            return None;
        }

        // If even the first event fails, index 0 is culprit
        if !test_fn(&self.events[..1]) {
            return Some(BisectResult {
                culprit_index: 0,
                culprit_event: self.events[0].clone(),
                iterations: 1,
            });
        }

        let mut low = 0;
        let mut high = self.events.len() - 1;
        let mut iterations = 0;

        while low + 1 < high {
            iterations += 1;
            let mid = low + (high - low) / 2;
            if test_fn(&self.events[..=mid]) {
                low = mid;
            } else {
                high = mid;
            }
        }

        Some(BisectResult {
            culprit_index: high,
            culprit_event: self.events[high].clone(),
            iterations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, Kind, QuarkId};

    #[test]
    fn test_replay_step_and_bisection() {
        let mut events = Vec::new();
        for i in 0..10 {
            let kind = if i == 6 {
                Kind::Message {
                    body: "BREAKING BUG INTRODUCED".to_string(),
                }
            } else {
                Kind::Message {
                    body: format!("Normal turn {i}"),
                }
            };
            events.push(Event::new(Actor::Quark(QuarkId::new("cli-agy")), None, kind));
        }

        let mut replayer = HeadlessReplayer::new(events);
        assert_eq!(replayer.len(), 10);

        let mut step_count = 0;
        while replayer.next_event().is_some() {
            step_count += 1;
        }
        assert_eq!(step_count, 10);

        // Bisect: predicate passes if none of the events contain "BREAKING BUG"
        let res = replayer.bisect(|subset| {
            !subset.iter().any(|e| match &e.kind {
                Kind::Message { body } => body.contains("BREAKING BUG"),
                _ => false,
            })
        });

        assert!(res.is_some());
        let bisect = res.unwrap();
        assert_eq!(bisect.culprit_index, 6);
        match bisect.culprit_event.kind {
            Kind::Message { body } => assert!(body.contains("BREAKING BUG")),
            _ => panic!("Expected message kind"),
        }
    }
}
