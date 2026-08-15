//! Nucleus memory and budget linter MCP tool.

use super::{ForgeMcpServer, ToolResponse};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NucleusLintArgs {
    /// Optional index budget in KB (defaults to 32 KB)
    pub budget_kb: Option<usize>,
}

#[tool_router(router = nucleus_lint_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_nucleus_lint",
        description = "Lint .hadron/nucleus/ for 32 KB budget adherence, broken pointer references, orphan notes, and note frontmatter validity"
    )]
    pub async fn nucleus_lint(&self, Parameters(args): Parameters<NucleusLintArgs>) -> Json<ToolResponse> {
        let nucleus_root = self.nucleus_root.clone();
        let budget_kb = args.budget_kb;

        let res = tokio::task::spawn_blocking(move || {
            hadron_forge::nucleus_lint::lint_nucleus(&nucleus_root, budget_kb)
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
            Err(e) => Json(ToolResponse::error(format!("Nucleus lint task failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_nucleus() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("index.md");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();

        std::fs::write(
            &index,
            "- [sample-lesson](notes/sample-lesson.md) — A sample hook\n",
        )
        .unwrap();

        std::fs::write(
            notes.join("sample-lesson.md"),
            "---\nname: sample-lesson\ndescription: Retrieval key\nmetadata:\n  type: project\n---\nLesson content.\n",
        )
        .unwrap();

        dir
    }

    #[tokio::test]
    async fn nucleus_lint_tool_executes() {
        let dir = fixture_nucleus();
        let server = ForgeMcpServer::with_nucleus(dir.path(), dir.path());

        let res = server
            .nucleus_lint(Parameters(NucleusLintArgs { budget_kb: None }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Nucleus lint PASSED"));
    }
}
