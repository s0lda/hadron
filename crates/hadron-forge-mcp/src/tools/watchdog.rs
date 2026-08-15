//! The **watchdog** family: background service health, crash diagnostics, and self-healing remediator.
//!
//! Exposes `hadron_forge_service_watchdog` to monitor background preview and service processes,
//! scan logs for panics, tracebacks, and port collisions, and generate self-healing recovery hints.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::watchdog::{check_service_health, WatchdogConfig};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WatchdogArgs {
    /// Optional process ID to monitor (defaults to first running background service).
    #[serde(default)]
    pub process_id: Option<u64>,
    /// Optional HTTP health URL to probe (e.g. "http://127.0.0.1:8080/").
    #[serde(default)]
    pub health_url: Option<String>,
    /// Number of error occurrences before flagging degraded health (defaults to 3).
    #[serde(default)]
    pub error_burst_threshold: Option<usize>,
    /// Probe timeout in milliseconds (defaults to 2000ms).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[tool_router(router = watchdog_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_service_watchdog",
        description = "Monitor background service processes, detect crash traces, panics, unhandled exceptions, port conflicts, and probe local health endpoints with self-healing advice."
    )]
    pub async fn service_watchdog(
        &self,
        Parameters(args): Parameters<WatchdogArgs>,
    ) -> Json<ToolResponse> {
        let config = WatchdogConfig {
            process_id: args.process_id,
            health_url: args.health_url,
            error_burst_threshold: args.error_burst_threshold,
            timeout_ms: args.timeout_ms,
        };

        match check_service_health(&self.process_manager, &config).await {
            Ok(report) => {
                let json = serde_json::to_string_pretty(&report)
                    .unwrap_or_else(|_| report.summary.clone());
                Json(ToolResponse::success(Some(json)))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn watchdog_tool_inspects_services() {
        let temp = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path());

        let res = server
            .service_watchdog(Parameters(WatchdogArgs {
                process_id: None,
                health_url: None,
                error_burst_threshold: Some(3),
                timeout_ms: Some(500),
            }))
            .await;

        assert!(res.0.ok);
        let blocks = res.0.blocks.unwrap();
        assert!(blocks.contains("status"));
    }
}
