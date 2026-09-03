//! Human Intent Drift Radar.
//!
//! Analyzes in-flight git diff changes against the human prompt's semantic keywords,
//! detecting goal divergence and flagging scope creep before engineering cycles are wasted.

use std::collections::HashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentDriftReport {
    pub score: f32, // 1.0 = fully aligned, 0.0 = completely drifted
    pub matched_keywords: Vec<String>,
    pub prompt_keywords: Vec<String>,
    pub drift_detected: bool,
}

pub struct IntentDriftRadar;

impl IntentDriftRadar {
    /// Extracts meaningful lowercase alphanumeric tokens (len >= 3, excluding stopwords).
    pub fn extract_keywords(text: &str) -> HashSet<String> {
        let stopwords: HashSet<&str> = [
            "the", "and", "for", "with", "this", "that", "from", "into", "some", "none",
            "have", "been", "will", "would", "could", "should", "what", "which", "when",
            "please", "also", "just", "like", "done", "need", "make", "take", "pub", "struct", "fn",
        ]
        .into_iter()
        .collect();

        let mut words = HashSet::new();
        for raw_token in text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
            // Split by underscores and hyphens
            for part in raw_token.split(|c: char| c == '_' || c == '-') {
                // Split CamelCase into words
                let mut current = String::new();
                for ch in part.chars() {
                    if ch.is_uppercase() && !current.is_empty() {
                        let w = current.to_lowercase();
                        if w.len() >= 3 && !stopwords.contains(w.as_str()) {
                            words.insert(w);
                        }
                        current.clear();
                    }
                    current.push(ch);
                }
                if !current.is_empty() {
                    let w = current.to_lowercase();
                    if w.len() >= 3 && !stopwords.contains(w.as_str()) {
                        words.insert(w);
                    }
                }
            }
        }
        words
    }

    /// Evaluates alignment between human prompt and in-flight git diff.
    pub fn evaluate(prompt: &str, git_diff: &str) -> IntentDriftReport {
        let prompt_tokens = Self::extract_keywords(prompt);
        let diff_tokens = Self::extract_keywords(git_diff);

        if prompt_tokens.is_empty() {
            return IntentDriftReport {
                score: 1.0,
                matched_keywords: Vec::new(),
                prompt_keywords: Vec::new(),
                drift_detected: false,
            };
        }

        let mut matched = Vec::new();
        for token in &prompt_tokens {
            if diff_tokens.contains(token) {
                matched.push(token.clone());
            }
        }
        matched.sort();

        let score = (matched.len() as f32) / (prompt_tokens.len() as f32);
        let drift_detected = score < 0.25;

        let mut prompt_keywords: Vec<String> = prompt_tokens.into_iter().collect();
        prompt_keywords.sort();

        IntentDriftReport {
            score,
            matched_keywords: matched,
            prompt_keywords,
            drift_detected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_drift_aligned() {
        let prompt = "Implement build cache mesh and isolation for cargo target";
        let diff = "+pub struct BuildCacheMesh {\n+   target_dir: PathBuf\n+}";

        let report = IntentDriftRadar::evaluate(prompt, diff);
        assert!(report.score > 0.3);
        assert!(!report.drift_detected);
        assert!(report.matched_keywords.contains(&"cache".to_string()) || report.matched_keywords.contains(&"target".to_string()));
    }

    #[test]
    fn test_intent_drift_divergent() {
        let prompt = "Fix sound cues and volume level for notifications in audio module";
        let diff = "+use postgresql::connection_pool;\n+fn execute_db_query() {}";

        let report = IntentDriftRadar::evaluate(prompt, diff);
        assert!(report.score < 0.25);
        assert!(report.drift_detected);
        assert!(report.matched_keywords.is_empty());
    }
}
