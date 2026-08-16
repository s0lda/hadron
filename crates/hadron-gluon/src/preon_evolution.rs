use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A recurring failure pattern extracted from post-mortems and test failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailurePattern {
    pub pattern_id: String,
    pub description: String,
    pub recurrence_count: usize,
    pub related_files: Vec<String>,
    pub suggested_invariant: String,
}

/// Synthesized preon document structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesizedPreon {
    pub name: String,
    pub description: String,
    pub invariants: Vec<String>,
    pub mandatory_rules: Vec<String>,
    pub markdown_body: String,
}

/// Evolution engine that clusters failure notes into actionable preons.
pub struct PreonEvolutionEngine;

impl PreonEvolutionEngine {
    /// Analyze note contents from `.hadron/nucleus/notes/` and extract failure patterns.
    pub fn extract_patterns(notes: &[(String, String)]) -> Vec<FailurePattern> {
        let mut keyword_clusters: HashMap<String, Vec<(String, String)>> = HashMap::new();

        for (slug, content) in notes {
            for line in content.lines() {
                let lower = line.to_lowercase();
                if lower.contains("must never") || lower.contains("invariant") || lower.contains("cost") || lower.contains("failed") {
                    let key = if lower.contains("vulkan") || lower.contains("lavapipe") || lower.contains("gpu") {
                        "rendering_gpu"
                    } else if lower.contains("acp") || lower.contains("wire") || lower.contains("ipc") {
                        "ipc_protocol"
                    } else if lower.contains("target") || lower.contains("cargo") || lower.contains("build") {
                        "build_target"
                    } else if lower.contains("gate") || lower.contains("merge") || lower.contains("rebase") {
                        "merge_gate"
                    } else {
                        "general_invariant"
                    };

                    keyword_clusters
                        .entry(key.to_string())
                        .or_default()
                        .push((slug.clone(), line.trim().to_string()));
                }
            }
        }

        let mut patterns = Vec::new();
        for (category, occurrences) in keyword_clusters {
            let count = occurrences.len();
            let related_files: Vec<String> = occurrences.iter().map(|(s, _)| s.clone()).collect();
            let summary = format!("Recurrent operational boundary in category '{}'", category);
            let suggested_invariant = occurrences.first().map(|(_, l)| l.clone()).unwrap_or_default();

            patterns.push(FailurePattern {
                pattern_id: format!("pattern-{}", category),
                description: summary,
                recurrence_count: count,
                related_files,
                suggested_invariant,
            });
        }

        patterns.sort_by(|a, b| b.recurrence_count.cmp(&a.recurrence_count));
        patterns
    }

    /// Synthesize a structured preon markdown document from clustered failure patterns.
    pub fn synthesize_preon(
        topic_name: &str,
        patterns: &[FailurePattern],
    ) -> SynthesizedPreon {
        let mut invariants = Vec::new();
        let mut rules = Vec::new();

        for p in patterns {
            if !p.suggested_invariant.is_empty() {
                invariants.push(p.suggested_invariant.clone());
                rules.push(format!("Enforce pattern constraint: {}", p.description));
            }
        }

        let mut body = String::new();
        body.push_str(&format!("---\nname: {}\ndescription: Auto-synthesized operational preon for {}\n---\n\n", topic_name, topic_name));
        body.push_str(&format!("# Preon: {}\n\n", topic_name));
        body.push_str("## Core Invariants\n");
        for inv in &invariants {
            body.push_str(&format!("- **{}**\n", inv));
        }
        body.push_str("\n## Mandatory Operational Rules\n");
        for r in &rules {
            body.push_str(&format!("- {}\n", r));
        }

        SynthesizedPreon {
            name: topic_name.to_string(),
            description: format!("Auto-synthesized preon for {}", topic_name),
            invariants,
            mandatory_rules: rules,
            markdown_body: body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preon_evolution_extraction_and_synthesis() {
        let notes = vec![
            (
                "gpu-lavapipe".into(),
                "LAVAPIPE is the only Vulkan ICD on target WSL machines; GPUI rasterizes in CPU software. Must never treat frame lag as a code regression.".into(),
            ),
            (
                "shared-target-dir".into(),
                "Shared target directory can serve a foreign rlib; cargo build must touch crate lib.rs when verifying under concurrent swarm turns.".into(),
            ),
        ];

        let patterns = PreonEvolutionEngine::extract_patterns(&notes);
        assert!(!patterns.is_empty());

        let preon = PreonEvolutionEngine::synthesize_preon("gpu-and-build", &patterns);
        assert_eq!(preon.name, "gpu-and-build");
        assert!(preon.markdown_body.contains("# Preon: gpu-and-build"));
        assert!(preon.markdown_body.contains("Core Invariants"));
    }
}
