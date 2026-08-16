//! The **binary_bloat** family: binary footprint and section overhead analyzer.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::binary_bloat::{self, BinaryBloatReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BinaryBloatArgs {
    pub binary_path: String,
    pub compare_path: Option<String>,
}

fn format_binary_bloat(report: BinaryBloatReport) -> String {
    let mut out = format!("### Binary Bloat Report\n\n{}\n\n", report.summary);
    out.push_str(&format!("- **Binary:** `{}`\n", report.binary_path));
    out.push_str(&format!("- **Total File Size:** {} bytes ({:.2} MB)\n", report.total_file_size_bytes, report.total_file_size_bytes as f64 / (1024.0 * 1024.0)));
    if let Some(delta) = report.comparison_delta_bytes {
        out.push_str(&format!("- **Delta vs Baseline:** {:+} bytes\n", delta));
    }
    out.push('\n');

    out.push_str("#### Section Size Distribution:\n");
    for s in report.sections {
        out.push_str(&format!(
            "- `{}`: {:.1}% ({} bytes)\n",
            s.name, s.percentage, s.size_bytes
        ));
    }
    out
}

#[tool_router(router = binary_bloat_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_binary_bloat",
        description = "Inspect executable binary footprint, section sizes (.text, .rodata, .data), and track binary bloat regressions"
    )]
    pub async fn binary_bloat(
        &self,
        Parameters(args): Parameters<BinaryBloatArgs>,
    ) -> Json<ToolResponse> {
        match binary_bloat::inspect_binary_bloat(
            &self.root,
            &args.binary_path,
            args.compare_path.as_deref(),
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_binary_bloat(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn binary_bloat_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let bin_file = dir.path().join("dummy.bin");
        std::fs::write(&bin_file, b"sample binary payload").unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .binary_bloat(Parameters(BinaryBloatArgs {
                binary_path: "dummy.bin".to_string(),
                compare_path: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Binary Bloat"));
    }
}
