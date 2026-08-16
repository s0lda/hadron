//! The **git_bisect** family: automated regression search across commit histories.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::git_bisect::{self, GitBisectReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GitBisectArgs {
    pub good_ref: String,
    pub bad_ref: Option<String>,
    pub test_program: String,
    pub test_args: Vec<String>,
    pub max_steps: Option<usize>,
}

fn format_git_bisect(report: GitBisectReport) -> String {
    let mut out = format!("### Git Bisect Automated Report\n\n{}\n\n", report.summary);
    out.push_str(&format!("- **Good Ref:** `{}`\n", report.good_ref));
    out.push_str(&format!("- **Bad Ref:** `{}`\n", report.bad_ref));
    out.push_str(&format!("- **Total Commits in Range:** {}\n", report.total_commits_evaluated));
    out.push_str(&format!("- **Bisect Steps Taken:** {}\n\n", report.steps_taken));

    if let Some(bad) = report.first_bad_commit {
        out.push_str("#### First Bad Commit Identified:\n");
        out.push_str(&format!("- **Commit:** `{}`\n", bad.commit_hash));
        out.push_str(&format!("- **Author:** {}\n", bad.author));
        out.push_str(&format!("- **Date:** {}\n", bad.date));
        out.push_str(&format!("- **Subject:** {}\n", bad.subject));
    }
    out
}

#[tool_router(router = git_bisect_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_git_bisect",
        description = "Automate binary regression search across git history with custom predicate test commands"
    )]
    pub async fn git_bisect(
        &self,
        Parameters(args): Parameters<GitBisectArgs>,
    ) -> Json<ToolResponse> {
        match git_bisect::run_git_bisect(
            &self.root,
            &args.good_ref,
            args.bad_ref.as_deref(),
            &args.test_program,
            &args.test_args,
            args.max_steps,
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_git_bisect(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn git_bisect_tool_handler_rejects_invalid_program() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .git_bisect(Parameters(GitBisectArgs {
                good_ref: "HEAD~1".to_string(),
                bad_ref: Some("HEAD".to_string()),
                test_program: "malicious_script".to_string(),
                test_args: vec![],
                max_steps: Some(1),
            }))
            .await;
        assert!(!res.0.ok);
    }
}
