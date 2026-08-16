//! The **wiretap** family: protocol stream inspection and frame assertion.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::wiretap::{self, WiretapAction, WiretapReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WiretapArgs {
    pub action: String,
    pub file_path: Option<String>,
    pub raw_payload: Option<String>,
    pub match_query: Option<String>,
    pub expected_sequence: Option<Vec<String>>,
}

fn format_wiretap(report: WiretapReport) -> String {
    let mut out = format!("### Wiretap Protocol Report\n\n{}\n\n", report.summary);
    out.push_str(&format!("- **Total Frames:** {}\n", report.total_frames));
    out.push_str(&format!("- **Matched Frames:** {}\n", report.matched_frames));
    out.push_str(&format!("- **Invalid Lines:** {}\n", report.invalid_frames));
    out.push_str(&format!("- **Sequence Valid:** {}\n\n", report.sequence_matched));

    if !report.frames_sample.is_empty() {
        out.push_str("#### Frame Sample:\n```json\n");
        for sample in report.frames_sample {
            out.push_str(&sample);
            out.push('\n');
        }
        out.push_str("```\n");
    }
    out
}

#[tool_router(router = wiretap_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_wiretap",
        description = "Inspect, filter, assert, and validate NDJSON and IPC protocol streams"
    )]
    pub async fn wiretap(
        &self,
        Parameters(args): Parameters<WiretapArgs>,
    ) -> Json<ToolResponse> {
        let action = match args.action.as_str() {
            "inspect_ndjson" => WiretapAction::InspectNdjson,
            "filter_frames" => WiretapAction::FilterFrames,
            "assert_sequence" => WiretapAction::AssertSequence,
            "validate_json" => WiretapAction::ValidateJson,
            other => {
                return Json(ToolResponse::error(format!(
                    "Unknown wiretap action '{}'. Expected: inspect_ndjson, filter_frames, assert_sequence, validate_json",
                    other
                )))
            }
        };

        match wiretap::run_wiretap(
            &self.root,
            action,
            args.file_path.as_deref(),
            args.raw_payload.as_deref(),
            args.match_query.as_deref(),
            args.expected_sequence,
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_wiretap(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wiretap_tool_handler_runs_on_payload() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .wiretap(Parameters(WiretapArgs {
                action: "inspect_ndjson".to_string(),
                file_path: None,
                raw_payload: Some("{\"event\":\"hello\"}\n{\"event\":\"world\"}".to_string()),
                match_query: None,
                expected_sequence: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Parsed 2 valid NDJSON frames"));
    }
}
