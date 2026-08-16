use serde::{Deserialize, Serialize};

/// Prompt optimization metrics in hadron-forge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptOptimizationReport {
    pub original_byte_len: usize,
    pub optimized_byte_len: usize,
    pub reduction_pct: f64,
}

pub struct PromptDistillerForge;

impl PromptDistillerForge {
    pub fn optimize(prompt_text: &str) -> (String, PromptOptimizationReport) {
        let original_byte_len = prompt_text.len();
        let mut lines = Vec::new();

        for line in prompt_text.lines() {
            let t = line.trim();
            if !t.is_empty() && !t.starts_with("---") {
                lines.push(t);
            }
        }

        let optimized = lines.join("\n");
        let optimized_byte_len = optimized.len();
        let reduction_pct = if original_byte_len > 0 {
            ((original_byte_len - optimized_byte_len) as f64 / original_byte_len as f64) * 100.0
        } else {
            0.0
        };

        let report = PromptOptimizationReport {
            original_byte_len,
            optimized_byte_len,
            reduction_pct,
        };

        (optimized, report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_distiller_forge() {
        let text = "Line 1\n\n---\nLine 2\n";
        let (opt, report) = PromptDistillerForge::optimize(text);
        assert_eq!(opt, "Line 1\nLine 2");
        assert!(report.optimized_byte_len <= report.original_byte_len);
    }
}
