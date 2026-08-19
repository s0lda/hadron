//! Speculative Dual-Execution Gate (Capability #1).
//!
//! Races two parallel worktree candidates through gate verification and selects the winner.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateExecution {
    pub candidate_id: String,
    pub branch: String,
    pub passed_tests: bool,
    pub duration_ms: u64,
    pub lines_changed: usize,
    pub memory_peak_mb: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeculativeWinner {
    Winner {
        candidate_id: String,
        reason: String,
    },
    Tie,
    AllFailed,
}

/// Evaluates two competing candidate executions and selects the superior implementation.
pub fn select_speculative_winner(
    candidate_a: &CandidateExecution,
    candidate_b: &CandidateExecution,
) -> SpeculativeWinner {
    match (candidate_a.passed_tests, candidate_b.passed_tests) {
        (true, false) => SpeculativeWinner::Winner {
            candidate_id: candidate_a.candidate_id.clone(),
            reason: "Passed verification tests while competitor failed".to_string(),
        },
        (false, true) => SpeculativeWinner::Winner {
            candidate_id: candidate_b.candidate_id.clone(),
            reason: "Passed verification tests while competitor failed".to_string(),
        },
        (false, false) => SpeculativeWinner::AllFailed,
        (true, true) => {
            // Both passed: choose faster execution, or fewer lines of churn
            if candidate_a.duration_ms + 100 < candidate_b.duration_ms {
                SpeculativeWinner::Winner {
                    candidate_id: candidate_a.candidate_id.clone(),
                    reason: format!(
                        "Completed in {}ms vs {}ms",
                        candidate_a.duration_ms, candidate_b.duration_ms
                    ),
                }
            } else if candidate_b.duration_ms + 100 < candidate_a.duration_ms {
                SpeculativeWinner::Winner {
                    candidate_id: candidate_b.candidate_id.clone(),
                    reason: format!(
                        "Completed in {}ms vs {}ms",
                        candidate_b.duration_ms, candidate_a.duration_ms
                    ),
                }
            } else if candidate_a.lines_changed < candidate_b.lines_changed {
                SpeculativeWinner::Winner {
                    candidate_id: candidate_a.candidate_id.clone(),
                    reason: format!(
                        "More concise diff ({} lines vs {})",
                        candidate_a.lines_changed, candidate_b.lines_changed
                    ),
                }
            } else if candidate_b.lines_changed < candidate_a.lines_changed {
                SpeculativeWinner::Winner {
                    candidate_id: candidate_b.candidate_id.clone(),
                    reason: format!(
                        "More concise diff ({} lines vs {})",
                        candidate_b.lines_changed, candidate_a.lines_changed
                    ),
                }
            } else {
                SpeculativeWinner::Tie
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speculative_gate_winner_selection() {
        let cand_a = CandidateExecution {
            candidate_id: "variant-a".to_string(),
            branch: "quark/a/01".to_string(),
            passed_tests: true,
            duration_ms: 1200,
            lines_changed: 45,
            memory_peak_mb: Some(120),
        };

        let cand_b = CandidateExecution {
            candidate_id: "variant-b".to_string(),
            branch: "quark/b/01".to_string(),
            passed_tests: true,
            duration_ms: 2500,
            lines_changed: 80,
            memory_peak_mb: Some(150),
        };

        let win = select_speculative_winner(&cand_a, &cand_b);
        match win {
            SpeculativeWinner::Winner { candidate_id, .. } => {
                assert_eq!(candidate_id, "variant-a");
            }
            _ => panic!("Expected variant-a winner"),
        }

        let cand_failed = CandidateExecution {
            candidate_id: "variant-c".to_string(),
            branch: "quark/c/01".to_string(),
            passed_tests: false,
            duration_ms: 500,
            lines_changed: 10,
            memory_peak_mb: None,
        };

        let win_pass = select_speculative_winner(&cand_failed, &cand_b);
        match win_pass {
            SpeculativeWinner::Winner { candidate_id, .. } => {
                assert_eq!(candidate_id, "variant-b");
            }
            _ => panic!("Expected variant-b winner"),
        }
    }
}
