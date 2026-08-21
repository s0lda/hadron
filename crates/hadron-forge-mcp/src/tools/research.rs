//! The **research** family: structured architectural research document creation, listing, and inspection.
//!
//! Exposes `hadron_forge_research_write`, `hadron_forge_research_list`, and `hadron_forge_research_read`.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::research::{list_research, read_research, write_research, ResearchWriteInput};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResearchWriteArgs {
    /// Kebab-case slug identifier (e.g. "custom-themes-engine").
    pub slug: String,
    /// Human-readable research document title.
    pub title: String,
    /// Optional author tag (e.g. "@Agy", "@researcher", "Human").
    #[serde(default)]
    pub author: Option<String>,
    /// Target subsystem or crate (e.g. "crates/hadron-chamber").
    #[serde(default)]
    pub target_area: Option<String>,
    /// High-level executive summary.
    #[serde(default)]
    pub executive_summary: Option<String>,
    /// Key findings and current state analysis.
    #[serde(default)]
    pub key_findings: Option<String>,
    /// Technical constraints, invariants, and boundaries.
    #[serde(default)]
    pub constraints: Option<String>,
    /// Approaches evaluated and trade-off comparison.
    #[serde(default)]
    pub trade_offs: Option<String>,
    /// Architectural recommendations and next steps.
    #[serde(default)]
    pub recommendations: Option<String>,
    /// Optional full custom markdown body to write verbatim instead of templated sections.
    #[serde(default)]
    pub custom_body: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResearchReadArgs {
    /// Relative or absolute path to the research document (e.g. ".hadron/docs/research/2026-08-21-theme-research.md").
    pub path: String,
}

#[tool_router(router = research_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_research_write",
        description = "Write or update a structured research paper or investigation document in .hadron/docs/research/."
    )]
    pub async fn research_write(
        &self,
        Parameters(args): Parameters<ResearchWriteArgs>,
    ) -> Json<ToolResponse> {
        let input = ResearchWriteInput {
            slug: args.slug,
            title: args.title,
            author: args.author,
            target_area: args.target_area,
            executive_summary: args.executive_summary,
            key_findings: args.key_findings,
            constraints: args.constraints,
            trade_offs: args.trade_offs,
            recommendations: args.recommendations,
            custom_body: args.custom_body,
        };

        match write_research(&self.root, &input) {
            Ok(output) => {
                let json = serde_json::to_string_pretty(&output)
                    .unwrap_or_else(|_| format!("Wrote research document to {}", output.rel_path));
                Json(ToolResponse::success(Some(json)))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_research_list",
        description = "List all existing research papers and investigation documents under .hadron/docs/research/."
    )]
    pub async fn research_list(&self) -> Json<ToolResponse> {
        match list_research(&self.root) {
            Ok(list) => {
                let json = serde_json::to_string_pretty(&list)
                    .unwrap_or_else(|_| "[]".to_string());
                Json(ToolResponse::success(Some(json)))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_research_read",
        description = "Read the markdown content of a research document under .hadron/docs/research/."
    )]
    pub async fn research_read(
        &self,
        Parameters(args): Parameters<ResearchReadArgs>,
    ) -> Json<ToolResponse> {
        match read_research(&self.root, &args.path) {
            Ok(content) => Json(ToolResponse::success(Some(content))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn research_mcp_tools_write_list_and_read() {
        let temp = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path());

        let res = server
            .research_write(Parameters(ResearchWriteArgs {
                slug: "theme-customization".into(),
                title: "Custom Theme Engine Exploration".into(),
                author: Some("@Agy".into()),
                target_area: Some("crates/hadron-chamber".into()),
                executive_summary: Some("Explore theme engine design".into()),
                key_findings: Some("Granular colors for tokens".into()),
                constraints: None,
                trade_offs: None,
                recommendations: Some("Author spec".into()),
                custom_body: None,
            }))
            .await;
        assert!(res.0.ok);

        let list_res = server.research_list().await;
        assert!(list_res.0.ok);
        assert!(list_res.0.blocks.as_deref().unwrap().contains("Custom Theme Engine Exploration"));
    }
}
