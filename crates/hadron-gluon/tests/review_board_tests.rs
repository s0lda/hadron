use hadron_gluon::skills::review_board::*;

#[test]
fn test_peer_review_board_verdict_and_annotations() {
    let annotations = vec![
        LineAnnotation {
            file: "crates/hadron-gluon/src/engine.rs".into(),
            line: 120,
            comment: "Potential deadlock in nested mutex acquisition".into(),
            severity: "error".into(),
        },
        LineAnnotation {
            file: "crates/hadron-chamber/src/app/actions.rs".into(),
            line: 45,
            comment: "Consider extracting common helper function".into(),
            severity: "info".into(),
        },
    ];

    let aspects = vec![
        AspectResult {
            aspect: ReviewAspect::Security,
            passed: true,
            score: 100,
            summary: "No vulnerability found".into(),
            annotations: vec![],
        },
        AspectResult {
            aspect: ReviewAspect::Architecture,
            passed: false,
            score: 40,
            summary: "Violates non-blocking invariant".into(),
            annotations: vec![annotations[0].clone()],
        },
        AspectResult {
            aspect: ReviewAspect::CodeSimplicity,
            passed: true,
            score: 75,
            summary: "Acceptable complexity".into(),
            annotations: vec![annotations[1].clone()],
        },
    ];

    let verdict = ReviewBoardVerdict::new(true, false, 75, annotations, aspects);
    assert_eq!(verdict.final_verdict, ReviewVerdict::Blocked);
    let md = verdict.to_markdown();
    assert!(md.contains("BLOCKED"));
    assert!(md.contains("Potential deadlock in nested mutex acquisition"));
}
