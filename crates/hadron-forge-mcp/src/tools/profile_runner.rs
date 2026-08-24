//! The **profile_runner** family: automated headless CPU and heap sampling profiler driver.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::profile_runner::{self, ProfileRunReport, ProfileType};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProfileRunnerArgs {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub profile_type: Option<String>,
    pub duration_secs: Option<u64>,
    pub top_limit: Option<usize>,
    pub output_svg_rel: Option<String>,
}

fn format_profile_report(report: ProfileRunReport) -> String {
    let mut out = format!("### Headless Profiler Report\n\n{}\n\n", report.summary);
    out.push_str(&format!("- **Duration:** {}ms\n", report.duration_ms));
    out.push_str(&format!("- **Total Samples:** {}\n", report.total_samples));
    if let Some(svg) = report.svg_path {
        out.push_str(&format!("- **SVG Flamegraph Saved:** `{}`\n", svg));
    }
    out.push('\n');

    if !report.top_hotspots.is_empty() {
        out.push_str("#### Top Hotspot Execution Frames:\n");
        for h in report.top_hotspots {
            out.push_str(&format!(
                "- `{}`: **{:.1}% self** ({} samples) | **{:.1}% total** ({} samples)\n",
                h.name, h.self_percentage, h.self_samples, h.total_percentage, h.total_samples
            ));
        }
    }
    out
}

#[tool_router(router = profile_runner_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_profile_runner",
        description = "Automated headless profiler runner that instruments target execution, captures CPU/heap samples, and outputs top hotspots and interactive SVG flamegraphs"
    )]
    pub async fn profile_runner(
        &self,
        Parameters(args): Parameters<ProfileRunnerArgs>,
    ) -> Json<ToolResponse> {
        let p_type = match args.profile_type.as_deref() {
            Some("heap") | Some("memory") => ProfileType::Heap,
            _ => ProfileType::Cpu,
        };

        let cmd_args = args.args.unwrap_or_default();
        let duration = args.duration_secs.unwrap_or(5);
        let top_limit = args.top_limit.unwrap_or(10);

        match profile_runner::profile_command(
            &self.root,
            &args.command,
            &cmd_args,
            p_type,
            duration,
            top_limit,
            args.output_svg_rel.as_deref(),
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_profile_report(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn profile_runner_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());

        let res = server
            .profile_runner(Parameters(ProfileRunnerArgs {
                command: "git".to_string(),
                args: Some(vec!["status".to_string()]),
                profile_type: Some("cpu".to_string()),
                duration_secs: Some(2),
                top_limit: Some(5),
                output_svg_rel: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Headless Profiler Report"));
    }
}
