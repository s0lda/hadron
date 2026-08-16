use serde::{Deserialize, Serialize};

/// A node in the spatial architecture canvas graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TopologyNode {
    Crate { name: String, path: String },
    Quark { id: String, role: String, state: String },
    Worktree { path: String, branch: String },
    Wiretap { channel: String, packet_count: usize },
}

/// A directed edge in the spatial architecture canvas graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologyEdge {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
}

/// Interactive Spatial Topology Graph for GPUI visualizer and Forge tooling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceTopologyGraph {
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
}

impl WorkspaceTopologyGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_crate(&mut self, name: &str, path: &str) {
        self.nodes.push(TopologyNode::Crate {
            name: name.to_string(),
            path: path.to_string(),
        });
    }

    pub fn add_quark(&mut self, id: &str, role: &str, state: &str) {
        self.nodes.push(TopologyNode::Quark {
            id: id.to_string(),
            role: role.to_string(),
            state: state.to_string(),
        });
    }

    pub fn add_worktree(&mut self, path: &str, branch: &str) {
        self.nodes.push(TopologyNode::Worktree {
            path: path.to_string(),
            branch: branch.to_string(),
        });
    }

    pub fn add_dependency(&mut self, from_crate: &str, to_crate: &str) {
        self.edges.push(TopologyEdge {
            source_id: from_crate.to_string(),
            target_id: to_crate.to_string(),
            relation: "depends_on".to_string(),
        });
    }

    pub fn add_quark_worktree_link(&mut self, quark_id: &str, worktree_branch: &str) {
        self.edges.push(TopologyEdge {
            source_id: quark_id.to_string(),
            target_id: worktree_branch.to_string(),
            relation: "operates_in".to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_graph_building() {
        let mut graph = WorkspaceTopologyGraph::new();
        graph.add_crate("hadron-chamber", "crates/hadron-chamber");
        graph.add_crate("hadron-gluon", "crates/hadron-gluon");
        graph.add_dependency("hadron-chamber", "hadron-gluon");

        graph.add_quark("agy", "orchestrator", "excited");
        graph.add_worktree(".hadron/trees/cli-agy", "quark/cli-agy/01M01");
        graph.add_quark_worktree_link("agy", "quark/cli-agy/01M01");

        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.edges.len(), 2);
    }
}
