//! The **prompt_distiller** tool: optimize prompts, preons, and context to token budget.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::prompt_distiller::PromptDistillerForge;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PromptDistillerArgs {
    pub prompt_text: String,
}

#[tool_router(router = prompt_distiller_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_prompt_distiller",
        description = "Distill and optimize prompt text and preon headers to fit constrained model token budgets"
    )]
    pub async fn prompt_distiller(
        &self,
        Parameters(args): Parameters<PromptDistillerArgs>,
    ) -> Json<ToolResponse> {
        let (optimized, report) = PromptDistillerForge::optimize(&args.prompt_text);
        let out = format!("### Prompt Distillation Report\n\n- Original bytes: {}\n- Optimized bytes: {}\n- Reduction: {:.1}%\n\n```markdown\n{}\n```",
            report.original_byte_len, report.optimized_byte_len, report.reduction_pct, optimized);
        Json(ToolResponse::success(Some(out)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prompt_distiller_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .prompt_distiller(Parameters(PromptDistillerArgs {
                prompt_text: "Line 1\n---\nLine 2".into(),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Prompt Distillation"));
    }
}
