//! Pre-Turn Baseline Health Snapshotter (Standard Model Rule 5).
//!
//! Captures pre-existing test failures and lints at worktree baseline, whitelisting
//! pre-existing failures from gatekeeper rejection so quarks own only the delta.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineReport {
    pub known_failing_tests: HashSet<String>,
    pub known_warnings: Vec<String>,
    pub timestamp: u64,
}

#[derive(Debug, Default, Clone)]
pub struct BaselineHealthSnapshotter;

impl BaselineHealthSnapshotter {
    pub fn new() -> Self {
        Self
    }

    /// Parses failing test names from raw `cargo test` stdout/stderr.
    pub fn parse_test_failures(output: &str) -> HashSet<String> {
        let mut failures = HashSet::new();

        for line in output.lines() {
            let trimmed = line.trim();
            // Match: "test foo::bar::test_baz ... FAILED"
            if trimmed.starts_with("test ") && trimmed.ends_with("... FAILED") {
                if let Some(rest) = trimmed.strip_prefix("test ") {
                    if let Some(test_name) = rest.strip_suffix("... FAILED") {
                        failures.insert(test_name.trim().to_string());
                    }
                }
            } else if trimmed.starts_with("failures:") {
                // Following lines might be failure list, handled per line
            }
        }

        failures
    }

    /// Creates a baseline snapshot from raw baseline run output.
    pub fn create_snapshot(output: &str, warnings: Vec<String>) -> BaselineReport {
        let known_failing_tests = Self::parse_test_failures(output);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        BaselineReport {
            known_failing_tests,
            known_warnings: warnings,
            timestamp,
        }
    }

    /// Filters current failures against baseline, isolating only true regressions.
    pub fn filter_regressions(
        baseline: &BaselineReport,
        current_failures: &HashSet<String>,
    ) -> HashSet<String> {
        current_failures
            .iter()
            .filter(|f| !baseline.known_failing_tests.contains(*f))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baseline_parse_and_whitelist() {
        let baseline_output = "\
running 3 tests\n\
test auth::tests::test_valid_login ... ok\n\
test legacy::tests::test_old_endpoint ... FAILED\n\
test cache::tests::test_hit ... ok\n";

        let snapshot = BaselineHealthSnapshotter::create_snapshot(baseline_output, vec![]);
        assert_eq!(snapshot.known_failing_tests.len(), 1);
        assert!(snapshot.known_failing_tests.contains("legacy::tests::test_old_endpoint"));

        // Current run has same legacy failure plus a new regression
        let current_output = "\
test legacy::tests::test_old_endpoint ... FAILED\n\
test auth::tests::test_token_expiry ... FAILED\n";

        let current_failures = BaselineHealthSnapshotter::parse_test_failures(current_output);
        assert_eq!(current_failures.len(), 2);

        // Filter regressions: only token_expiry should be flagged
        let regressions = BaselineHealthSnapshotter::filter_regressions(&snapshot, &current_failures);
        assert_eq!(regressions.len(), 1);
        assert!(regressions.contains("auth::tests::test_token_expiry"));
        assert!(!regressions.contains("legacy::tests::test_old_endpoint"));
    }
}
