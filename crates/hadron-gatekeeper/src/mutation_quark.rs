//! Continuous Background Mutation Quark (Capability #10).
//!
//! Generates code mutations in background threads, evaluates test suite killing power,
//! and maintains a persistent test resilience ledger.

use crate::mutation::{MutantStatus, MutationEvaluation};
#[cfg(test)]
use crate::mutation::MutantResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationPlan {
    pub file_path: String,
    pub target_operator: String,
    pub replacement_operator: String,
    pub line_number: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MutationQuarkTracker {
    pub cumulative_runs: usize,
    pub total_mutants_generated: usize,
    pub total_mutants_killed: usize,
    pub total_mutants_survived: usize,
    pub file_resilience: HashMap<String, f64>,
}

impl MutationQuarkTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an evaluation run and updates resilience metrics per file.
    pub fn record_evaluation(&mut self, eval: &MutationEvaluation) {
        self.cumulative_runs += 1;
        self.total_mutants_generated += eval.total_mutants;
        self.total_mutants_killed += eval.killed + eval.timed_out;
        self.total_mutants_survived += eval.survived;

        let mut file_results: HashMap<String, (usize, usize)> = HashMap::new();
        for res in &eval.results {
            let entry = file_results.entry(res.file_path.clone()).or_default();
            entry.1 += 1;
            if res.status == MutantStatus::Killed || res.status == MutantStatus::TimedOut {
                entry.0 += 1;
            }
        }

        for (file, (killed, total)) in file_results {
            let score = if total > 0 { (killed as f64 / total as f64) * 100.0 } else { 100.0 };
            self.file_resilience.insert(file, score);
        }
    }

    /// Returns overall test suite mutation score percentage.
    pub fn overall_score_pct(&self) -> f64 {
        let effective = self.total_mutants_killed + self.total_mutants_survived;
        if effective > 0 {
            (self.total_mutants_killed as f64 / effective as f64) * 100.0
        } else {
            100.0
        }
    }
}

/// Generates candidate mutation plans for a given source snippet.
pub fn generate_mutants(file_path: &str, source: &str) -> Vec<MutationPlan> {
    let mut plans = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        let line_num = line_idx + 1;
        if line.contains("==") {
            plans.push(MutationPlan {
                file_path: file_path.to_string(),
                target_operator: "==".to_string(),
                replacement_operator: "!=".to_string(),
                line_number: line_num,
            });
        }
        if line.contains("!=") {
            plans.push(MutationPlan {
                file_path: file_path.to_string(),
                target_operator: "!=".to_string(),
                replacement_operator: "==".to_string(),
                line_number: line_num,
            });
        }
        if line.contains(" && ") {
            plans.push(MutationPlan {
                file_path: file_path.to_string(),
                target_operator: " && ".to_string(),
                replacement_operator: " || ".to_string(),
                line_number: line_num,
            });
        }
        if line.contains(" || ") {
            plans.push(MutationPlan {
                file_path: file_path.to_string(),
                target_operator: " || ".to_string(),
                replacement_operator: " && ".to_string(),
                line_number: line_num,
            });
        }
        if line.contains(" < ") {
            plans.push(MutationPlan {
                file_path: file_path.to_string(),
                target_operator: " < ".to_string(),
                replacement_operator: " <= ".to_string(),
                line_number: line_num,
            });
        }
        if line.contains(" > ") {
            plans.push(MutationPlan {
                file_path: file_path.to_string(),
                target_operator: " > ".to_string(),
                replacement_operator: " >= ".to_string(),
                line_number: line_num,
            });
        }
    }
    plans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutation_quark_generation_and_tracking() {
        let source = r#"
pub fn is_valid(age: i32, active: bool) -> bool {
    if age > 18 && active == true {
        true
    } else {
        false
    }
}
"#;

        let mutants = generate_mutants("src/auth.rs", source);
        assert!(!mutants.is_empty());
        assert!(mutants.iter().any(|m| m.target_operator == " > "));
        assert!(mutants.iter().any(|m| m.target_operator == " && "));
        assert!(mutants.iter().any(|m| m.target_operator == "=="));

        let results = vec![
            MutantResult {
                mutant_id: "mut-1".to_string(),
                file_path: "src/auth.rs".to_string(),
                line_number: 3,
                original_token: ">".to_string(),
                mutated_token: ">=".to_string(),
                status: MutantStatus::Killed,
                test_name_killed_by: Some("test_is_valid".to_string()),
            },
            MutantResult {
                mutant_id: "mut-2".to_string(),
                file_path: "src/auth.rs".to_string(),
                line_number: 3,
                original_token: "==".to_string(),
                mutated_token: "!=".to_string(),
                status: MutantStatus::Survived,
                test_name_killed_by: None,
            },
        ];

        let eval = MutationEvaluation::compute(results, 50.0);
        let mut tracker = MutationQuarkTracker::new();
        tracker.record_evaluation(&eval);

        assert_eq!(tracker.cumulative_runs, 1);
        assert_eq!(tracker.total_mutants_generated, 2);
        assert_eq!(tracker.total_mutants_killed, 1);
        assert_eq!(tracker.total_mutants_survived, 1);
        assert_eq!(tracker.overall_score_pct(), 50.0);
        assert_eq!(tracker.file_resilience.get("src/auth.rs"), Some(&50.0));
    }
}
