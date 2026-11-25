//! The **nucleus** family: search the swarm's shared lessons, synthesize invariants, and promote notes.
//!
//! Read-only queries and strict Standard Model writes rooted **outside** the worktree jail: the nucleus lives at
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PromoteNoteArgs {
    /// Short kebab-case slug for the note
    pub slug: String,
    /// One-line retrieval key description (< 100 chars)
    pub description: String,
    /// Core fact body
    pub fact: String,
    /// Note type: "project", "feedback", "user", or "reference"
    pub note_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InvariantSynthesizeArgs {
    /// Failure diagnostic output, compiler error, or test trace
    pub failure_text: String,
    /// Category: "compile_error", "borrow_checker", "test_failure", "runtime_panic", "merge_conflict", "security_violation"
    pub category: String,
    /// Whether to automatically persist to .hadron/nucleus/invariants/
    #[serde(default)]
    pub auto_persist: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NucleusSemanticSearchArgs {
    /// Natural language or keyword query
    pub query: String,
    /// Maximum number of search candidates to return
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    5
}

/// Record one open per distinct note slug in the usage ledger. Usage is telemetry:
/// a failed write must not fail the read that triggered it, so it is reported on
/// stderr (stdout is the MCP channel) rather than returned.
pub(super) fn record_note_opens<'a>(nucleus_root: &std::path::Path, slugs: impl IntoIterator<Item = &'a str>) {
    let unique: std::collections::BTreeSet<&str> = slugs.into_iter().collect();
    for slug in unique {
        if let Err(e) = hadron_lattice::nucleus_usage::record_note_open(nucleus_root, slug, None) {
            eprintln!("[forge] nucleus usage not recorded for {slug:?}: {e}");
        }
    }
}

/// Slugs of the note files that produced hits in `query_nucleus` output, whose
/// lines are `<rel-path>:<line>: <text>`. `index.md` lines that merely *link* to a
/// note are not opens of it.
fn note_hits(results: &str) -> Vec<String> {
    results
        .lines()
        .filter_map(|l| l.split_once(':'))
        .filter_map(|(path, _)| hadron_lattice::nucleus_usage::extract_note_slug_from_path(path))
        .collect()
}

#[tool_router(router = nucleus_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_query_nucleus",
        description = "Query the swarm's shared nucleus memory (index.md and notes/*.md) by keyword or path"
    )]
    pub async fn query_nucleus(&self, Parameters(args): Parameters<QueryNucleusArgs>) -> Json<ToolResponse> {
        match query_nucleus(&self.nucleus_root, &args.query, args.path.as_deref()) {
            Ok(results) => {
                let hits = note_hits(&results);
                let targeted = args.path.as_deref().and_then(hadron_lattice::nucleus_usage::extract_note_slug_from_path);
                record_note_opens(
                    self.nucleus_root.path(),
                    hits.iter().chain(targeted.iter()).map(String::as_str),
                );
                Json(ToolResponse::success(Some(results)))
            }
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

    #[tool(
        name = "hadron_forge_nucleus_promote",
        description = "Promote a discovered lesson, turn, or fact into a Standard Model nucleus note and append its pointer to index.md (Capability #20)"
    )]
    pub async fn nucleus_promote(&self, Parameters(args): Parameters<PromoteNoteArgs>) -> Json<ToolResponse> {
        let repo_root = self.root.path();
        let req = hadron_lattice::promoter::PromotionRequest {
            slug: args.slug,
            description: args.description,
            fact: args.fact,
            note_type: args.note_type,
        };
        match hadron_lattice::promoter::promote_to_note(repo_root, &req) {
            Ok(path) => Json(ToolResponse::success(Some(format!(
                "Successfully promoted note to {}",
                path.display()
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_invariant_synthesize",
        description = "Synthesize a structured Standard Model invariant from compiler errors, test panics, or merge failures (Capability #19)"
    )]
    pub async fn invariant_synthesize(&self, Parameters(args): Parameters<InvariantSynthesizeArgs>) -> Json<ToolResponse> {
        let cat = match args.category.to_ascii_lowercase().as_str() {
            "borrow_checker" | "borrow" => hadron_gatekeeper::InvariantCategory::BorrowChecker,
            "test_failure" | "test" => hadron_gatekeeper::InvariantCategory::TestFailure,
            "runtime_panic" | "runtime" => hadron_gatekeeper::InvariantCategory::RuntimePanic,
            "merge_conflict" | "merge" => hadron_gatekeeper::InvariantCategory::MergeConflict,
            "security_violation" | "security" => hadron_gatekeeper::InvariantCategory::SecurityViolation,
            _ => hadron_gatekeeper::InvariantCategory::CompileError,
        };

        let synthesized = hadron_gatekeeper::synthesize_invariant(&args.failure_text, cat);
        if args.auto_persist {
            let repo_root = self.root.path();
            match hadron_gatekeeper::write_synthesized_invariant(repo_root, &synthesized) {
                Ok(path) => {
                    let json = serde_json::to_string_pretty(&synthesized).unwrap_or_default();
                    Json(ToolResponse::success(Some(format!(
                        "Persisted invariant to {}\n{}",
                        path.display(),
                        json
                    ))))
                }
                Err(e) => Json(ToolResponse::error(e.to_string())),
            }
        } else {
            let json = serde_json::to_string_pretty(&synthesized).unwrap_or_default();
            Json(ToolResponse::success(Some(json)))
        }
    }

    #[tool(
        name = "hadron_forge_nucleus_semantic_search",
        description = "Vectorized semantic ranking search over .hadron/nucleus/notes/ lessons and invariants"
    )]
    pub async fn nucleus_semantic_search(&self, Parameters(args): Parameters<NucleusSemanticSearchArgs>) -> Json<ToolResponse> {
        let repo_root = self.root.path();
        match hadron_lattice::nucleus_search::query_nucleus_semantic(repo_root, &args.query, args.limit) {
            Ok(results) => {
                let json = serde_json::to_string_pretty(&results).unwrap_or_default();
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
    async fn query_nucleus_records_note_hits_but_not_index_links() {
        let worktree_dir = tempfile::tempdir().unwrap();
        let nucleus_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(nucleus_dir.path().join("notes")).unwrap();
        std::fs::write(nucleus_dir.path().join("index.md"), "- [cold](notes/cold.md) — gate lore\n").unwrap();
        std::fs::write(nucleus_dir.path().join("notes/hot.md"), "gate fact\nmore gate\n").unwrap();
        std::fs::write(nucleus_dir.path().join("notes/cold.md"), "unrelated\n").unwrap();
        let server = ForgeMcpServer::with_nucleus(worktree_dir.path(), nucleus_dir.path());

        let res = server
            .query_nucleus(Parameters(QueryNucleusArgs { query: "gate".into(), path: None }))
            .await;
        assert!(res.0.ok);
        let ledger = hadron_lattice::nucleus_usage::load_usage_ledger(nucleus_dir.path()).unwrap();
        assert_eq!(ledger.notes["hot"].opens, 1, "two matching lines are one open");
        assert!(!ledger.notes.contains_key("cold"), "an index link is not an open");

        server
            .query_nucleus(Parameters(QueryNucleusArgs { query: "zzz".into(), path: Some("notes/cold.md".into()) }))
            .await;
        let ledger = hadron_lattice::nucleus_usage::load_usage_ledger(nucleus_dir.path()).unwrap();
        assert_eq!(ledger.notes["cold"].opens, 1, "a targeted note read is an open");
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

    #[tokio::test]
    async fn nucleus_promote_and_invariant_synthesize_tools_execute() {
        let worktree_dir = tempfile::tempdir().unwrap();
        let nucleus_dir = worktree_dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus_dir).unwrap();
        std::fs::write(nucleus_dir.join("index.md"), "# Index\n").unwrap();

        let server = ForgeMcpServer::with_nucleus(worktree_dir.path(), nucleus_dir.as_path());

        let res_promote = server
            .nucleus_promote(Parameters(PromoteNoteArgs {
                slug: "mcp-promoted-rule".into(),
                description: "Retrieval key for promoted rule".into(),
                fact: "Promoted rule fact body".into(),
                note_type: Some("project".into()),
            }))
            .await;
        assert!(res_promote.0.ok);

        let res_synth = server
            .invariant_synthesize(Parameters(InvariantSynthesizeArgs {
                failure_text: "error[E0277]: the trait bound is not satisfied".into(),
                category: "compile_error".into(),
                auto_persist: true,
            }))
            .await;
        assert!(res_synth.0.ok);
        assert!(res_synth.0.blocks.as_ref().unwrap().contains("fix-error-e0277"));

        let res_search = server
            .nucleus_semantic_search(Parameters(NucleusSemanticSearchArgs {
                query: "trait bound error".into(),
                limit: 3,
            }))
            .await;
        assert!(res_search.0.ok);
    }
}
