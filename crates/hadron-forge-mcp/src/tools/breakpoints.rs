//! The **breakpoints** tool: tool breakpoints and step interception hooks.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::breakpoints::{BreakpointEntry, BreakpointsForge};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BreakpointsArgs {
    pub tool_name: String,
    pub argument_filter: Option<String>,
    pub test_args_json: Option<String>,
}

#[tool_router(router = breakpoints_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_tool_breakpoints",
        description = "Configure tool breakpoints and test interception matching against proposed tool calls"
    )]
    pub async fn tool_breakpoints(
        &self,
        Parameters(args): Parameters<BreakpointsArgs>,
    ) -> Json<ToolResponse> {
        let entry = BreakpointEntry {
            id: format!("bp-{}", hadron_lattice::Ulid::new()),
            tool_name: args.tool_name.clone(),
            argument_filter: args.argument_filter.clone(),
            enabled: true,
        };

        if let Some(test_json) = &args.test_args_json {
            let matched = BreakpointsForge::matches_breakpoint(&entry, &args.tool_name, test_json);
            let out = format!("Breakpoint `{}` matches: {}", entry.id, matched);
            Json(ToolResponse::success(Some(out)))
        } else {
            match serde_json::to_string_pretty(&entry) {
                Ok(json) => Json(ToolResponse::success(Some(json))),
                Err(e) => Json(ToolResponse::error(e.to_string())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn breakpoints_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .tool_breakpoints(Parameters(BreakpointsArgs {
                tool_name: "edit".into(),
                argument_filter: Some("Cargo.toml".into()),
                test_args_json: Some(r#"{"file":"Cargo.toml"}"#.into()),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("matches: true"));
    }
}
