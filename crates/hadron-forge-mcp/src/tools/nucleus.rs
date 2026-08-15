//! The **nucleus** family: search the swarm's shared lessons and invariants.
//!
//! Read-only, and rooted **outside** the worktree jail: the nucleus lives at
//! `<repo>/.hadron/nucleus`, which no quark's `Root` can reach.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::nucleus::query_nucleus;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryNucleusArgs {
    pub query: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DistillLessonArgs {
    /// Short kebab-case slug for the lesson (e.g. "always-touch-entrypoints-before-gate")
    pub slug: String,
    /// Retrieval key description (1 line, decides when to open note)
    pub description: String,
    /// The core fact or invariant discovered
    pub fact: String,
    /// Rationale explaining why this invariant is required
    pub why: String,
    /// Actionable instructions on how to apply the lesson
    pub how_to_apply: String,
    /// Section heading in index.md (e.g. "The merge gate" or "How we get things wrong")
    pub section: Option<String>,
}

#[tool_router(router = nucleus_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_query_nucleus",
        description = "Query the swarm's shared nucleus memory (index.md and notes/*.md) by keyword or path"
    )]
    pub async fn query_nucleus(&self, Parameters(args): Parameters<QueryNucleusArgs>) -> Json<ToolResponse> {
        match query_nucleus(&self.nucleus_root, &args.query, args.path.as_deref()) {
            Ok(results) => Json(ToolResponse::success(Some(results))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_nucleus_distill_lesson",
        description = "Autonomously distill a post-mortem lesson or permanent invariant into .hadron/nucleus/notes/<slug>.md and register a 1-line pointer in index.md"
    )]
    pub async fn nucleus_distill_lesson(&self, Parameters(args): Parameters<DistillLessonArgs>) -> Json<ToolResponse> {
        let input = hadron_forge::nucleus::DistillLessonInput {
            slug: args.slug,
            description: args.description,
            fact: args.fact,
            why: args.why,
            how_to_apply: args.how_to_apply,
            section: args.section,
        };

        match hadron_forge::nucleus::distill_lesson(&self.nucleus_root, &input) {
            Ok(out) => {
                let json = serde_json::to_string_pretty(&out).unwrap_or_else(|_| out.summary.clone());
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
    async fn nucleus_tool_queries_and_rejects_escape() {
        let worktree_dir = tempfile::tempdir().unwrap();
        let nucleus_dir = tempfile::tempdir().unwrap();
        std::fs::write(nucleus_dir.path().join("index.md"), "lesson about testing").unwrap();

        let server = ForgeMcpServer::with_nucleus(worktree_dir.path(), nucleus_dir.path());

        // Valid query
        let res = server
            .query_nucleus(Parameters(QueryNucleusArgs {
                query: "testing".into(),
                path: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.as_ref().unwrap().contains("lesson about testing"));

        // Path escape attempt
        let res_escape = server
            .query_nucleus(Parameters(QueryNucleusArgs {
                query: "testing".into(),
                path: Some("../secret.txt".into()),
            }))
            .await;
        assert!(!res_escape.0.ok);
        assert!(res_escape.0.reason.as_ref().unwrap().contains("escapes root"));
    }

    #[tokio::test]
    async fn nucleus_distill_lesson_tool_executes() {
        let worktree_dir = tempfile::tempdir().unwrap();
        let nucleus_dir = tempfile::tempdir().unwrap();
        std::fs::write(nucleus_dir.path().join("index.md"), "# Memory index\n\n## Swarm Lessons\n").unwrap();

        let server = ForgeMcpServer::with_nucleus(worktree_dir.path(), nucleus_dir.path());

        let res = server
            .nucleus_distill_lesson(Parameters(DistillLessonArgs {
                slug: "mcp-distilled-lesson".into(),
                description: "Retrieval key for mcp distilled note".into(),
                fact: "Fact content for mcp distilled note.".into(),
                why: "Testing mcp distillation pathway.".into(),
                how_to_apply: "Invoke tool whenever non-obvious fixes are made.".into(),
                section: Some("Swarm Lessons".into()),
            }))
            .await;

        assert!(res.0.ok);
        assert!(res.0.blocks.as_ref().unwrap().contains("mcp-distilled-lesson"));
    }
}

