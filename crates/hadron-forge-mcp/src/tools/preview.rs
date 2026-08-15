//! The **preview** family: production packaging, background build, and live preview launcher.
//!
//! Exposes `hadron_forge_preview_launch` to compile projects, launch local services in isolated process groups,
//! and verify health checks for live interaction.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::preview::{launch_preview, PreviewLaunchInput};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PreviewLaunchArgs {
    /// Optional project packaging override ("rust", "node", "python", "static_html", "custom").
    #[serde(default)]
    pub project_type: Option<String>,
    /// Optional release or build command (e.g. "cargo build --release" or "npm run build").
    #[serde(default)]
    pub build_command: Option<String>,
    /// Optional server start command (e.g. "python3 -m http.server 8080" or "cargo run").
    #[serde(default)]
    pub start_command: Option<String>,
    /// Optional port number (defaults to 8080).
    #[serde(default)]
    pub port: Option<u16>,
    /// Optional HTTP health path (defaults to "/").
    #[serde(default)]
    pub health_path: Option<String>,
    /// Optional timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[tool_router(router = preview_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_preview_launch",
        description = "Build and launch a production/preview service in an isolated background process group, verify health checks, and return live preview URL."
    )]
    pub async fn preview_launch(
        &self,
        Parameters(args): Parameters<PreviewLaunchArgs>,
    ) -> Json<ToolResponse> {
        let input = PreviewLaunchInput {
            project_type: args.project_type,
            build_command: args.build_command,
            start_command: args.start_command,
            port: args.port,
            health_path: args.health_path,
            timeout_secs: args.timeout_secs,
        };

        match launch_preview(&self.process_manager, &input).await {
            Ok(report) => {
                let json = serde_json::to_string_pretty(&report)
                    .unwrap_or_else(|_| report.summary.clone());
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
    async fn preview_tool_launches_and_returns_url() {
        let temp = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path());

        let res = server
            .preview_launch(Parameters(PreviewLaunchArgs {
                project_type: Some("custom".into()),
                build_command: None,
                start_command: Some("git status".into()),
                port: Some(8080),
                health_path: Some("/".into()),
                timeout_secs: Some(5),
            }))
            .await;

        assert!(res.0.ok);
        let blocks = res.0.blocks.unwrap();
        assert!(blocks.contains("custom"));
        assert!(blocks.contains("process_id"));
    }
}
