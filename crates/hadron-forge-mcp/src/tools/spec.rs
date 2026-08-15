//! The **spec** family: compile high-level feature prompts into formal Design Specs and DAG plans.
//!
//! Exposes `hadron_forge_spec_compile` to transform user intent into verified Markdown
//! specs and task dependency DAG plans matching the engine scheduler.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::spec_compiler::{compile_spec_and_plan, SpecCompileInput, SpecTaskInput};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpecTaskArgs {
    pub id: usize,
    pub title: String,
    #[serde(default)]
    pub dependencies: Vec<usize>,
    #[serde(default)]
    pub files_create: Vec<String>,
    #[serde(default)]
    pub files_modify: Vec<String>,
    #[serde(default)]
    pub files_test: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpecCompileArgs {
    /// High-level feature intent or product requirement.
    pub goal: String,
    /// Kebab-case slug identifier (e.g. "realtime-chat-backend").
    pub slug: String,
    /// Optional tech stack description (e.g. "Rust 2021, Tokio, Axum").
    #[serde(default)]
    pub tech_stack: Option<String>,
    /// Optional high-level architecture overview.
    #[serde(default)]
    pub architecture_overview: Option<String>,
    /// Optional explicit tasks list. If omitted or empty, auto-synthesizes standard TDD tasks.
    #[serde(default)]
    pub tasks: Vec<SpecTaskArgs>,
}

#[tool_router(router = spec_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_spec_compile",
        description = "Compile high-level feature intent into formal Design Spec (.hadron/docs/specs/) and executable DAG Implementation Plan (.hadron/docs/plans/)."
    )]
    pub async fn spec_compile(
        &self,
        Parameters(args): Parameters<SpecCompileArgs>,
    ) -> Json<ToolResponse> {
        let tasks = args
            .tasks
            .into_iter()
            .map(|t| SpecTaskInput {
                id: t.id,
                title: t.title,
                dependencies: t.dependencies,
                files_create: t.files_create,
                files_modify: t.files_modify,
                files_test: t.files_test,
                steps: t.steps,
            })
            .collect();

        let input = SpecCompileInput {
            goal: args.goal,
            slug: args.slug,
            tech_stack: args.tech_stack,
            architecture_overview: args.architecture_overview,
            tasks,
        };

        match compile_spec_and_plan(&self.root, &input) {
            Ok(output) => {
                let json = serde_json::to_string_pretty(&output)
                    .unwrap_or_else(|_| format!("Compiled spec to {}", output.spec_path));
                Json(ToolResponse::success(Some(json)))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spec_tool_compiles_spec_and_plan_files() {
        let temp = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path());

        let res = server
            .spec_compile(Parameters(SpecCompileArgs {
                goal: "Implement high throughput websocket server".into(),
                slug: "websocket-server-core".into(),
                tech_stack: Some("Rust, Tokio, Tungstenite".into()),
                architecture_overview: Some("Actor-based broadcast channels".into()),
                tasks: vec![],
            }))
            .await;

        assert!(res.0.ok);
        let blocks = res.0.blocks.unwrap();
        assert!(blocks.contains("websocket-server-core-design.md"));
        assert!(blocks.contains("websocket-server-core.md"));

        assert!(temp
            .path()
            .join(".hadron/docs/specs/2026-08-15-websocket-server-core-design.md")
            .exists());
        assert!(temp
            .path()
            .join(".hadron/docs/plans/2026-08-15-websocket-server-core.md")
            .exists());
    }
}
