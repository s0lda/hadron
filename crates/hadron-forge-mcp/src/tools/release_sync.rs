//! The **release_sync** family: SemVer calculation and changelog generation.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::release_sync::{self, ReleaseSyncReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReleaseSyncArgs {
    pub since_tag: Option<String>,
    pub current_version: Option<String>,
}

fn format_release_sync(report: ReleaseSyncReport) -> String {
    let mut out = format!("### Release Sync & SemVer Report\n\n{}\n\n", report.summary);
    out.push_str(&format!("- **Base Ref/Tag:** `{}`\n", report.since_tag));
    out.push_str(&format!("- **Commits Analyzed:** {}\n", report.total_commits));
    out.push_str(&format!("- **Recommended SemVer Bump:** `{:?}`\n", report.recommended_bump));
    if let Some(ver) = report.recommended_version {
        out.push_str(&format!("- **Next Target Version:** `{}`\n", ver));
    }
    out.push('\n');

    if !report.changelog_snippet.is_empty() {
        out.push_str("#### Generated Changelog Snippet:\n\n");
        out.push_str(&report.changelog_snippet);
    }
    out
}

#[tool_router(router = release_sync_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_release_sync",
        description = "Analyze conventional commits, compute recommended SemVer version bump, and generate CHANGELOG snippets"
    )]
    pub async fn release_sync(
        &self,
        Parameters(args): Parameters<ReleaseSyncArgs>,
    ) -> Json<ToolResponse> {
        match release_sync::run_release_sync(
            &self.root,
            args.since_tag.as_deref(),
            args.current_version.as_deref(),
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_release_sync(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn release_sync_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .release_sync(Parameters(ReleaseSyncArgs {
                since_tag: Some("HEAD~1".to_string()),
                current_version: Some("0.9.1".to_string()),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Release Sync"));
    }
}
