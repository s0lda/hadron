//! The **mesh** tool: containerized offload execution and remote worker routing.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::mesh::MeshForge;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MeshArgs {
    pub image: String,
    pub workdir: String,
    pub command: Vec<String>,
}

#[tool_router(router = mesh_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_swarm_mesh",
        description = "Configure and route containerized task offloading and remote worker orchestration"
    )]
    pub async fn swarm_mesh(
        &self,
        Parameters(args): Parameters<MeshArgs>,
    ) -> Json<ToolResponse> {
        let cmd = MeshForge::build_docker_run_command(&args.image, &args.workdir, &args.command);
        match serde_json::to_string_pretty(&cmd) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mesh_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .swarm_mesh(Parameters(MeshArgs {
                image: "rust:latest".into(),
                workdir: "/app".into(),
                command: vec!["cargo".into(), "test".into()],
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("docker"));
    }
}
