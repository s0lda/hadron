use hadron_gluon::skills::grill_me::*;

#[test]
fn test_grill_me_interview_and_spec_synthesis() {
    let mut interview = GrillInterviewState::new("Vectorized Nucleus Search");
    assert!(!interview.is_complete(2));

    interview.ask_question("Should we use SQLite FTS5 or an in-memory TF-IDF index?");
    interview.answer_question("Use an in-memory TF-IDF index over .hadron/nucleus/notes/ for zero external dependencies.");

    interview.ask_question("What is the max candidate limit for search results?");
    interview.answer_question("Default to 5 most relevant notes.");

    assert!(interview.is_complete(2));
    let spec = interview.synthesize_spec();
    assert!(spec.contains("# Specification: Vectorized Nucleus Search"));
    assert!(spec.contains("in-memory TF-IDF index"));
    assert!(spec.contains("Default to 5 most relevant notes"));
}
