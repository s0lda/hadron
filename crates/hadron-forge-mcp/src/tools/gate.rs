//! Pre-flight merge gate MCP tool.

use super::{ForgeMcpServer, ToolResponse};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PreflightGateArgs {
    /// Base branch to check against (defaults to "main")
    pub base: Option<String>,
    /// When true, runs `cargo check` instead of full `cargo test`
    pub check_only: Option<bool>,
}

#[tool_router(router = gate_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_preflight_gate",
        description = "Run pre-flight merge gate verification (rebase conflict check, touch entrypoints to avoid stale .rlib reuse, and execute workspace tests)"
    )]
    pub async fn preflight_gate(&self, Parameters(args): Parameters<PreflightGateArgs>) -> Json<ToolResponse> {
        let root = self.root.clone();
        let base = args.base;
        let check_only = args.check_only.unwrap_or(false);

        // Run preflight gate on worker thread since cargo test is blocking
        let res = tokio::task::spawn_blocking(move || {
            hadron_forge::gate::run_preflight_gate(&root, base.as_deref(), check_only)
        })
        .await;

        match res {
            Ok(Ok(report)) => {
                let json = serde_json::to_string_pretty(&report).unwrap_or_else(|_| report.summary.clone());
                if report.ok {
                    Json(ToolResponse::success(Some(json)))
                } else {
                    let mut resp = ToolResponse::error(report.summary);
                    resp.blocks = Some(json);
                    Json(resp)
                }
            }
            Ok(Err(e)) => Json(ToolResponse::error(e.to_string())),
            Err(e) => Json(ToolResponse::error(format!("Pre-flight gate execution task failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn fixture_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "initial commit"]);
        dir
    }

    #[tokio::test]
    async fn preflight_gate_tool_executes() {
        let dir = fixture_repo();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .preflight_gate(Parameters(PreflightGateArgs {
                base: Some("main".to_string()),
                check_only: Some(true),
            }))
            .await;
        // In fixture repo, cargo check should succeed
        assert!(res.0.blocks.is_some());
    }
}
