//! The **pty** family: interactive pseudo-terminal manager.
//!
//! Spawns interactive CLI tools, TUI apps, REPLs, and terminal prompts within
//! real pseudo-terminals, preserving ANSI sequences, window sizing, and raw keystroke input.

use std::collections::HashMap;

use super::{ForgeMcpServer, ToolResponse};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PtyStartArgs {
    /// The allowlisted program to spawn (e.g. `cargo`, `git`, `python3`, `node`).
    pub program: String,
    /// Arguments, already split into individual items (no shell quoting).
    #[serde(default)]
    pub args: Vec<String>,
    /// Terminal columns (default: 80).
    #[serde(default)]
    pub cols: Option<u16>,
    /// Terminal rows (default: 24).
    #[serde(default)]
    pub rows: Option<u16>,
    /// Optional environment variables.
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PtyWriteArgs {
    /// The numeric ID of the PTY session.
    pub pty_id: u64,
    /// The text, input, or control characters to send to the PTY.
    pub input: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PtyReadArgs {
    /// The numeric ID of the PTY session.
    pub pty_id: u64,
    /// Whether to strip ANSI escape codes from output (default: false).
    #[serde(default)]
    pub strip_ansi: Option<bool>,
    /// Number of recent bytes to retrieve from the tail.
    #[serde(default)]
    pub tail_bytes: Option<usize>,
    /// Byte offset cursor to read incrementally.
    #[serde(default)]
    pub cursor: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PtyResizeArgs {
    /// The numeric ID of the PTY session.
    pub pty_id: u64,
    /// New terminal columns.
    pub cols: u16,
    /// New terminal rows.
    pub rows: u16,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PtyKillArgs {
    /// The numeric ID of the PTY session to terminate.
    pub pty_id: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PtyListArgs {}

#[tool_router(router = pty_router, vis = "pub(super)")]
impl ForgeMcpServer {
    /// Spawn an interactive PTY session inside the worktree jail.
    #[tool(
        name = "hadron_forge_pty_start",
        description = "Spawn an interactive process in a real pseudo-terminal (PTY) with terminal dimensions, raw stdin, and ANSI output support."
    )]
    pub async fn pty_start(
        &self,
        Parameters(args): Parameters<PtyStartArgs>,
    ) -> Json<ToolResponse> {
        match self
            .pty_manager
            .spawn(
                &args.program,
                &args.args,
                args.cols,
                args.rows,
                args.env,
            )
            .await
        {
            Ok(id) => Json(ToolResponse::success(Some(format!(
                "PTY session started with ID: {id}"
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Write input or control characters to an active PTY session.
    #[tool(
        name = "hadron_forge_pty_write",
        description = "Send raw input, keystrokes, or control characters to an active PTY session."
    )]
    pub async fn pty_write(
        &self,
        Parameters(args): Parameters<PtyWriteArgs>,
    ) -> Json<ToolResponse> {
        match self.pty_manager.write(args.pty_id, &args.input).await {
            Ok(n) => Json(ToolResponse::success(Some(format!(
                "Wrote {n} byte(s) to PTY session {}",
                args.pty_id
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Read output buffer from an active PTY session.
    #[tool(
        name = "hadron_forge_pty_read",
        description = "Read output from an interactive PTY session with optional ANSI stripping and cursor pagination."
    )]
    pub async fn pty_read(
        &self,
        Parameters(args): Parameters<PtyReadArgs>,
    ) -> Json<ToolResponse> {
        let strip = args.strip_ansi.unwrap_or(false);
        match self
            .pty_manager
            .read(args.pty_id, strip, args.tail_bytes, args.cursor)
            .await
        {
            Ok(res) => match serde_json::to_string_pretty(&res) {
                Ok(json) => Json(ToolResponse::success(Some(json))),
                Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
            },
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Resize terminal rows and columns for an active PTY session.
    #[tool(
        name = "hadron_forge_pty_resize",
        description = "Resize terminal dimensions (cols and rows) for an active PTY session."
    )]
    pub async fn pty_resize(
        &self,
        Parameters(args): Parameters<PtyResizeArgs>,
    ) -> Json<ToolResponse> {
        match self
            .pty_manager
            .resize(args.pty_id, args.cols, args.rows)
            .await
        {
            Ok(()) => Json(ToolResponse::success(Some(format!(
                "PTY session {} resized to {}x{}",
                args.pty_id, args.cols, args.rows
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Terminate an active PTY session and its process group.
    #[tool(
        name = "hadron_forge_pty_kill",
        description = "Terminate an active PTY session and kill its entire process group."
    )]
    pub async fn pty_kill(
        &self,
        Parameters(args): Parameters<PtyKillArgs>,
    ) -> Json<ToolResponse> {
        match self.pty_manager.kill(args.pty_id).await {
            Ok(true) => Json(ToolResponse::success(Some(format!(
                "PTY session {} terminated",
                args.pty_id
            )))),
            Ok(false) => Json(ToolResponse::success(Some(format!(
                "PTY session {} was not running",
                args.pty_id
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// List all tracked PTY sessions.
    #[tool(
        name = "hadron_forge_pty_list",
        description = "List all tracked interactive PTY sessions and their runtime status."
    )]
    pub async fn pty_list(
        &self,
        _args: Parameters<PtyListArgs>,
    ) -> Json<ToolResponse> {
        let list = self.pty_manager.list().await;
        match serde_json::to_string_pretty(&list) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn pty_router_tools_lifecycle() {
        let temp = tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path().to_path_buf());

        let start_res = server
            .pty_start(Parameters(PtyStartArgs {
                program: "cargo".into(),
                args: vec!["--version".into()],
                cols: Some(80),
                rows: Some(24),
                env: None,
            }))
            .await;
        assert!(start_res.0.ok);

        tokio::time::sleep(Duration::from_millis(300)).await;

        let read_res = server
            .pty_read(Parameters(PtyReadArgs {
                pty_id: 1,
                strip_ansi: Some(true),
                tail_bytes: None,
                cursor: None,
            }))
            .await;
        assert!(read_res.0.ok);
        assert!(read_res.0.blocks.unwrap().contains("cargo"));

        let list_res = server.pty_list(Parameters(PtyListArgs {})).await;
        assert!(list_res.0.ok);

        let resize_res = server
            .pty_resize(Parameters(PtyResizeArgs {
                pty_id: 1,
                cols: 120,
                rows: 40,
            }))
            .await;
        assert!(resize_res.0.ok);

        let kill_res = server
            .pty_kill(Parameters(PtyKillArgs { pty_id: 1 }))
            .await;
        assert!(kill_res.0.ok);
    }
}
