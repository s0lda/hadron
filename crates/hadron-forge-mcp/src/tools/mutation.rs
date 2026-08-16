//! The **mutation** tool: generate code mutants and calculate kill rates.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::mutation::AstMutator;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MutationGateArgs {
    pub file_path: String,
    pub source_code: String,
}

#[tool_router(router = mutation_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_mutation_gate",
        description = "Generate syntax-aware AST mutation candidates for adversarial mutation testing and red-teaming"
    )]
    pub async fn mutation_gate(
        &self,
        Parameters(args): Parameters<MutationGateArgs>,
    ) -> Json<ToolResponse> {
        let candidates = AstMutator::generate_mutations(&args.file_path, &args.source_code);
        match serde_json::to_string_pretty(&candidates) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mutation_gate_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .mutation_gate(Parameters(MutationGateArgs {
                file_path: "src/lib.rs".into(),
                source_code: "if x == 10 { true } else { false }".into(),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("!="));
    }
}
