//! The **diagnostics** family: compiler errors and warnings, parsed.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::diagnostics::{self, Diagnostic};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiagnosticsArgs {
    pub package: Option<String>,
}

fn format_diagnostics(diagnostics: Vec<Diagnostic>) -> String {
    if diagnostics.is_empty() {
        return "clean: no compiler errors or warnings".to_string();
    }
    diagnostics
        .into_iter()
        .map(|d| {
            let code_str = d
                .code
                .as_ref()
                .map(|c| format!(" [{}]", c))
                .unwrap_or_default();
            format!("{}:{}: {} {}{}", d.file, d.line, d.level, d.message, code_str)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tool_router(router = diagnostics_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_diagnostics",
        description = "Run cargo check --message-format=json and return a compact list of errors and warnings"
    )]
    pub async fn diagnostics(
        &self,
        Parameters(args): Parameters<DiagnosticsArgs>,
    ) -> Json<ToolResponse> {
        match diagnostics::get_diagnostics(&self.root, args.package.as_deref()) {
            Ok(diags) => Json(ToolResponse::success(Some(format_diagnostics(diags)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn diagnostics_tool_handler_returns_success() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .diagnostics(Parameters(DiagnosticsArgs { package: None }))
            .await;
        assert!(res.0.ok);
    }

    #[tokio::test]
    async fn diagnostics_tool_handler_refuses_path_escape_in_package() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .diagnostics(Parameters(DiagnosticsArgs {
                package: Some("../evil".into()),
            }))
            .await;
        assert!(!res.0.ok);
        assert!(res.0.reason.unwrap().contains("outside the worktree"));
    }
}

