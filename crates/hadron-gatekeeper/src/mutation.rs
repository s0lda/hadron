use serde::{Deserialize, Serialize};

/// The outcome status of a generated code mutant when subjected to test execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutantStatus {
    /// Mutant caused test failure (desirable: test caught the mutation).
    Killed,
    /// Mutant passed tests without failure (test gap: mutation was undetected).
    Survived,
    /// Mutant caused syntax or compilation failure before tests ran.
    Unbuildable,
    /// Mutant execution exceeded allotted timeout.
    TimedOut,
}

/// A recorded mutation test execution result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutantResult {
    pub mutant_id: String,
    pub file_path: String,
    pub line_number: usize,
    pub original_token: String,
    pub mutated_token: String,
    pub status: MutantStatus,
    pub test_name_killed_by: Option<String>,
}

/// Comprehensive evaluation summary for a mutation testing run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationEvaluation {
    pub total_mutants: usize,
    pub killed: usize,
    pub survived: usize,
    pub unbuildable: usize,
    pub timed_out: usize,
    pub score_pct: f64,
    pub passed_gate: bool,
    pub results: Vec<MutantResult>,
}

impl MutationEvaluation {
    pub fn compute(results: Vec<MutantResult>, minimum_score_pct: f64) -> Self {
        let total = results.len();
        let mut killed = 0;
        let mut survived = 0;
        let mut unbuildable = 0;
        let mut timed_out = 0;

        for r in &results {
            match r.status {
                MutantStatus::Killed => killed += 1,
                MutantStatus::Survived => survived += 1,
                MutantStatus::Unbuildable => unbuildable += 1,
                MutantStatus::TimedOut => timed_out += 1,
            }
        }

        // Mutation score = (killed + timed_out) / (total - unbuildable) * 100
        let effective_mutants = total.saturating_sub(unbuildable);
        let score_pct = if effective_mutants > 0 {
            ((killed + timed_out) as f64 / effective_mutants as f64) * 100.0
        } else {
            100.0
        };

        let passed_gate = score_pct >= minimum_score_pct;

        Self {
            total_mutants: total,
            killed,
            survived,
            unbuildable,
            timed_out,
            score_pct,
            passed_gate,
            results,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutation_evaluation_score() {
        let results = vec![
            MutantResult {
                mutant_id: "m1".into(),
                file_path: "src/lib.rs".into(),
                line_number: 10,
                original_token: ">".into(),
                mutated_token: ">=".into(),
                status: MutantStatus::Killed,
                test_name_killed_by: Some("test_boundary".into()),
            },
            MutantResult {
                mutant_id: "m2".into(),
                file_path: "src/lib.rs".into(),
                line_number: 20,
                original_token: "+".into(),
                mutated_token: "-".into(),
                status: MutantStatus::Survived,
                test_name_killed_by: None,
            },
            MutantResult {
                mutant_id: "m3".into(),
                file_path: "src/lib.rs".into(),
                line_number: 30,
                original_token: "true".into(),
                mutated_token: "false".into(),
                status: MutantStatus::Killed,
                test_name_killed_by: Some("test_flag".into()),
            },
        ];

        let eval = MutationEvaluation::compute(results, 60.0);
        assert_eq!(eval.total_mutants, 3);
        assert_eq!(eval.killed, 2);
        assert_eq!(eval.survived, 1);
        assert!((eval.score_pct - 66.666).abs() < 0.1);
        assert!(eval.passed_gate);
    }
}
