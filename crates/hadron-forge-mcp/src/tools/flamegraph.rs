//! The **flamegraph** family: CPU and memory allocation hotspot profiling.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::flamegraph::{self, FlamegraphAction, FlamegraphReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FlamegraphArgs {
    pub action: String,
    pub folded_content: Option<String>,
    pub folded_file: Option<String>,
    pub output_svg_rel: Option<String>,
    pub title: Option<String>,
}

fn format_flamegraph(report: FlamegraphReport) -> String {
    let mut out = format!("### Flamegraph Profile Analysis\n\n{}\n\n", report.summary);
    out.push_str(&format!("- **Total Samples:** {}\n", report.total_samples));
    out.push_str(&format!("- **Unique Stacks:** {}\n", report.total_stacks));
    if let Some(svg) = report.svg_path {
        out.push_str(&format!("- **SVG Rendered:** `{}`\n", svg));
    }
    out.push('\n');

    if !report.top_hotspots.is_empty() {
        out.push_str("#### Top Hotspot Frames:\n");
        for h in report.top_hotspots {
            out.push_str(&format!(
                "- `{}`: **{:.1}% self** ({} samples) | **{:.1}% total** ({} samples)\n",
                h.name, h.self_percentage, h.self_samples, h.total_percentage, h.total_samples
            ));
        }
    }
    out
}

#[tool_router(router = flamegraph_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_flamegraph",
        description = "Analyze folded stack traces, compute CPU/memory hotspots, and generate interactive SVG flamegraphs"
    )]
    pub async fn flamegraph(
        &self,
        Parameters(args): Parameters<FlamegraphArgs>,
    ) -> Json<ToolResponse> {
        let action = match args.action.as_str() {
            "analyze_folded" => FlamegraphAction::AnalyzeFolded,
            "top_hotspots" => FlamegraphAction::TopHotspots,
            "render_svg" => FlamegraphAction::RenderSvg,
            other => {
                return Json(ToolResponse::error(format!(
                    "Unknown flamegraph action '{}'. Expected: analyze_folded, top_hotspots, render_svg",
                    other
                )))
            }
        };

        match flamegraph::run_flamegraph(
            &self.root,
            action,
            args.folded_content.as_deref(),
            args.folded_file.as_deref(),
            args.output_svg_rel.as_deref(),
            args.title.as_deref(),
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_flamegraph(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn flamegraph_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .flamegraph(Parameters(FlamegraphArgs {
                action: "top_hotspots".to_string(),
                folded_content: Some("app;init 10\napp;render 90\n".to_string()),
                folded_file: None,
                output_svg_rel: None,
                title: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Flamegraph Profile"));
    }
}
