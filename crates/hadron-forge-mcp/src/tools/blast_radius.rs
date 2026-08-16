//! The **blast_radius** family: static impact analysis and affected test suites.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::blast_radius::{self, BlastRadiusReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BlastRadiusArgs {
    pub since_ref: Option<String>,
    pub files: Option<Vec<String>>,
}

fn format_blast_radius(report: BlastRadiusReport) -> String {
    let mut out = format!("### Blast Radius Analysis\n\n{}\n\n", report.summary);
    if !report.changed_files.is_empty() {
        out.push_str("#### Modified Files:\n");
        for f in report.changed_files {
            out.push_str(&format!("- `{}`\n", f));
        }
        out.push('\n');
    }
    if !report.direct_crates.is_empty() {
        out.push_str("#### Directly Impacted Crates:\n");
        for c in report.direct_crates {
            out.push_str(&format!("- `{}`\n", c));
        }
        out.push('\n');
    }
    if !report.downstream_crates.is_empty() {
        out.push_str("#### Downstream Dependents:\n");
        for c in report.downstream_crates {
            out.push_str(&format!("- `{}`\n", c));
        }
        out.push('\n');
    }
    if !report.impacted_test_targets.is_empty() {
        out.push_str("#### Recommended Test Commands:\n");
        for t in report.impacted_test_targets {
            out.push_str(&format!("- `{}`\n", t));
        }
    }
    out
}

#[tool_router(router = blast_radius_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_blast_radius",
        description = "Analyze workspace blast radius for modified files, mapping direct crates, downstream dependents, and affected test suites"
    )]
    pub async fn blast_radius(
        &self,
        Parameters(args): Parameters<BlastRadiusArgs>,
    ) -> Json<ToolResponse> {
        match blast_radius::analyze_blast_radius(
            &self.root,
            args.since_ref.as_deref(),
            args.files,
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_blast_radius(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blast_radius_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .blast_radius(Parameters(BlastRadiusArgs {
                since_ref: None,
                files: Some(vec!["crates/foo/src/lib.rs".to_string()]),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Blast Radius"));
    }
}
