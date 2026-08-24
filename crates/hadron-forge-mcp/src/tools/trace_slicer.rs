//! The **trace_slicer** family: semantic compaction of stack traces, compiler errors, and logs.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::trace_slicer::{self, SlicedTraceReport, TraceSlicerAction};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TraceSlicerArgs {
    pub action: String,
    pub raw_text: String,
    pub project_crates: Option<Vec<String>>,
    pub max_lines: Option<usize>,
    pub filter_term: Option<String>,
    pub min_level: Option<String>,
}

fn format_sliced_report(report: SlicedTraceReport) -> String {
    let mut out = format!(
        "### Sliced Trace & Log Compaction Report\n\n- **Original Lines:** {}\n- **Sliced Lines:** {}\n- **Compression:** {:.1}%\n",
        report.original_lines, report.sliced_lines, report.compression_ratio_pct
    );
    if let Some(rc) = report.root_cause {
        out.push_str(&format!("- **Identified Root Cause:** `{}`\n", rc));
    }
    out.push_str("\n```text\n");
    out.push_str(&report.formatted_output);
    out.push_str("\n```\n");
    out
}

#[tool_router(router = trace_slicer_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_trace_slicer",
        description = "Semantic backtrace, compiler cascade, and log slicer that compacts massive dumps into actionable root cause summaries"
    )]
    pub async fn trace_slicer(
        &self,
        Parameters(args): Parameters<TraceSlicerArgs>,
    ) -> Json<ToolResponse> {
        let action = match args.action.as_str() {
            "slice_backtrace" => TraceSlicerAction::SliceBacktrace,
            "compact_compiler_errors" => TraceSlicerAction::CompactCompilerErrors,
            "filter_log_spans" => TraceSlicerAction::FilterLogSpans,
            other => {
                return Json(ToolResponse::error(format!(
                    "Unknown trace slicer action '{}'. Expected: slice_backtrace, compact_compiler_errors, filter_log_spans",
                    other
                )))
            }
        };

        let crate_refs: Option<Vec<&str>> = args
            .project_crates
            .as_ref()
            .map(|crates| crates.iter().map(|s| s.as_str()).collect());

        match trace_slicer::run_trace_slicer(
            action,
            &args.raw_text,
            crate_refs.as_deref(),
            args.max_lines,
            args.filter_term.as_deref(),
            args.min_level.as_deref(),
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_sliced_report(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn trace_slicer_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .trace_slicer(Parameters(TraceSlicerArgs {
                action: "compact_compiler_errors".to_string(),
                raw_text: "error[E0308]: mismatched types\n --> src/main.rs:1:1\n".to_string(),
                project_crates: None,
                max_lines: Some(10),
                filter_term: None,
                min_level: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("error[E0308]"));
    }
}
