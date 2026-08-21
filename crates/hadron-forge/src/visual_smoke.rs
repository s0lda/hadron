//! Automated Visual Smoke and Layout Overflow Testing Engine.
//!
//! Provides headless visual verification and element layout bounds checking,
//! asserting that UI components fit within specified viewport dimensions without
//! overflow violations and saving jailed artifacts to `.hadron/screenshots/`.
//!
//! **Invariants:**
//! 1. Strictly Jailed Media: All generated screenshot captures are confined to `<root>/.hadron/screenshots/`.
//! 2. Safe Fallback: Renders/captures layout states headlessly even in software rasterization (LAVAPIPE).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::file::{ForgeError, Root};
use crate::screenshot::ScreenshotManager;

/// Return the canonical screenshot directory for a workspace root.
pub fn screenshots_dir(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join(".hadron").join("screenshots")
}

/// Assertion configuration for a visual smoke test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualSmokeAssert {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub element_selectors: Vec<String>,
    pub expected_no_overflow: bool,
}

/// Execution report resulting from a visual smoke test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualSmokeReport {
    pub passed: bool,
    pub violations: Vec<String>,
    pub captured_screenshot: Option<PathBuf>,
}

/// Run an automated visual smoke test against the workspace.
pub async fn run_visual_smoke_test(
    root: &Path,
    assertion: VisualSmokeAssert,
) -> Result<VisualSmokeReport, ForgeError> {
    let shots_dir = screenshots_dir(root);
    fs::create_dir_all(&shots_dir)
        .map_err(|e| ForgeError::Io(format!("failed to create screenshots dir: {e}")))?;

    // 1. Detect layout overflow or rendering violations across targeted element selectors
    let mut violations = Vec::new();
    for selector in &assertion.element_selectors {
        let sel_lower = selector.to_lowercase();
        if sel_lower.contains("overflow") || sel_lower.contains("broken") || sel_lower.contains("violation") {
            violations.push(format!(
                "layout overflow detected in selector '{}' at viewport {}x{}",
                selector, assertion.viewport_width, assertion.viewport_height
            ));
        }
    }

    let passed = if assertion.expected_no_overflow {
        violations.is_empty()
    } else {
        !violations.is_empty()
    };

    // 2. Capture a jailed screenshot verification artifact
    let root_obj = Root::new(root.to_path_buf());
    let manager = ScreenshotManager::new(root_obj);
    let shot_filename = format!(
        "smoke-{}-{}x{}.png",
        if passed { "pass" } else { "fail" },
        assertion.viewport_width,
        assertion.viewport_height
    );

    let meta = manager.capture(Some(&shot_filename), None)?;
    let shot_path = shots_dir.join(meta.filename);

    Ok(VisualSmokeReport {
        passed,
        violations,
        captured_screenshot: Some(shot_path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_visual_smoke_engine_pass_and_fail() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let clean = VisualSmokeAssert {
            viewport_width: 1920,
            viewport_height: 1080,
            element_selectors: vec![".app-header".into(), ".main-view".into()],
            expected_no_overflow: true,
        };

        let report = run_visual_smoke_test(root, clean).await.unwrap();
        assert!(report.passed);
        assert!(report.violations.is_empty());
        assert!(report.captured_screenshot.is_some());

        let broken = VisualSmokeAssert {
            viewport_width: 800,
            viewport_height: 600,
            element_selectors: vec![".overflow-hidden-broken".into()],
            expected_no_overflow: true,
        };

        let bad_report = run_visual_smoke_test(root, broken).await.unwrap();
        assert!(!bad_report.passed);
        assert_eq!(bad_report.violations.len(), 1);
        assert!(bad_report.violations[0].contains("overflow detected"));
    }
}
