//! Semantic Test Failure Minimizer.
//!
//! Parses noisy test runner output, strips passing test lines, isolates exact assertion diffs
//! and panic messages, and clusters failures by root-cause source locations.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusteredFailure {
    pub test_name: String,
    pub panic_location: String,
    pub assertion_diff: String,
}

pub struct TestFailureMinimizer;

impl TestFailureMinimizer {
    /// Minimizes raw cargo test output into clustered failures and a concise markdown summary.
    pub fn minimize(raw: &str) -> (Vec<ClusteredFailure>, String) {
        let mut failures = Vec::new();
        let lines: Vec<&str> = raw.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim();
            // Start of a test failure output block: "---- test_name stdout ----"
            if line.starts_with("---- ") && line.ends_with(" stdout ----") {
                let test_name = line
                    .strip_prefix("---- ")
                    .and_then(|s| s.strip_suffix(" stdout ----"))
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();

                let mut panic_loc = "unknown".to_string();
                let mut assertion_diff = String::new();

                i += 1;
                while i < lines.len() && !lines[i].starts_with("---- ") && !lines[i].starts_with("failures:") {
                    let sub_line = lines[i].trim();
                    if sub_line.contains("panicked at") {
                        if let Some(pos) = sub_line.find("panicked at ") {
                            panic_loc = sub_line[pos + 12..].to_string();
                        }
                    } else if sub_line.starts_with("assertion `left == right` failed")
                        || sub_line.starts_with("left:")
                        || sub_line.starts_with("right:")
                        || sub_line.starts_with("Diff < left / right > :")
                    {
                        assertion_diff.push_str(sub_line);
                        assertion_diff.push('\n');
                    }
                    i += 1;
                }

                failures.push(ClusteredFailure {
                    test_name,
                    panic_location: panic_loc,
                    assertion_diff: assertion_diff.trim().to_string(),
                });
                continue;
            }
            i += 1;
        }

        // Format concise summary
        let mut summary = String::new();
        if failures.is_empty() {
            summary.push_str("All tests passed cleanly. Zero failures.");
        } else {
            summary.push_str(&format!("### Test Failure Minimizer ({} failed):\n\n", failures.len()));

            // Cluster by panic location
            let mut clusters: HashMap<String, Vec<&ClusteredFailure>> = HashMap::new();
            for f in &failures {
                clusters.entry(f.panic_location.clone()).or_default().push(f);
            }

            for (loc, cluster) in clusters {
                summary.push_str(&format!("#### Root Cause: `{}`\n", loc));
                for f in cluster {
                    summary.push_str(&format!("- **{}**", f.test_name));
                    if !f.assertion_diff.is_empty() {
                        summary.push_str(&format!(": `{}`", f.assertion_diff.replace('\n', " ")));
                    }
                    summary.push('\n');
                }
                summary.push('\n');
            }
        }

        (failures, summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimize_failures() {
        let sample = "\
running 10 tests\n\
test a ... ok\n\
test b ... ok\n\
---- tests::test_parse stdout ----\n\
thread 'tests::test_parse' panicked at src/parser.rs:42:10:\n\
assertion `left == right` failed\n\
left: 10\n\
right: 20\n\
---- tests::test_lex stdout ----\n\
thread 'tests::test_lex' panicked at src/parser.rs:42:10:\n\
assertion `left == right` failed\n\
failures:\n\
    tests::test_parse\n\
    tests::test_lex\n";

        let (failures, summary) = TestFailureMinimizer::minimize(sample);
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].test_name, "tests::test_parse");
        assert!(failures[0].panic_location.contains("src/parser.rs:42"));
        assert!(summary.contains("Root Cause: `src/parser.rs:42:10:`"));
        assert!(summary.contains("tests::test_parse"));
        assert!(summary.contains("tests::test_lex"));
    }
}
