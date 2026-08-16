//! The **preon_evolution** tool: cluster failure notes and synthesize candidate preons.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::preon_evolution::PreonForge;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PreonEvolutionArgs {
    pub notes: Vec<(String, String)>,
}

#[tool_router(router = preon_evolution_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_preon_evolution",
        description = "Extract recurring failure patterns from nucleus notes and synthesize evolutionary preons"
    )]
    pub async fn preon_evolution(
        &self,
        Parameters(args): Parameters<PreonEvolutionArgs>,
    ) -> Json<ToolResponse> {
        let clusters = PreonForge::cluster_notes(&args.notes);
        match serde_json::to_string_pretty(&clusters) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preon_evolution_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .preon_evolution(Parameters(PreonEvolutionArgs {
                notes: vec![("gpu-note".into(), "lavapipe rendering issue".into())],
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("rendering"));
    }
}
