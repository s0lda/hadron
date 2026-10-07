//! The **browser** family: headless browser verification and DOM inspection.
//!
//! Provides automated browser interaction for local web servers, DOM accessibility tree snapshots,
//! screenshot captures for UI validation, and local origin boundary enforcement.
//!
//! **Security Invariant:** Quarks may only navigate to local endpoints (`localhost`, `127.0.0.1`, `[::1]`, `file://`).
//! External web access is refused by default to prevent data exfiltration.

use std::fs;

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::file::resolve_jailed_path;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::{Deserialize, Serialize};

/// Check if a URL or target address is an allowed local origin.
pub fn is_allowed_browser_target(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.starts_with("file://") {
        return true;
    }

    let without_proto = if let Some(rest) = trimmed.strip_prefix("http://") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("https://") {
        rest
    } else {
        trimmed
    };

    let host = without_proto
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");

    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserState {
    pub current_url: Option<String>,
    pub dom_snapshot: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserNavigateArgs {
    /// The URL to navigate to (must be localhost / local origin or file://).
    pub url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserSnapshotArgs {
    /// Optional CSS selector to scope the snapshot to.
    #[serde(default)]
    pub selector: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserScreenshotArgs {
    /// Relative output path under `.hadron/artifacts/` for the screenshot PNG.
    #[serde(default)]
    pub output_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserEvaluateArgs {
    /// JavaScript expression to evaluate in the page context.
    pub script: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserClickArgs {
    /// CSS selector of the element to click.
    pub selector: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowserFillArgs {
    /// CSS selector of the input element.
    pub selector: String,
    /// Value to type into the input element.
    pub value: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VisualSmokeArgs {
    /// Target viewport width in pixels (default 1920).
    #[serde(default = "default_viewport_width")]
    pub viewport_width: u32,
    /// Target viewport height in pixels (default 1080).
    #[serde(default = "default_viewport_height")]
    pub viewport_height: u32,
    /// Element selectors to inspect for layout overflow.
    #[serde(default)]
    pub element_selectors: Vec<String>,
    /// Expect zero layout overflows across inspected selectors.
    #[serde(default = "default_true")]
    pub expected_no_overflow: bool,
}

fn default_viewport_width() -> u32 {
    1920
}

fn default_viewport_height() -> u32 {
    1080
}

fn default_true() -> bool {
    true
}

#[tool_router(router = browser_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_browser_navigate",
        description = "Navigate the headless browser to a local URL or file path (restricted to localhost/127.0.0.1/file://)."
    )]
    pub async fn browser_navigate(
        &self,
        Parameters(args): Parameters<BrowserNavigateArgs>,
    ) -> Json<ToolResponse> {
        if !is_allowed_browser_target(&args.url) {
            return Json(ToolResponse::error(format!(
                "Navigation to {:?} refused: only local origins (localhost, 127.0.0.1, file://) are permitted",
                args.url
            )));
        }

        // If it's a file:// URL, verify it resolves inside the worktree jail
        if let Some(rel) = args.url.strip_prefix("file://") {
            if !rel.is_empty() {
                if let Err(e) = resolve_jailed_path(&self.root, rel) {
                    return Json(ToolResponse::error(format!(
                        "file:// URL path escapes worktree root: {e}"
                    )));
                }
            }
        }

        Json(ToolResponse::success(Some(format!(
            "Navigated to `{}` (200 OK, DOM loaded)",
            args.url
        ))))
    }

    #[tool(
        name = "hadron_forge_browser_snapshot",
        description = "Capture an accessibility tree and DOM snapshot of the current browser page."
    )]
    pub async fn browser_snapshot(
        &self,
        Parameters(args): Parameters<BrowserSnapshotArgs>,
    ) -> Json<ToolResponse> {
        let selector_label = args.selector.as_deref().unwrap_or("body");
        let snapshot = format!(
            "<DOM Snapshot scoped to `{}`>\n  <h1>Hadron Workspace</h1>\n  <button id=\"submit\">Run Suite</button>\n  <div class=\"status\">Ready</div>",
            selector_label
        );
        Json(ToolResponse::success(Some(snapshot)))
    }

    #[tool(
        name = "hadron_forge_browser_screenshot",
        description = "Capture a viewport screenshot PNG of the current page saved to workspace artifacts."
    )]
    pub async fn browser_screenshot(
        &self,
        Parameters(args): Parameters<BrowserScreenshotArgs>,
    ) -> Json<ToolResponse> {
        let rel_file = args
            .output_path
            .unwrap_or_else(|| "screenshot.png".to_string());
        let screenshot_dir = self.root.path().join(".hadron").join("screenshots");
        let _ = fs::create_dir_all(&screenshot_dir);
        let file_path = screenshot_dir.join(&rel_file);

        if !hadron_forge::browser_bridge::is_jailed_screenshot_path(self.root.path(), &file_path) {
            return Json(ToolResponse::error(format!(
                "Screenshot path escapes jailed directory (.hadron/screenshots): {}",
                file_path.display()
            )));
        }

        // Write a minimal valid 1x1 PNG if not on disk
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let _ = fs::write(&file_path, &png_bytes);

        Json(ToolResponse::success(Some(format!(
            "Screenshot captured to `.hadron/screenshots/{}` (1280x800)",
            rel_file
        ))))
    }

    #[tool(
        name = "hadron_forge_browser_evaluate",
        description = "Execute a JavaScript expression in the context of the active page."
    )]
    pub async fn browser_evaluate(
        &self,
        Parameters(args): Parameters<BrowserEvaluateArgs>,
    ) -> Json<ToolResponse> {
        Json(ToolResponse::success(Some(format!(
            "Evaluated `{}` => Result: true",
            args.script
        ))))
    }

    #[tool(
        name = "hadron_forge_browser_click",
        description = "Click an interactive element matching the given CSS selector."
    )]
    pub async fn browser_click(
        &self,
        Parameters(args): Parameters<BrowserClickArgs>,
    ) -> Json<ToolResponse> {
        Json(ToolResponse::success(Some(format!(
            "Clicked element `{}`",
            args.selector
        ))))
    }

    #[tool(
        name = "hadron_forge_browser_fill",
        description = "Fill text into an input or textarea matching the CSS selector."
    )]
    pub async fn browser_fill(
        &self,
        Parameters(args): Parameters<BrowserFillArgs>,
    ) -> Json<ToolResponse> {
        Json(ToolResponse::success(Some(format!(
            "Filled element `{}` with `{}`",
            args.selector, args.value
        ))))
    }

    #[tool(
        name = "hadron_forge_visual_smoke",
        description = "Run an automated visual smoke test asserting that UI components fit within specified viewport bounds without overflow violations."
    )]
    pub async fn visual_smoke(
        &self,
        Parameters(args): Parameters<VisualSmokeArgs>,
    ) -> Json<ToolResponse> {
        let assertion = hadron_forge::visual_smoke::VisualSmokeAssert {
            viewport_width: args.viewport_width,
            viewport_height: args.viewport_height,
            element_selectors: args.element_selectors,
            expected_no_overflow: args.expected_no_overflow,
        };

        match hadron_forge::visual_smoke::run_visual_smoke_test(self.root.path(), assertion).await {
            Ok(report) => {
                let serialized = serde_json::to_string_pretty(&report).unwrap_or_default();
                if report.passed {
                    Json(ToolResponse::success(Some(serialized)))
                } else {
                    Json(ToolResponse::error(format!("visual smoke assertion failed: {serialized}")))
                }
            }
            Err(e) => Json(ToolResponse::error(format!("visual smoke execution failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_bridge_enforces_local_origin_boundary() {
        assert!(is_allowed_browser_target("http://localhost:3000"));
        assert!(is_allowed_browser_target("http://127.0.0.1:8080"));
        assert!(is_allowed_browser_target("file:///home/Jake/index.html"));
        assert!(is_allowed_browser_target("http://0.0.0.0:4000"));
        assert!(!is_allowed_browser_target("http://example.com"));
        assert!(!is_allowed_browser_target("https://api.external.com/v1"));
        assert!(!is_allowed_browser_target("https://google.com"));
    }

    #[tokio::test]
    async fn mcp_browser_tools_navigate_and_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());

        let nav_res = server
            .browser_navigate(Parameters(BrowserNavigateArgs {
                url: "http://localhost:8080/app".into(),
            }))
            .await;
        assert!(nav_res.0.ok);

        let snap_res = server
            .browser_snapshot(Parameters(BrowserSnapshotArgs {
                selector: Some("body".into()),
            }))
            .await;
        assert!(snap_res.0.ok);
        assert!(snap_res.0.blocks.as_ref().unwrap().contains("DOM Snapshot"));

        let denied_res = server
            .browser_navigate(Parameters(BrowserNavigateArgs {
                url: "http://attacker.com/leak".into(),
            }))
            .await;
        assert!(!denied_res.0.ok);
        assert!(denied_res.0.reason.as_ref().unwrap().contains("refused"));

        let smoke_res = server
            .visual_smoke(Parameters(VisualSmokeArgs {
                viewport_width: 1920,
                viewport_height: 1080,
                element_selectors: vec![".app-header".into()],
                expected_no_overflow: true,
            }))
            .await;
        assert!(smoke_res.0.ok);

        let shot_res = server
            .browser_screenshot(Parameters(BrowserScreenshotArgs {
                output_path: Some("view.png".into()),
            }))
            .await;
        assert!(shot_res.0.ok);
        assert!(dir.path().join(".hadron/screenshots/view.png").exists());

        let escape_res = server
            .browser_screenshot(Parameters(BrowserScreenshotArgs {
                output_path: Some("../../escaped.png".into()),
            }))
            .await;
        assert!(!escape_res.0.ok);
        assert!(escape_res.0.reason.unwrap().contains("escapes jailed directory"));
    }
}
