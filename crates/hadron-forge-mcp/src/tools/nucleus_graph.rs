//! The **nucleus_graph** family: swarm memory graph, dead links, and Mermaid visualizer.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::nucleus_graph::{self, NucleusGraphAction, NucleusGraphReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NucleusGraphArgs {
    pub action: String,
}

fn format_nucleus_graph(report: NucleusGraphReport) -> String {
    let mut out = format!("### Nucleus Graph Report\n\n{}\n\n", report.summary);
    out.push_str(&format!("- **Total Notes:** {}\n", report.total_notes));
    out.push_str(&format!("- **Total Inter-Note Links:** {}\n", report.total_links));
    out.push_str(&format!("- **Orphaned Notes:** {}\n", report.orphaned_notes.len()));
    out.push_str(&format!("- **Dead/Broken Links:** {}\n\n", report.dead_links.len()));

    if !report.orphaned_notes.is_empty() {
        out.push_str("#### Orphaned Notes (0 in/out links):\n");
        for o in report.orphaned_notes {
            out.push_str(&format!("- `{}`\n", o));
        }
        out.push('\n');
    }

    if !report.dead_links.is_empty() {
        out.push_str("#### Dead Links:\n");
        for (from, to) in report.dead_links {
            out.push_str(&format!("- `{}` → `{}` (missing target note)\n", from, to));
        }
        out.push('\n');
    }

    if let Some(mermaid) = report.mermaid_diagram {
        out.push_str("#### Graph Diagram:\n");
        out.push_str(&mermaid);
    }
    out
}

#[tool_router(router = nucleus_graph_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_nucleus_graph",
        description = "Analyze knowledge connectivity, detect broken/orphaned links, and generate Mermaid graphs for .hadron/nucleus/"
    )]
    pub async fn nucleus_graph(
        &self,
        Parameters(args): Parameters<NucleusGraphArgs>,
    ) -> Json<ToolResponse> {
        let action = match args.action.as_str() {
            "topology" => NucleusGraphAction::Topology,
            "dead_links" => NucleusGraphAction::DeadLinks,
            "orphans" => NucleusGraphAction::Orphans,
            "mermaid" => NucleusGraphAction::Mermaid,
            other => {
                return Json(ToolResponse::error(format!(
                    "Unknown nucleus_graph action '{}'. Expected: topology, dead_links, orphans, mermaid",
                    other
                )))
            }
        };

        match nucleus_graph::run_nucleus_graph(&self.nucleus_root, action) {
            Ok(report) => Json(ToolResponse::success(Some(format_nucleus_graph(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn nucleus_graph_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .nucleus_graph(Parameters(NucleusGraphArgs {
                action: "topology".to_string(),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Nucleus Knowledge Graph"));
    }
}
