//! The **ast_rewrite** family: Tree-sitter structural pattern search and refactoring.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::ast_rewrite::{self, AstRewriteReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AstRewriteArgs {
    pub files: Vec<String>,
    pub node_kind: Option<String>,
    pub pattern: Option<String>,
    pub replacement: Option<String>,
    pub dry_run: Option<bool>,
}

fn format_ast_rewrite(report: AstRewriteReport) -> String {
    let mut out = format!("### AST Structural Rewrite Report\n\n{}\n\n", report.summary);
    if !report.matches.is_empty() {
        out.push_str("#### Matches Found:\n");
        for m in report.matches.iter().take(25) {
            out.push_str(&format!(
                "- `{}` [{}:{}] (`{}`)\n",
                m.file, m.start_line, m.start_col, m.node_kind
            ));
            out.push_str(&format!("  ```\n  {}\n  ```\n", m.matched_text.trim()));
            if let Some(rep) = &m.replacement_preview {
                out.push_str(&format!("  → Replacement Preview:\n  ```\n  {}\n  ```\n", rep.trim()));
            }
        }
    }
    out
}

#[tool_router(router = ast_rewrite_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_ast_rewrite",
        description = "Perform Tree-sitter AST structural search and code refactoring across multi-language files"
    )]
    pub async fn ast_rewrite(
        &self,
        Parameters(args): Parameters<AstRewriteArgs>,
    ) -> Json<ToolResponse> {
        let dry_run = args.dry_run.unwrap_or(true);
        match ast_rewrite::rewrite_ast(
            &self.root,
            &args.files,
            args.node_kind.as_deref(),
            args.pattern.as_deref(),
            args.replacement.as_deref(),
            dry_run,
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_ast_rewrite(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ast_rewrite_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .ast_rewrite(Parameters(AstRewriteArgs {
                files: vec!["src/lib.rs".to_string()],
                node_kind: Some("function_item".to_string()),
                pattern: None,
                replacement: None,
                dry_run: Some(true),
            }))
            .await;
        assert!(res.0.ok);
    }
}
