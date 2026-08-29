use regex::Regex;

#[derive(Debug, Clone)]
pub struct InvariantViolation {
    pub rule_name: String,
    pub file_path: String,
    pub line_number: usize,
    pub snippet: String,
}

pub struct InvariantLinter {
    rules: Vec<(String, Regex)>,
}

impl InvariantLinter {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_forbidden_pattern(&mut self, rule_name: &str, pattern: &str) {
        if let Ok(re) = Regex::new(pattern) {
            self.rules.push((rule_name.to_string(), re));
        }
    }

    pub fn lint_file(&self, path: &str, content: &str) -> Vec<InvariantViolation> {
        let mut violations = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            for (name, re) in &self.rules {
                if re.is_match(line) {
                    violations.push(InvariantViolation {
                        rule_name: name.clone(),
                        file_path: path.to_string(),
                        line_number: idx + 1,
                        snippet: line.trim().to_string(),
                    });
                }
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invariant_linter_bans_unqualified_colors() {
        let mut linter = InvariantLinter::new();
        linter.add_forbidden_pattern("One Font Family", r"font_family\s*=\s*.*,");

        let code_bad = "theme.font_family = \"JetBrains Mono, Menlo, monospace\";";
        let violations = linter.lint_file("crates/hadron-chamber/src/app/theme.rs", code_bad);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_name, "One Font Family");
    }
}
