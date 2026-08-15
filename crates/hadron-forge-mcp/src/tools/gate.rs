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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AcceptanceGateArgs {
    /// Base branch to check against (defaults to "main")
    pub base: Option<String>,
    /// When true, executes cargo workspace tests
    pub run_unit_tests: Option<bool>,
    /// When true, executes cargo workspace check
    pub run_lint_check: Option<bool>,
    /// Minimum count of screenshot files required in .hadron/screenshots/
    pub min_screenshots: Option<usize>,
    /// Custom shell commands to verify
    pub custom_commands: Option<Vec<String>>,
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

    #[tool(
        name = "hadron_forge_acceptance_gate",
        description = "Run multi-modal acceptance verification gate (preflight rebase, workspace tests, linter, screenshot validation, and custom commands)"
    )]
    pub async fn acceptance_gate(&self, Parameters(args): Parameters<AcceptanceGateArgs>) -> Json<ToolResponse> {
        let root = self.root.clone();
        let config = hadron_forge::gate::AcceptanceSuiteConfig {
            base: args.base,
            run_unit_tests: args.run_unit_tests.unwrap_or(true),
            run_lint_check: args.run_lint_check.unwrap_or(false),
            verify_process_lifecycle: None,
            verify_screenshots: args.min_screenshots.map(|min| hadron_forge::gate::ScreenshotVerificationCheck {
                min_count: min,
                check_dir: None,
            }),
            custom_commands: args.custom_commands.unwrap_or_default(),
        };

        let res = tokio::task::spawn_blocking(move || {
            hadron_forge::gate::run_acceptance_suite(&root, &config)
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
            Err(e) => Json(ToolResponse::error(format!("Acceptance gate task failed: {e}"))),
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
        run(&["branch", "-M", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.path().join(".gitignore"), ".hadron/\n").unwrap();
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

    #[tokio::test]
    async fn acceptance_gate_tool_executes() {
        let dir = fixture_repo();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .acceptance_gate(Parameters(AcceptanceGateArgs {
                base: Some("main".to_string()),
                run_unit_tests: Some(false),
                run_lint_check: Some(false),
                min_screenshots: None,
                custom_commands: Some(vec!["git status".to_string()]),
            }))
            .await;
        assert!(res.0.blocks.is_some());
    }
}

