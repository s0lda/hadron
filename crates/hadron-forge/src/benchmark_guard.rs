use serde::{Deserialize, Serialize};

/// Benchmark data collector and regression evaluator in hadron-forge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkRunSummary {
    pub total_benchmarks: usize,
    pub passed_threshold: bool,
    pub regression_names: Vec<String>,
}

pub struct BenchmarkForge;

impl BenchmarkForge {
    /// Parse standard criterion/cargo bench output text into benchmark timing maps.
    pub fn parse_bench_output(output: &str) -> Vec<(String, f64)> {
        let mut results = Vec::new();
        for line in output.lines() {
            let trimmed = line.trim();
            // Look for patterns like "test bench_foo ... bench: 1,234 ns/iter (+/- 50)"
            if trimmed.contains("bench:") && trimmed.contains("ns/iter") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 5 && parts[0] == "test" {
                    let name = parts[1].to_string();
                    let ns_str = parts[4].replace(',', "");
                    if let Ok(ns) = ns_str.parse::<f64>() {
                        results.push((name, ns));
                    }
                }
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bench_output() {
        let output = r#"
test bench_render_frame ... bench:       1,500 ns/iter (+/- 100)
test bench_json_parse   ... bench:         250 ns/iter (+/- 10)
"#;
        let parsed = BenchmarkForge::parse_bench_output(output);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "bench_render_frame");
        assert_eq!(parsed[0].1, 1500.0);
        assert_eq!(parsed[1].0, "bench_json_parse");
        assert_eq!(parsed[1].1, 250.0);
    }
}
