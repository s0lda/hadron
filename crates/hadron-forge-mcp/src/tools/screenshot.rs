//! The **screenshot** family: jailed desktop and window screen capture.
//!
//! Provides visual capture tools strictly confined to `<repo_root>/.hadron/screenshots/`,
//! allowing agents to capture and verify graphical UI without risk of PII leakage.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::screenshot::{Region, ScreenshotManager, ScreenshotTarget};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScreenshotCaptureArgs {
    /// Optional filename for the screenshot (default: capture-<timestamp>.png).
    #[serde(default)]
    pub filename: Option<String>,
    /// Optional window title substring to capture.
    #[serde(default)]
    pub window_title: Option<String>,
    /// Optional process PID whose window should be captured.
    #[serde(default)]
    pub pid: Option<u32>,
    /// Optional rectangular region [x, y, width, height] to capture.
    #[serde(default)]
    pub region: Option<[u32; 4]>,
    /// Optional display index.
    #[serde(default)]
    pub display: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScreenshotPruneArgs {
    /// Prune captures older than this many minutes. If omitted, prunes all screenshots.
    #[serde(default)]
    pub older_than_mins: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ScreenshotResult {
    pub path: String,
    pub filename: String,
    pub width: u32,
    pub height: u32,
    pub byte_size: u64,
    pub timestamp_ms: u64,
    pub format: String,
}

#[tool_router(router = screenshot_router, vis = "pub(super)")]
impl ForgeMcpServer {
    /// Capture a desktop, window, or region screenshot saved into `.hadron/screenshots/`.
    #[tool(
        name = "hadron_forge_screenshot_capture",
        description = "Capture a desktop, window, or region screenshot. The image is strictly saved to .hadron/screenshots/."
    )]
    pub async fn screenshot_capture(
        &self,
        Parameters(args): Parameters<ScreenshotCaptureArgs>,
    ) -> Json<ToolResponse> {
        let manager = ScreenshotManager::new(self.root.clone());

        let target = if let Some(title) = args.window_title {
            Some(ScreenshotTarget::WindowTitle(title))
        } else if let Some(pid) = args.pid {
            Some(ScreenshotTarget::Pid(pid))
        } else if let Some([x, y, width, height]) = args.region {
            Some(ScreenshotTarget::Region(Region {
                x,
                y,
                width,
                height,
            }))
        } else if let Some(display) = args.display {
            Some(ScreenshotTarget::Display(Some(display)))
        } else {
            None
        };

        match manager.capture(args.filename.as_deref(), target) {
            Ok(meta) => {
                let res = ScreenshotResult {
                    path: meta.path,
                    filename: meta.filename,
                    width: meta.width,
                    height: meta.height,
                    byte_size: meta.byte_size,
                    timestamp_ms: meta.timestamp_ms,
                    format: meta.format,
                };
                match serde_json::to_string_pretty(&res) {
                    Ok(json) => Json(ToolResponse::success(Some(json))),
                    Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
                }
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// List all screenshots currently stored in `.hadron/screenshots/`.
    #[tool(
        name = "hadron_forge_screenshot_list",
        description = "List all screenshots currently stored in the .hadron/screenshots/ directory."
    )]
    pub async fn screenshot_list(&self) -> Json<ToolResponse> {
        let manager = ScreenshotManager::new(self.root.clone());
        match manager.list() {
            Ok(list) => match serde_json::to_string_pretty(&list) {
                Ok(json) => Json(ToolResponse::success(Some(json))),
                Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
            },
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Prune screenshots in `.hadron/screenshots/` older than specified age or purge all.
    #[tool(
        name = "hadron_forge_screenshot_prune",
        description = "Prune old screenshots in .hadron/screenshots/ to reclaim disk space."
    )]
    pub async fn screenshot_prune(
        &self,
        Parameters(args): Parameters<ScreenshotPruneArgs>,
    ) -> Json<ToolResponse> {
        let manager = ScreenshotManager::new(self.root.clone());
        match manager.prune(args.older_than_mins) {
            Ok(count) => Json(ToolResponse::success(Some(format!(
                "Successfully pruned {count} screenshot(s)"
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn screenshot_router_handles_capture_list_and_prune() {
        let temp = tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path().to_path_buf());

        // Capture
        let cap_res = server
            .screenshot_capture(Parameters(ScreenshotCaptureArgs {
                filename: Some("ui-test.png".into()),
                window_title: None,
                pid: None,
                region: None,
                display: None,
            }))
            .await;
        assert!(cap_res.0.ok);
        let blocks = cap_res.0.blocks.unwrap();
        assert!(blocks.contains("ui-test.png"));

        // List
        let list_res = server.screenshot_list().await;
        assert!(list_res.0.ok);
        assert!(list_res.0.blocks.unwrap().contains("ui-test.png"));

        // Prune
        let prune_res = server
            .screenshot_prune(Parameters(ScreenshotPruneArgs {
                older_than_mins: None,
            }))
            .await;
        assert!(prune_res.0.ok);
        assert!(prune_res.0.blocks.unwrap().contains("pruned 1"));
    }
}
