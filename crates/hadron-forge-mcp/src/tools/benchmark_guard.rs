//! The **benchmark_guard** tool: benchmark parser and regression checker.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::benchmark_guard::BenchmarkForge;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BenchmarkGuardArgs {
    pub bench_output: String,
}

#[tool_router(router = benchmark_guard_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_benchmark_guard",
        description = "Parse benchmark outputs and detect performance regressions across critical hotpaths"
    )]
    pub async fn benchmark_guard(
        &self,
        Parameters(args): Parameters<BenchmarkGuardArgs>,
    ) -> Json<ToolResponse> {
        let results = BenchmarkForge::parse_bench_output(&args.bench_output);
        match serde_json::to_string_pretty(&results) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn benchmark_guard_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .benchmark_guard(Parameters(BenchmarkGuardArgs {
                bench_output: "test bench_foo ... bench: 500 ns/iter (+/- 10)".into(),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("bench_foo"));
    }
}
