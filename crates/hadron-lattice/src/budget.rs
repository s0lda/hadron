use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetStatus {
    Normal,
    Warning(u8),    // e.g. 80% threshold reached
    Throttled(u8),  // 100%+ threshold reached
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetGuardrail {
    pub max_tokens_per_plan: u64,
    pub max_dollars_per_plan: f64,
    pub current_spent_tokens: u64,
    pub current_spent_dollars: f64,
}

impl BudgetGuardrail {
    pub fn new(max_tokens_per_plan: u64, max_dollars_per_plan: f64) -> Self {
        Self {
            max_tokens_per_plan,
            max_dollars_per_plan,
            current_spent_tokens: 0,
            current_spent_dollars: 0.0,
        }
    }

    pub fn record_spend(&mut self, tokens: u64, dollars: f64) {
        self.current_spent_tokens = self.current_spent_tokens.saturating_add(tokens);
        self.current_spent_dollars += dollars;
    }

    pub fn evaluate_status(&self) -> BudgetStatus {
        let token_ratio = if self.max_tokens_per_plan > 0 {
            (self.current_spent_tokens as f64) / (self.max_tokens_per_plan as f64)
        } else {
            0.0
        };

        let dollar_ratio = if self.max_dollars_per_plan > 0.0 {
            self.current_spent_dollars / self.max_dollars_per_plan
        } else {
            0.0
        };

        let max_ratio = token_ratio.max(dollar_ratio);
        let pct = (max_ratio * 100.0).round() as u8;

        if pct >= 100 {
            BudgetStatus::Throttled(pct)
        } else if pct >= 80 {
            BudgetStatus::Warning(pct)
        } else {
            BudgetStatus::Normal
        }
    }

    pub fn is_throttled(&self) -> bool {
        matches!(self.evaluate_status(), BudgetStatus::Throttled(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_guardrails_evaluation() {
        let mut guard = BudgetGuardrail::new(100_000, 2.0);
        assert_eq!(guard.evaluate_status(), BudgetStatus::Normal);

        guard.record_spend(82_000, 1.64);
        assert_eq!(guard.evaluate_status(), BudgetStatus::Warning(82));
        assert!(!guard.is_throttled());

        guard.record_spend(25_000, 0.50);
        assert!(guard.is_throttled());
        assert_eq!(guard.evaluate_status(), BudgetStatus::Throttled(107));
    }
}
