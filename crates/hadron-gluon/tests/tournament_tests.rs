use hadron_gluon::tournament::*;

#[test]
fn test_tournament_spec_winner_selection() {
    let mut spec = TournamentSpec::new("Optimize render loop for Lavapipe", 3);
    spec.add_candidate_branch("quark/worker-1/approach-a");
    spec.add_candidate_branch("quark/worker-2/approach-b");
    spec.add_candidate_branch("quark/worker-3/approach-c");

    assert_eq!(spec.candidate_count, 3);
    assert_eq!(spec.branches.len(), 3);

    let candidates = vec![
        CandidateResult {
            branch: "quark/worker-1/approach-a".into(),
            gate_passed: true,
            test_duration_ms: 2500,
            diff_lines: 80,
        },
        CandidateResult {
            branch: "quark/worker-2/approach-b".into(),
            gate_passed: true,
            test_duration_ms: 1100,
            diff_lines: 24,
        },
        CandidateResult {
            branch: "quark/worker-3/approach-c".into(),
            gate_passed: false,
            test_duration_ms: 400,
            diff_lines: 15,
        },
    ];

    let winner_report = TournamentSpec::evaluate_winner(&candidates);
    assert_eq!(
        winner_report.winner_branch.as_deref(),
        Some("quark/worker-2/approach-b")
    );
    assert!(winner_report.reason.contains("24 lines"));
}

#[test]
fn test_speculative_tournament_prefers_minimal_diff_and_green_gate() {
    let candidates = vec![
        CandidateResult {
            branch: "feat/fast-model".to_string(),
            gate_passed: true,
            diff_lines: 35,
            test_duration_ms: 1200,
        },
        CandidateResult {
            branch: "feat/frontier-model".to_string(),
            gate_passed: true,
            diff_lines: 120,
            test_duration_ms: 8500,
        },
    ];
    let report = TournamentSpec::evaluate_winner(&candidates);
    assert_eq!(report.winner_branch.as_deref(), Some("feat/fast-model"));

    let bakeoff_report = hadron_gluon::bakeoff::BakeOffManager::speculative_tournament_winner(&candidates);
    assert_eq!(bakeoff_report.winner_branch.as_deref(), Some("feat/fast-model"));
}

