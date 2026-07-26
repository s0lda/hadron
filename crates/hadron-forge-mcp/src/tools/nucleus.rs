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
}
