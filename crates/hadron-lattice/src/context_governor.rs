//! In-Flight Context Budget Auto-Governor.
//!
//! Compacts verbose tool stdout/stderr (e.g. massive compiler dumps, long test backtraces)
//! into concise, panic/error-preserving summaries before appending to the turn context.

#[derive(Debug, Clone)]
pub struct GovernorConfig {
    pub max_tool_bytes: usize,
    pub preserve_head_lines: usize,
    pub preserve_tail_lines: usize,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            max_tool_bytes: 4096,
            preserve_head_lines: 10,
            preserve_tail_lines: 15,
        }
    }
}

pub struct ContextGovernor;

impl ContextGovernor {
    /// Checks if a line contains critical diagnostic signals that should always be retained.
    pub fn is_critical_line(line: &str) -> bool {
        let l = line.trim();
        l.starts_with("error:")
            || l.starts_with("error[")
            || l.contains("panicked at")
            || l.contains("FAILED")
            || l.contains("test result:")
            || l.starts_with("Caused by:")
            || l.starts_with("Assertion failed")
    }

    /// Compacts raw tool stdout/stderr according to governor budget limits.
    pub fn compact_tool_output(raw: &str, config: &GovernorConfig) -> String {
        if raw.len() <= config.max_tool_bytes {
            return raw.to_string();
        }

        let lines: Vec<&str> = raw.lines().collect();
        let total_lines = lines.len();

        if total_lines <= config.preserve_head_lines + config.preserve_tail_lines {
            return raw.to_string();
        }

        let head = &lines[..config.preserve_head_lines];
        let tail = &lines[total_lines - config.preserve_tail_lines..];
        let middle = &lines[config.preserve_head_lines..total_lines - config.preserve_tail_lines];

        // Harvest any critical error lines from the middle chunk
        let mut critical_middle = Vec::new();
        for line in middle {
            if Self::is_critical_line(line) {
                critical_middle.push(*line);
            }
        }

        let mut output = String::with_capacity(config.max_tool_bytes);
        for line in head {
            output.push_str(line);
            output.push('\n');
        }

        let omitted_count = middle.len().saturating_sub(critical_middle.len());
        output.push_str(&format!(
            "\n[... ContextGovernor: omitted {} verbose output lines ...]\n\n",
            omitted_count
        ));

        for line in &critical_middle {
            output.push_str(line);
            output.push('\n');
        }

        for line in tail {
            output.push_str(line);
            output.push('\n');
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_governor_short_output() {
        let short = "Everything passed cleanly in 0.05s.";
        let config = GovernorConfig::default();
        let result = ContextGovernor::compact_tool_output(short, &config);
        assert_eq!(result, short);
    }

    #[test]
    fn test_context_governor_compacts_and_retains_critical_lines() {
        let mut lines = Vec::new();
        for i in 0..100 {
            if i == 50 {
                lines.push("error[E0308]: mismatched types expected usize, found u32");
            } else if i == 51 {
                lines.push("panicked at src/main.rs:12: assertion failed");
            } else {
                lines.push("Compiling some_dependency v0.1.0 ...");
            }
        }
        let raw = lines.join("\n");
        let config = GovernorConfig {
            max_tool_bytes: 200,
            preserve_head_lines: 5,
            preserve_tail_lines: 5,
        };

        let compacted = ContextGovernor::compact_tool_output(&raw, &config);
        assert!(compacted.len() < raw.len());
        assert!(compacted.contains("ContextGovernor: omitted"));
        assert!(compacted.contains("error[E0308]: mismatched types"));
        assert!(compacted.contains("panicked at src/main.rs:12"));
    }
}
