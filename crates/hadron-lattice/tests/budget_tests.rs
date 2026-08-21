use hadron_lattice::budget::*;

#[test]
fn test_elastic_quota_and_budget_guardrails() {
    let mut guardrail = BudgetGuardrail::new(500_000, 10.0);
    assert_eq!(guardrail.evaluate_status(), BudgetStatus::Normal);
    assert!(!guardrail.is_throttled());

    // Record 410k tokens (82%) -> Warning
    guardrail.record_spend(410_000, 8.2);
    assert_eq!(guardrail.evaluate_status(), BudgetStatus::Warning(82));
    assert!(!guardrail.is_throttled());

    // Record another 100k tokens (102%) -> Throttled
    guardrail.record_spend(100_000, 2.0);
    assert!(guardrail.is_throttled());
    assert_eq!(guardrail.evaluate_status(), BudgetStatus::Throttled(102));
}
