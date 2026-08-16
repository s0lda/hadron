use serde::{Deserialize, Serialize};

/// Benchmark measurement metric for a designated performance hotpath.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkMetric {
    pub name: String,
    pub mean_time_nanos: f64,
    pub std_dev_nanos: f64,
    pub allocations_bytes: Option<u64>,
    pub throughput_mb_s: Option<f64>,
}

/// Baseline performance snapshot captured on main branch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BenchmarkBaseline {
    pub commit_sha: String,
    pub metrics: Vec<BenchmarkMetric>,
}

/// Regression delta between candidate run and baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkDelta {
    pub metric_name: String,
    pub baseline_time_nanos: f64,
    pub candidate_time_nanos: f64,
    pub delta_pct: f64,
    pub is_regression: bool,
}

/// Performance guard verdict for gatekeeper merge policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkVerdict {
    pub passed: bool,
    pub tolerance_pct: f64,
    pub deltas: Vec<BenchmarkDelta>,
    pub regressions_count: usize,
}

impl BenchmarkVerdict {
    /// Evaluate candidate metrics against baseline with maximum allowable degradation percentage.
    pub fn evaluate(
        baseline: &BenchmarkBaseline,
        candidate_metrics: &[BenchmarkMetric],
        max_regression_tolerance_pct: f64,
    ) -> Self {
        let mut deltas = Vec::new();
        let mut regressions_count = 0;

        for cand in candidate_metrics {
            if let Some(base) = baseline.metrics.iter().find(|m| m.name == cand.name) {
                let delta_pct = if base.mean_time_nanos > 0.0 {
                    ((cand.mean_time_nanos - base.mean_time_nanos) / base.mean_time_nanos) * 100.0
                } else {
                    0.0
                };

                let is_regression = delta_pct > max_regression_tolerance_pct;
                if is_regression {
                    regressions_count += 1;
                }

                deltas.push(BenchmarkDelta {
                    metric_name: cand.name.clone(),
                    baseline_time_nanos: base.mean_time_nanos,
                    candidate_time_nanos: cand.mean_time_nanos,
                    delta_pct,
                    is_regression,
                });
            }
        }

        Self {
            passed: regressions_count == 0,
            tolerance_pct: max_regression_tolerance_pct,
            deltas,
            regressions_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_guard_evaluation() {
        let baseline = BenchmarkBaseline {
            commit_sha: "base123".into(),
            metrics: vec![
                BenchmarkMetric {
                    name: "bench_parser".into(),
                    mean_time_nanos: 1000.0,
                    std_dev_nanos: 10.0,
                    allocations_bytes: None,
                    throughput_mb_s: None,
                },
                BenchmarkMetric {
                    name: "bench_raster".into(),
                    mean_time_nanos: 2000.0,
                    std_dev_nanos: 20.0,
                    allocations_bytes: None,
                    throughput_mb_s: None,
                },
            ],
        };

        // Candidate within tolerance (5% vs 10% allowed)
        let candidate_ok = vec![
            BenchmarkMetric {
                name: "bench_parser".into(),
                mean_time_nanos: 1030.0, // +3%
                std_dev_nanos: 10.0,
                allocations_bytes: None,
                throughput_mb_s: None,
            },
            BenchmarkMetric {
                name: "bench_raster".into(),
                mean_time_nanos: 1950.0, // -2.5% (faster)
                std_dev_nanos: 20.0,
                allocations_bytes: None,
                throughput_mb_s: None,
            },
        ];

        let verdict_ok = BenchmarkVerdict::evaluate(&baseline, &candidate_ok, 5.0);
        assert!(verdict_ok.passed);
        assert_eq!(verdict_ok.regressions_count, 0);

        // Candidate regressing (+20%)
        let candidate_regressed = vec![
            BenchmarkMetric {
                name: "bench_parser".into(),
                mean_time_nanos: 1200.0, // +20%
                std_dev_nanos: 10.0,
                allocations_bytes: None,
                throughput_mb_s: None,
            },
        ];

        let verdict_fail = BenchmarkVerdict::evaluate(&baseline, &candidate_regressed, 5.0);
        assert!(!verdict_fail.passed);
        assert_eq!(verdict_fail.regressions_count, 1);
        assert_eq!(verdict_fail.deltas[0].delta_pct, 20.0);
    }
}
