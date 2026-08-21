use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentSpec {
    pub task: String,
    pub candidate_count: usize,
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateResult {
    pub branch: String,
    pub gate_passed: bool,
    pub test_duration_ms: u64,
    pub diff_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WinnerReport {
    pub winner_branch: Option<String>,
    pub reason: String,
    pub candidates: Vec<CandidateResult>,
}

impl TournamentSpec {
    pub fn new(task: impl Into<String>, candidate_count: usize) -> Self {
        Self {
            task: task.into(),
            candidate_count,
            branches: Vec::new(),
        }
    }

    pub fn add_candidate_branch(&mut self, branch: impl Into<String>) {
        self.branches.push(branch.into());
    }

    pub fn evaluate_winner(candidates: &[CandidateResult]) -> WinnerReport {
        let passed: Vec<&CandidateResult> = candidates.iter().filter(|c| c.gate_passed).collect();
        if passed.is_empty() {
            return WinnerReport {
                winner_branch: None,
                reason: "No candidate branches passed the merge gate".to_string(),
                candidates: candidates.to_vec(),
            };
        }

        // Rank by least diff lines, then by lowest test duration
        let winner = passed
            .into_iter()
            .min_by_key(|c| (c.diff_lines, c.test_duration_ms))
            .expect("passed is non-empty");

        WinnerReport {
            winner_branch: Some(winner.branch.clone()),
            reason: format!(
                "Candidate branch {} won tournament: passed gate with smallest diff ({} lines) in {}ms",
                winner.branch, winner.diff_lines, winner.test_duration_ms
            ),
            candidates: candidates.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tournament_winner_evaluation() {
        let candidates = vec![
            CandidateResult {
                branch: "quark/worker-1/fix-a".into(),
                gate_passed: true,
                test_duration_ms: 1200,
                diff_lines: 45,
            },
            CandidateResult {
                branch: "quark/worker-2/fix-b".into(),
                gate_passed: true,
                test_duration_ms: 800,
                diff_lines: 18,
            },
            CandidateResult {
                branch: "quark/worker-3/fix-c".into(),
                gate_passed: false,
                test_duration_ms: 500,
                diff_lines: 10,
            },
        ];

        let report = TournamentSpec::evaluate_winner(&candidates);
        assert_eq!(report.winner_branch.as_deref(), Some("quark/worker-2/fix-b"));
        assert!(report.reason.contains("18 lines"));
    }

    #[test]
    fn test_tournament_all_failed() {
        let candidates = vec![CandidateResult {
            branch: "quark/worker-1/fail".into(),
            gate_passed: false,
            test_duration_ms: 300,
            diff_lines: 12,
        }];

        let report = TournamentSpec::evaluate_winner(&candidates);
        assert!(report.winner_branch.is_none());
    }
}
