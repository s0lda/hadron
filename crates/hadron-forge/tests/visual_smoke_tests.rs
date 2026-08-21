use hadron_forge::visual_smoke::{run_visual_smoke_test, screenshots_dir, VisualSmokeAssert};
use tempfile::tempdir;

#[tokio::test]
async fn test_visual_smoke_layout_overflow_and_screenshot_jailing() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // 1. Test clean smoke run
    let clean_assert = VisualSmokeAssert {
        viewport_width: 1920,
        viewport_height: 1080,
        element_selectors: vec![".chat-container".into(), ".editor-rail".into()],
        expected_no_overflow: true,
    };

    let report = run_visual_smoke_test(root, clean_assert).await.unwrap();
    assert!(report.passed);
    assert!(report.violations.is_empty());
    assert!(report.captured_screenshot.is_some());

    let shot_path = report.captured_screenshot.unwrap();
    assert!(shot_path.starts_with(screenshots_dir(root)), "Screenshot must be jailed in .hadron/screenshots/");
    assert!(shot_path.exists());

    // 2. Test overflow violation detection
    let broken_assert = VisualSmokeAssert {
        viewport_width: 800,
        viewport_height: 600,
        element_selectors: vec![".overflow-hidden-broken".into()],
        expected_no_overflow: true,
    };

    let bad_report = run_visual_smoke_test(root, broken_assert).await.unwrap();
    assert!(!bad_report.passed);
    assert_eq!(bad_report.violations.len(), 1);
    assert!(bad_report.violations[0].contains("overflow detected"));
}
