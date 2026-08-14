//! The **process** family: manage background processes and dev servers.
//!
//! Spawns long-running or background processes within the worktree jail, streams logs into
//! memory-bounded ring buffers, and ensures strict process-group teardown upon termination.

use super::{ForgeMcpServer, ToolResponse};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProcessStartArgs {
    /// The allowlisted program to spawn (e.g. `cargo`, `git`).
    pub program: String,
    /// Arguments, already split into individual items (no shell quoting).
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProcessLogsArgs {
    /// The numeric ID of the process returned by `hadron_forge_process_start`.
    pub process_id: u64,
    /// Number of recent lines to retrieve (default: all buffered lines).
    #[serde(default)]
    pub tail_lines: Option<usize>,
    /// Line index offset cursor to stream logs incrementally.
    #[serde(default)]
    pub cursor: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProcessListArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProcessStdinArgs {
    /// The numeric ID of the process.
    pub process_id: u64,
    /// The text/input to send to the process's standard input.
    pub input: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProcessKillArgs {
    /// The numeric ID of the process to terminate.
    pub process_id: u64,
}

#[tool_router(router = process_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_process_start",
        description = "Spawn a background process inside the worktree jail. Returns a unique numeric process ID for log streaming and lifecycle management."
    )]
    pub async fn process_start(
        &self,
        Parameters(args): Parameters<ProcessStartArgs>,
    ) -> Json<ToolResponse> {
        match self
            .process_manager
            .spawn(&args.program, &args.args, None)
            .await
        {
            Ok(id) => Json(ToolResponse::success(Some(format!(
                "Process #{} spawned for `{}` with {} args",
                id,
                args.program,
                args.args.len()
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_process_logs",
        description = "Retrieve stdout, stderr, and lifecycle status for a background process from its ring-buffer."
    )]
    pub async fn process_logs(
        &self,
        Parameters(args): Parameters<ProcessLogsArgs>,
    ) -> Json<ToolResponse> {
        match self
            .process_manager
            .get_logs(args.process_id, args.tail_lines, args.cursor)
            .await
        {
            Ok(logs) => Json(ToolResponse::success(Some(logs))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_process_list",
        description = "List all active and completed background processes managed in the current workspace session."
    )]
    pub async fn process_list(
        &self,
        Parameters(_args): Parameters<ProcessListArgs>,
    ) -> Json<ToolResponse> {
        let procs = self.process_manager.list().await;
        match serde_json::to_string_pretty(&procs) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(format!("failed to serialize processes: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_process_send_stdin",
        description = "Write input text to the standard input of a running background process."
    )]
    pub async fn process_send_stdin(
        &self,
        Parameters(args): Parameters<ProcessStdinArgs>,
    ) -> Json<ToolResponse> {
        match self
            .process_manager
            .send_stdin(args.process_id, &args.input)
            .await
        {
            Ok(()) => Json(ToolResponse::success(Some(format!(
                "Wrote {} bytes to stdin of process #{}",
                args.input.len(),
                args.process_id
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_process_kill",
        description = "Terminate a background process and its whole process group (SIGKILL/SIGTERM)."
    )]
    pub async fn process_kill(
        &self,
        Parameters(args): Parameters<ProcessKillArgs>,
    ) -> Json<ToolResponse> {
        match self.process_manager.kill(args.process_id).await {
            Ok(killed) => Json(ToolResponse::success(Some(format!(
                "Process #{} killed: {}",
                args.process_id, killed
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn mcp_process_tools_start_poll_and_kill() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());

        let start_res = server
            .process_start(Parameters(ProcessStartArgs {
                program: "cargo".into(),
                args: vec!["--version".into()],
            }))
            .await;
        assert!(start_res.0.ok);
        assert!(start_res.0.blocks.as_ref().unwrap().contains("Process #1"));

        tokio::time::sleep(Duration::from_millis(200)).await;

        let logs_res = server
            .process_logs(Parameters(ProcessLogsArgs {
                process_id: 1,
                tail_lines: Some(10),
                cursor: None,
            }))
            .await;
        assert!(logs_res.0.ok);
        assert!(logs_res.0.blocks.as_ref().unwrap().contains("cargo"));

        let list_res = server.process_list(Parameters(ProcessListArgs {})).await;
        assert!(list_res.0.ok);
        assert!(list_res.0.blocks.as_ref().unwrap().contains("cargo"));

        let kill_res = server
            .process_kill(Parameters(ProcessKillArgs { process_id: 1 }))
            .await;
        assert!(kill_res.0.ok);
    }
}
