//! The **fuzz_harness** family: property-based fuzz test generation.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::fuzz_harness::{self, FuzzHarnessReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FuzzHarnessArgs {
    pub target_format: String,
    pub iterations: Option<usize>,
}

fn format_fuzz_harness(report: FuzzHarnessReport) -> String {
    let mut out = format!("### Fuzz Harness Generator\n\n{}\n\n", report.summary);
    out.push_str("#### Generated Fuzz Test Vectors:\n");
    for case in report.generated_cases {
        out.push_str(&format!(
            "- Case #{}: `{}`\n  ```\n  {}\n  ```\n",
            case.iteration, case.mutation_kind, case.payload_preview
        ));
    }
    out
}

#[tool_router(router = fuzz_harness_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_fuzz_harness",
        description = "Generate adversarial property-based fuzz test mutations for parsers, serializers, and protocols"
    )]
    pub async fn fuzz_harness(
        &self,
        Parameters(args): Parameters<FuzzHarnessArgs>,
    ) -> Json<ToolResponse> {
        match fuzz_harness::run_fuzz_harness(
            &self.root,
            &args.target_format,
            args.iterations,
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_fuzz_harness(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fuzz_harness_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .fuzz_harness(Parameters(FuzzHarnessArgs {
                target_format: "json".to_string(),
                iterations: Some(3),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Fuzz Harness"));
    }
}
