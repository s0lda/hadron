use serde::{Deserialize, Serialize};

/// AST mutation generator and tester for Adversarial Quarks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstMutationCandidate {
    pub file_path: String,
    pub original_snippet: String,
    pub mutated_snippet: String,
    pub byte_offset: usize,
}

pub struct AstMutator;

impl AstMutator {
    /// Scan source text and generate mutation candidates (operator swaps, boundary swaps, bool flips).
    pub fn generate_mutations(file_path: &str, source_code: &str) -> Vec<AstMutationCandidate> {
        let mut candidates = Vec::new();

        let replacements = [
            (" == ", " != "),
            (" != ", " == "),
            (" > ", " <= "),
            (" < ", " >= "),
            (" >= ", " < "),
            (" <= ", " > "),
            (" + ", " - "),
            (" - ", " + "),
            ("true", "false"),
            ("false", "true"),
            (" && ", " || "),
            (" || ", " && "),
        ];

        for (from, to) in &replacements {
            let mut search_start = 0;
            while let Some(pos) = source_code[search_start..].find(from) {
                let actual_offset = search_start + pos;
                candidates.push(AstMutationCandidate {
                    file_path: file_path.to_string(),
                    original_snippet: from.to_string(),
                    mutated_snippet: to.to_string(),
                    byte_offset: actual_offset,
                });
                search_start = actual_offset + from.len();
            }
        }

        candidates
    }

    /// Apply a single mutation to source code string.
    pub fn apply_mutation(source_code: &str, candidate: &AstMutationCandidate) -> Option<String> {
        if candidate.byte_offset + candidate.original_snippet.len() <= source_code.len() {
            let prefix = &source_code[..candidate.byte_offset];
            let suffix = &source_code[candidate.byte_offset + candidate.original_snippet.len()..];
            Some(format!("{}{}{}", prefix, candidate.mutated_snippet, suffix))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ast_mutator_generation_and_application() {
        let src = "if x > 10 && ready == true { return a + b; }";
        let mutations = AstMutator::generate_mutations("src/calc.rs", src);
        assert!(!mutations.is_empty());

        let plus_mutation = mutations.iter().find(|m| m.original_snippet == " + ").unwrap();
        let mutated = AstMutator::apply_mutation(src, plus_mutation).unwrap();
        assert!(mutated.contains("a - b"));
    }
}
