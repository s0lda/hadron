use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::Event;

/// Vector clock tracking sequence numbers per swarm node for causality ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VectorClock {
    pub entries: HashMap<String, u64>,
}

impl VectorClock {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn get(&self, node_id: &str) -> u64 {
        self.entries.get(node_id).copied().unwrap_or(0)
    }

    pub fn increment(&mut self, node_id: &str) -> u64 {
        let val = self.entries.entry(node_id.to_string()).or_insert(0);
        *val += 1;
        *val
    }

    pub fn set(&mut self, node_id: &str, seq: u64) {
        self.entries.insert(node_id.to_string(), seq);
    }

    /// Merge another vector clock by taking component-wise maximums.
    pub fn merge(&mut self, other: &VectorClock) {
        for (node, &seq) in &other.entries {
            let entry = self.entries.entry(node.clone()).or_insert(0);
            if seq > *entry {
                *entry = seq;
            }
        }
    }

    /// Check if self is strictly greater than or equal to other in all dimensions.
    pub fn dominates(&self, other: &VectorClock) -> bool {
        for (node, &seq) in &other.entries {
            if self.get(node) < seq {
                return false;
            }
        }
        true
    }
}

/// A synchronization frame exchanged between peer Lattice daemons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatticeSyncFrame {
    pub origin_node: String,
    pub clock: VectorClock,
    pub delta_events: Vec<Event>,
    pub timestamp: DateTime<Utc>,
}

/// Metadata representing a remote peer Lattice swarm node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerLatticeNode {
    pub node_id: String,
    pub endpoint_url: String,
    pub last_synced_clock: VectorClock,
    pub latency_ms: u32,
    pub is_active: bool,
}

/// Distributed sync engine for P2P and remote swarm Lattice event replication.
pub struct LatticeSyncEngine {
    node_id: String,
    clock: VectorClock,
    peers: HashMap<String, PeerLatticeNode>,
}

impl LatticeSyncEngine {
    pub fn new(node_id: &str) -> Self {
        let mut clock = VectorClock::new();
        clock.increment(node_id);
        Self {
            node_id: node_id.to_string(),
            clock,
            peers: HashMap::new(),
        }
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn clock(&self) -> &VectorClock {
        &self.clock
    }

    pub fn register_peer(&mut self, peer: PeerLatticeNode) {
        self.peers.insert(peer.node_id.clone(), peer);
    }

    pub fn get_peer(&self, node_id: &str) -> Option<&PeerLatticeNode> {
        self.peers.get(node_id)
    }

    /// Prepare an outgoing sync frame carrying newly emitted events.
    pub fn create_outgoing_frame(&mut self, delta_events: Vec<Event>) -> LatticeSyncFrame {
        self.clock.increment(&self.node_id);
        LatticeSyncFrame {
            origin_node: self.node_id.clone(),
            clock: self.clock.clone(),
            delta_events,
            timestamp: Utc::now(),
        }
    }

    /// Ingest an incoming sync frame from a peer node, merging clocks and deduping events.
    pub fn ingest_incoming_frame(
        &mut self,
        frame: &LatticeSyncFrame,
        existing_events: &[Event],
    ) -> Vec<Event> {
        self.clock.merge(&frame.clock);
        if let Some(peer) = self.peers.get_mut(&frame.origin_node) {
            peer.last_synced_clock = frame.clock.clone();
        }

        let existing_ids: std::collections::HashSet<_> =
            existing_events.iter().map(|e| e.id).collect();

        // Filter only new events not already present in the existing ledger
        frame
            .delta_events
            .iter()
            .filter(|e| !existing_ids.contains(&e.id))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, Event, Kind};

    #[test]
    fn test_vector_clock_and_sync() {
        let mut node_a = LatticeSyncEngine::new("node_a");
        let mut node_b = LatticeSyncEngine::new("node_b");

        let ev1 = Event::new(Actor::Human, None, Kind::Message { body: "from a".into() });
        let frame_a = node_a.create_outgoing_frame(vec![ev1.clone()]);

        let ingested = node_b.ingest_incoming_frame(&frame_a, &[]);
        assert_eq!(ingested.len(), 1);
        assert_eq!(ingested[0].id, ev1.id);
        assert_eq!(node_b.clock().get("node_a"), frame_a.clock.get("node_a"));

        // Idempotent duplicate ingestion returns empty new events
        let ingested_dup = node_b.ingest_incoming_frame(&frame_a, &ingested);
        assert!(ingested_dup.is_empty());
    }
}
