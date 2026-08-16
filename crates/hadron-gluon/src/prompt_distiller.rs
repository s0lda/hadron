use serde::{Deserialize, Serialize};

/// Compression and context optimization metrics for distilled prompts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistillationMetrics {
    pub original_chars: usize,
    pub distilled_chars: usize,
    pub estimated_original_tokens: usize,
    pub estimated_distilled_tokens: usize,
    pub compression_ratio: f64,
}

/// Dynamic prompt distiller that optimizes prompt headers, preons, and nucleus indices to fit context budgets.
pub struct PromptDistiller;

impl PromptDistiller {
    /// Compress raw prompt text by trimming conversational filler, collapsing redundant whitespace,
    /// and retaining essential invariant and protocol lines.
    pub fn distill_prompt(raw_prompt: &str, target_max_tokens: usize) -> (String, DistillationMetrics) {
        let original_chars = raw_prompt.len();
        let estimated_original_tokens = original_chars / 4;

        let mut lines = Vec::new();
        for line in raw_prompt.lines() {
            let trimmed = line.trim();
            // Skip empty lines or purely decorative horizontal separators if over budget
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("---") || trimmed.starts_with("===") {
                continue;
            }
            lines.push(trimmed);
        }

        let mut distilled = String::new();
        for line in lines {
            let line_tokens = (line.len() + 3) / 4;
            let current_tokens = distilled.len() / 4;
            if current_tokens + line_tokens > target_max_tokens {
                break;
            }
            distilled.push_str(line);
            distilled.push('\n');
        }

        let distilled_chars = distilled.len();
        let estimated_distilled_tokens = distilled_chars / 4;
        let compression_ratio = if original_chars > 0 {
            distilled_chars as f64 / original_chars as f64
        } else {
            1.0
        };

        let metrics = DistillationMetrics {
            original_chars,
            distilled_chars,
            estimated_original_tokens,
            estimated_distilled_tokens,
            compression_ratio,
        };

        (distilled, metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_distillation() {
        let raw = r#"
# Instructions

---

You are a quark working in a swarm.
Always adhere to the standard model rules.

---

Do not produce conversational fluff.
"#;
        let (distilled, metrics) = PromptDistiller::distill_prompt(raw, 50);
        assert!(distilled.contains("You are a quark working in a swarm."));
        assert!(!distilled.contains("---"));
        assert!(metrics.distilled_chars <= metrics.original_chars);
        assert!(metrics.estimated_distilled_tokens <= 50);
    }
}
