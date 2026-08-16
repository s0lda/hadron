//! The **time_travel** tool: session history analysis, turn rewind, and session diffs.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::time_travel::TimeTravelForge;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TimeTravelArgs {
    pub action: String, // "analyze" | "rewind"
    pub events_ndjson: String,
    pub target_turn_ulid: Option<String>,
}

#[tool_router(router = time_travel_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_time_travel",
        description = "Analyze Lattice session event history, rewind to historical turn ULIDs, and inspect state deltas"
    )]
    pub async fn time_travel(
        &self,
        Parameters(args): Parameters<TimeTravelArgs>,
    ) -> Json<ToolResponse> {
        match args.action.as_str() {
            "analyze" => {
                let report = TimeTravelForge::analyze_session_events(&args.events_ndjson);
                match serde_json::to_string_pretty(&report) {
                    Ok(json) => Json(ToolResponse::success(Some(json))),
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "rewind" => {
                let target = args.target_turn_ulid.unwrap_or_default();
                let rewound = TimeTravelForge::rewind_ndjson(&args.events_ndjson, &target);
                Json(ToolResponse::success(Some(rewound)))
            }
            other => Json(ToolResponse::error(format!("Unknown action: {}", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn time_travel_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .time_travel(Parameters(TimeTravelArgs {
                action: "analyze".into(),
                events_ndjson: r#"{"v":1,"turn":"01M01"}"#.into(),
                target_turn_ulid: None,
            }))
            .await;
        assert!(res.0.ok);
    }
}
