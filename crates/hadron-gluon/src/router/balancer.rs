//! Predictive Rate-Limit & Token Load Balancer (Capability #8).
//!
//! Evaluates candidate quarks based on remaining provider quota, in-flight concurrency,
//! historical turn latency, and rate-limit risks to select optimal execution targets.

use hadron_lattice::QuarkId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuarkCandidateMetrics {
    pub quark: QuarkId,
    pub quota_remaining_pct: f32, // 0.0 to 1.0
    pub in_flight_turns: usize,
    pub average_turn_duration_secs: f32,
    pub cost_per_m_tokens_usd: f32,
    pub is_rate_limited: bool,
}

impl QuarkCandidateMetrics {
    /// Computes load balancer dispatch score. Higher is better.
    pub fn score(&self) -> f32 {
        if self.is_rate_limited {
            return -10_000.0;
        }

        let quota_score = (self.quota_remaining_pct * 100.0).clamp(0.0, 100.0) * 0.4;
        let concurrency_penalty = (self.in_flight_turns as f32) * 25.0;
        let latency_penalty = self.average_turn_duration_secs.clamp(0.0, 60.0) * 0.5;
        let cost_penalty = self.cost_per_m_tokens_usd.clamp(0.0, 50.0) * 1.5;

        quota_score - concurrency_penalty - latency_penalty - cost_penalty
    }
}

/// Selects the best candidate quark based on load balancer scores.
pub fn select_best_candidate(candidates: &[QuarkCandidateMetrics]) -> Option<QuarkId> {
    candidates
        .iter()
        .filter(|c| !c.is_rate_limited)
        .max_by(|a, b| a.score().partial_cmp(&b.score()).unwrap_or(std::cmp::Ordering::Equal))
        .map(|c| c.quark.clone())
}

/// Predicts whether a planned token request risks tripping provider rate limits.
pub fn predict_rate_limit_risk(
    estimated_tokens: u64,
    bucket_remaining: u64,
    cooldown_secs: u64,
) -> bool {
    if cooldown_secs > 0 {
        return true;
    }
    if bucket_remaining == 0 {
        return true;
    }
    estimated_tokens > bucket_remaining
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balancer_routing_and_scores() {
        let candidate_a = QuarkCandidateMetrics {
            quark: QuarkId::new("claude"),
            quota_remaining_pct: 0.95,
            in_flight_turns: 0,
            average_turn_duration_secs: 4.5,
            cost_per_m_tokens_usd: 3.0,
            is_rate_limited: false,
        };

        let candidate_b = QuarkCandidateMetrics {
            quark: QuarkId::new("gpt4"),
            quota_remaining_pct: 0.20,
            in_flight_turns: 2,
            average_turn_duration_secs: 15.0,
            cost_per_m_tokens_usd: 10.0,
            is_rate_limited: false,
        };

        let candidate_c = QuarkCandidateMetrics {
            quark: QuarkId::new("gemini"),
            quota_remaining_pct: 0.99,
            in_flight_turns: 0,
            average_turn_duration_secs: 2.0,
            cost_per_m_tokens_usd: 0.5,
            is_rate_limited: false,
        };

        let candidates = vec![candidate_a, candidate_b, candidate_c];
        let best = select_best_candidate(&candidates);
        assert_eq!(best, Some(QuarkId::new("gemini")));
    }

    #[test]
    fn test_predict_rate_limit_risk() {
        assert!(predict_rate_limit_risk(10_000, 5_000, 0));
        assert!(predict_rate_limit_risk(1_000, 50_000, 30));
        assert!(!predict_rate_limit_risk(2_000, 100_000, 0));
    }
}
