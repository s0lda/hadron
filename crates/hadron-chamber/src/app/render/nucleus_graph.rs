//! 2D/3D Force-Directed Nucleus Knowledge Graph.
//!
//! Provides a physical spring-embedder force simulation calculating dynamic 2D coordinates
//! for nucleus notes, invariants, and features connected by wiki-links.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForceNode {
    pub slug: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForceEdge {
    pub source: String,
    pub target: String,
}

#[allow(dead_code)]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ForceDirectedGraph {
    pub nodes: HashMap<String, ForceNode>,
    pub edges: Vec<ForceEdge>,
}

#[allow(dead_code)]
impl ForceDirectedGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, slug: impl Into<String>, x: f32, y: f32) {
        let s = slug.into();
        self.nodes.insert(
            s.clone(),
            ForceNode {
                slug: s,
                x,
                y,
                vx: 0.0,
                vy: 0.0,
            },
        );
    }

    pub fn add_edge(&mut self, source: impl Into<String>, target: impl Into<String>) {
        self.edges.push(ForceEdge {
            source: source.into(),
            target: target.into(),
        });
    }

    /// Advances the spring-embedder physical simulation by one tick.
    pub fn step_simulation(&mut self, repulsion_k: f32, attraction_k: f32, damping: f32) {
        let slugs: Vec<String> = self.nodes.keys().cloned().collect();

        // 1. Repulsive forces between all node pairs
        for i in 0..slugs.len() {
            for j in (i + 1)..slugs.len() {
                let n1 = &self.nodes[&slugs[i]];
                let n2 = &self.nodes[&slugs[j]];

                let dx = n2.x - n1.x;
                let dy = n2.y - n1.y;
                let dist_sq = (dx * dx + dy * dy).max(1.0);
                let dist = dist_sq.sqrt();

                let force = repulsion_k / dist_sq;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                if let Some(node1) = self.nodes.get_mut(&slugs[i]) {
                    node1.vx -= fx;
                    node1.vy -= fy;
                }
                if let Some(node2) = self.nodes.get_mut(&slugs[j]) {
                    node2.vx += fx;
                    node2.vy += fy;
                }
            }
        }

        // 2. Attractive forces along edges (Hooke's law)
        for edge in &self.edges {
            if let (Some(n1), Some(n2)) = (self.nodes.get(&edge.source), self.nodes.get(&edge.target)) {
                let dx = n2.x - n1.x;
                let dy = n2.y - n1.y;
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);

                let force = dist * attraction_k;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                if let Some(node1) = self.nodes.get_mut(&edge.source) {
                    node1.vx += fx;
                    node1.vy += fy;
                }
                if let Some(node2) = self.nodes.get_mut(&edge.target) {
                    node2.vx -= fx;
                    node2.vy -= fy;
                }
            }
        }

        // 3. Apply velocity and damping
        for node in self.nodes.values_mut() {
            node.x += node.vx;
            node.y += node.vy;
            node.vx *= damping;
            node.vy *= damping;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_force_directed_graph_repulsion() {
        let mut graph = ForceDirectedGraph::new();
        // Place two nodes very close together
        graph.add_node("note-a", 0.0, 0.0);
        graph.add_node("note-b", 1.0, 0.0);
        graph.add_edge("note-a", "note-b");

        graph.step_simulation(100.0, 0.0, 0.9);

        let na = &graph.nodes["note-a"];
        let nb = &graph.nodes["note-b"];

        // Repulsion should push them further apart along the x axis
        assert!(na.x < 0.0);
        assert!(nb.x > 1.0);
    }
}
