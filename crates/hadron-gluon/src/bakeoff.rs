//! Speculative Parallel Bake-Offs (`/bakeoff`).
//!
//! Spawns competing worker quarks across isolated branches to solve the same spec,
//! auto-benchmarking code churn, test coverage, and execution metrics to select
//! the winning implementation and generate fast-forward recommendations.

use serde::{Deserialize, Serialize};

/// Evaluation metrics for a single candidate branch in a tournament bake-off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BakeOffCandidateResult {
    pub quark_id: String,
    pub branch_name: String,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub tests_added: usize,
    pub duration_ms: u64,
    pub gate_passed: bool,
    pub score: f64,
}

impl BakeOffCandidateResult {
    /// Total line churn (lines added + lines removed).
    pub fn lines_changed(&self) -> usize {
        self.lines_added + self.lines_removed
    }
}

/// Lifecycle state of a tournament bake-off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TournamentStatus {
    Pending,
    InProgress,
    Completed { winner: BakeOffCandidateResult },
    Failed { reason: String },
}

/// Summary report of tournament outcomes with ranking and fast-forward guidance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BakeOffWinnerReport {
    pub spec_id: String,
    pub task_description: String,
    pub winner: Option<BakeOffCandidateResult>,
    pub ranking: Vec<BakeOffCandidateResult>,
    pub summary_markdown: String,
    pub fast_forward_command: Option<String>,
}

/// Scoring helper for evaluating candidate implementations.
pub struct BakeOffScorer;

impl BakeOffScorer {
    /// Calculate composite candidate score.
    ///
    /// - Gate pass is a strict prerequisite: 0.0 if failed.
    /// - Base pass reward: 1000.0 pts.
    /// - Test coverage reward: +50.0 pts per test added.
    /// - Code simplicity reward: -0.5 pts per line of churn (rewarding conciseness per Rule 10).
    /// - Performance reward: +100.0 pts for fast runs (< 5000ms), +50.0 pts otherwise.
    pub fn calculate(
        lines_added: usize,
        lines_removed: usize,
        tests_added: usize,
        duration_ms: u64,
        gate_passed: bool,
    ) -> f64 {
        if !gate_passed {
            return 0.0;
        }

        let base = 1000.0;
        let test_bonus = (tests_added as f64) * 50.0;
        let churn_penalty = ((lines_added + lines_removed) as f64) * 0.5;
        let perf_bonus = if duration_ms < 5000 { 100.0 } else { 50.0 };

        (base + test_bonus - churn_penalty + perf_bonus).max(1.0)
    }
}

/// Manager orchestrating speculative tournament bake-offs across competing quarks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BakeOffManager {
    pub spec_id: String,
    pub task_description: String,
    candidates: Vec<BakeOffCandidateResult>,
    status: TournamentStatus,
}

impl BakeOffManager {
    pub fn new(spec_id: &str, task_description: &str) -> Self {
        Self {
            spec_id: spec_id.to_string(),
            task_description: task_description.to_string(),
            candidates: Vec::new(),
            status: TournamentStatus::Pending,
        }
    }

    /// Register or record a completed evaluation result for a candidate quark.
    pub fn record_result(
        &mut self,
        quark_id: &str,
        branch_name: &str,
        lines_added: usize,
        lines_removed: usize,
        tests_added: usize,
        duration_ms: u64,
        gate_passed: bool,
    ) -> f64 {
        let score = BakeOffScorer::calculate(lines_added, lines_removed, tests_added, duration_ms, gate_passed);

        let result = BakeOffCandidateResult {
            quark_id: quark_id.to_string(),
            branch_name: branch_name.to_string(),
            lines_added,
            lines_removed,
            tests_added,
            duration_ms,
            gate_passed,
            score,
        };

        // Replace if already present for this quark, otherwise push
        if let Some(pos) = self.candidates.iter().position(|c| c.quark_id == quark_id) {
            self.candidates[pos] = result;
        } else {
            self.candidates.push(result);
        }

        self.status = TournamentStatus::InProgress;
        score
    }

    /// Return all candidate results sorted by score descending.
    pub fn ranked_candidates(&self) -> Vec<BakeOffCandidateResult> {
        let mut ranked = self.candidates.clone();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.lines_changed().cmp(&b.lines_changed()))
        });
        ranked
    }

    /// Select the highest scoring viable winner (must have passed gate).
    pub fn select_winner(&mut self) -> Option<BakeOffCandidateResult> {
        let winner = self
            .candidates
            .iter()
            .filter(|c| c.gate_passed)
            .max_by(|a, b| {
                a.score
                    .partial_cmp(&b.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.lines_changed().cmp(&a.lines_changed()))
            })
            .cloned();

        if let Some(ref w) = winner {
            self.status = TournamentStatus::Completed { winner: w.clone() };
        } else if !self.candidates.is_empty() {
            self.status = TournamentStatus::Failed {
                reason: "No candidate passed the Merge Gate verification".to_string(),
            };
        }

        winner
    }

    /// Generate a structured Markdown report summarizing tournament standings.
    pub fn generate_winner_report(&mut self) -> BakeOffWinnerReport {
        let winner = self.select_winner();
        let ranking = self.ranked_candidates();

        let mut md = String::new();
        md.push_str(&format!("# Speculative Bake-Off Tournament: `{}`\n\n", self.spec_id));
        md.push_str(&format!("**Task**: {}\n\n", self.task_description));

        if let Some(ref w) = winner {
            md.push_str(&format!(
                "🏆 **Winning Candidate**: `@`{} (Branch: `{}` | Score: `{:.1}`)\n\n",
                w.quark_id, w.branch_name, w.score
            ));
        } else {
            md.push_str("⚠️ **No Winning Candidate**: None of the competing branches passed gate verification.\n\n");
        }

        md.push_str("| Rank | Quark | Branch | Lines (+/-) | Tests | Duration | Gate | Score |\n");
        md.push_str("|---|---|---|---|---|---|---|---|\n");

        for (idx, c) in ranking.iter().enumerate() {
            let is_winner = winner.as_ref().map(|w| w.quark_id == c.quark_id).unwrap_or(false);
            let badge = if is_winner { " 👑" } else { "" };
            let gate_str = if c.gate_passed { "✅ PASS" } else { "❌ FAIL" };

            md.push_str(&format!(
                "| {} | `@{}`{} | `{}` | `+{}/-{}` | `{}` | `{}ms` | {} | `{:.1}` |\n",
                idx + 1,
                c.quark_id,
                badge,
                c.branch_name,
                c.lines_added,
                c.lines_removed,
                c.tests_added,
                c.duration_ms,
                gate_str,
                c.score
            ));
        }

        let ff_cmd = winner
            .as_ref()
            .map(|w| format!("git merge --ff-only {}", w.branch_name));

        if let Some(ref cmd) = ff_cmd {
            md.push_str(&format!("\n### Integration Recommendation\n```bash\n{}\n```\n", cmd));
        }

        BakeOffWinnerReport {
            spec_id: self.spec_id.clone(),
            task_description: self.task_description.clone(),
            winner,
            ranking,
            summary_markdown: md,
            fast_forward_command: ff_cmd,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bakeoff_candidate_ranking_and_winner_selection() {
        let mut manager = BakeOffManager::new("spec-vulkan-pipeline", "Implement Vulkan buffer barrier");

        // Candidate 1: Passing, concise, 2 tests
        manager.record_result("quark-alpha", "quark/alpha/01M1", 45, 10, 2, 1200, true);

        // Candidate 2: Passing, bloated churn, 1 test
        manager.record_result("quark-beta", "quark/beta/01M2", 300, 80, 1, 4500, true);

        // Candidate 3: Failed gate
        manager.record_result("quark-gamma", "quark/gamma/01M3", 20, 5, 4, 800, false);

        let ranking = manager.ranked_candidates();
        assert_eq!(ranking.len(), 3);
        assert_eq!(ranking[0].quark_id, "quark-alpha");
        assert!(ranking[0].score > ranking[1].score);
        assert_eq!(ranking[2].quark_id, "quark-gamma");
        assert_eq!(ranking[2].score, 0.0);

        let winner = manager.select_winner().expect("Winner should be chosen");
        assert_eq!(winner.quark_id, "quark-alpha");
        assert_eq!(winner.branch_name, "quark/alpha/01M1");
    }

    #[test]
    fn test_bakeoff_winner_report_generation() {
        let mut manager = BakeOffManager::new("spec-ast-slicer", "Refactor syn AST pattern match");
        manager.record_result("quark-agy", "quark/agy/b1", 60, 15, 3, 2000, true);
        manager.record_result("quark-codex", "quark/codex/b2", 120, 30, 2, 3500, true);

        let report = manager.generate_winner_report();
        assert!(report.winner.is_some());
        assert_eq!(report.winner.unwrap().quark_id, "quark-agy");
        assert!(report.summary_markdown.contains("Speculative Bake-Off Tournament"));
        assert!(report.summary_markdown.contains("quark/agy/b1"));
        assert_eq!(
            report.fast_forward_command.as_deref(),
            Some("git merge --ff-only quark/agy/b1")
        );
    }

    #[test]
    fn test_bakeoff_all_failed_handling() {
        let mut manager = BakeOffManager::new("spec-fail", "Hard task that all fail");
        manager.record_result("quark-1", "quark/1/b", 10, 5, 0, 1000, false);
        manager.record_result("quark-2", "quark/2/b", 20, 8, 1, 1500, false);

        let winner = manager.select_winner();
        assert!(winner.is_none());
        assert!(matches!(manager.status, TournamentStatus::Failed { .. }));
    }
}
