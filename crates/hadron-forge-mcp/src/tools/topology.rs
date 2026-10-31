//! The **topology** tool: crate dependencies, worktree nodes, and wiretap flows.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::topology::WorkspaceTopologyGraph;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TopologyArgs {
    pub include_worktrees: Option<bool>,
}

#[tool_router(router = topology_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_topology_graph",
        description = "Build spatial architecture canvas topology graph of crates, active worktrees, and IPC streams"
    )]
    pub async fn topology_graph(
        &self,
        Parameters(_args): Parameters<TopologyArgs>,
    ) -> Json<ToolResponse> {
        let graph = WorkspaceTopologyGraph::from_workspace(self.root.path());

        match serde_json::to_string_pretty(&graph) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn topology_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .topology_graph(Parameters(TopologyArgs {
                include_worktrees: Some(true),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("hadron-chamber"));
    }
}
