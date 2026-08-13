//! The **semantic** search tool: embedded search over code symbols and nucleus notes.

use super::{ForgeMcpServer, ToolResponse};
use hadron_lattice::semantic::SemanticGraphIndex;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SemanticSearchArgs {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    5
}

#[tool_router(router = semantic_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_semantic_search",
        description = "Perform embedded local semantic search over code items and memory graph"
    )]
    pub async fn semantic_search(&self, Parameters(args): Parameters<SemanticSearchArgs>) -> Json<ToolResponse> {
        let mut index = SemanticGraphIndex::new_in_memory();
        // Index nucleus notes if accessible
        if let Ok(injector) = hadron_lattice::DynamicNucleusInjector::load_from_dir(self.nucleus_root.path()) {
            for note in injector.notes() {
                let _ = index.index_chunk(&note.slug, &note.description, &note.content);
            }
        }

        match index.search(&args.query, args.limit) {
            Ok(results) => {
                let mut out = String::new();
                for r in results {
                    out.push_str(&format!("- [{}] (score: {:.2})\n  {}\n\n", r.chunk.path, r.score, r.chunk.symbol));
                }
                Json(ToolResponse::success(Some(out)))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn semantic_tool_executes_query() {
        let worktree_dir = tempfile::tempdir().unwrap();
        let nucleus_dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::with_nucleus(worktree_dir.path(), nucleus_dir.path());

        let res = server
            .semantic_search(Parameters(SemanticSearchArgs {
                query: "merge gate".into(),
                limit: 5,
            }))
            .await;
        assert!(res.0.ok);
    }
}
