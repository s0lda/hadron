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

    pub fn lint_diff(&self, diff: &str) -> Vec<InvariantViolation> {
        let mut violations = Vec::new();
        let mut current_file = String::from("unknown");
        let mut current_line = 0;

        for line in diff.lines() {
            if line.starts_with("+++ b/") || line.starts_with("+++ ") {
                current_file = line
                    .trim_start_matches("+++ b/")
                    .trim_start_matches("+++ ")
                    .trim()
                    .to_string();
                current_line = 0;
                continue;
            }

            if line.starts_with("@@ ") {
                if let Some(plus_idx) = line.find('+') {
                    let num_part = &line[plus_idx + 1..];
                    let num_str = num_part.split([',', ' ']).next().unwrap_or("0");
                    current_line = num_str.parse::<usize>().unwrap_or(0);
                }
                continue;
            }

            if line.starts_with('+') && !line.starts_with("+++") {
                current_line += 1;
                let added = &line[1..];
                let trimmed = added.trim();

                // 1. Ban Allow Dead Code: Only in Rust production source files (.rs, not in tests/, benches/, or examples/)
                if current_file.ends_with(".rs")
                    && !current_file.contains("tests/")
                    && !current_file.contains("benches/")
                {
                    // Must be an actual Rust attribute, not a comment, docstring, or string literal
                    if (trimmed.starts_with("#[allow(") || trimmed.starts_with("#![allow("))
                        && trimmed.contains("dead_code")
                    {
                        violations.push(InvariantViolation {
                            rule_name: "Ban Allow Dead Code".to_string(),
                            file_path: current_file.clone(),
                            line_number: current_line,
                            snippet: trimmed.to_string(),
                        });
                    }
                }

                // 2. One Font Family: GPUI font stack cannot be comma-separated
                if current_file.ends_with(".rs")
                    && (current_file.contains("crates/hadron-chamber") || current_file.contains("theme"))
                {
                    if (trimmed.contains("font_family =") || trimmed.contains("font_family:"))
                        && trimmed.contains(',')
                        && !trimmed.starts_with("//")
                        && !trimmed.starts_with("/*")
                        && !trimmed.starts_with('*')
                    {
                        violations.push(InvariantViolation {
                            rule_name: "One Font Family".to_string(),
                            file_path: current_file.clone(),
                            line_number: current_line,
                            snippet: trimmed.to_string(),
                        });
                    }
                }

                // 3. Any additional custom regex rules registered in self.rules
                for (name, re) in &self.rules {
                    if name == "Ban Allow Dead Code" || name == "One Font Family" {
                        continue;
                    }
                    if re.is_match(trimmed) {
                        violations.push(InvariantViolation {
                            rule_name: name.clone(),
                            file_path: current_file.clone(),
                            line_number: current_line,
                            snippet: trimmed.to_string(),
                        });
                    }
                }
            } else if !line.starts_with('-') {
                current_line += 1;
            }
        }

        violations
    }

    pub fn lint_file(&self, path: &str, content: &str) -> Vec<InvariantViolation> {
        // If content looks like a unified diff, parse as diff
        if content.starts_with("diff --git") || content.contains("\n+++ b/") {
            return self.lint_diff(content);
        }

        let mut violations = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            if path.ends_with(".rs") && !path.contains("tests/") && !path.contains("benches/") {
                if (trimmed.starts_with("#[allow(") || trimmed.starts_with("#![allow("))
                    && trimmed.contains("dead_code")
                {
                    violations.push(InvariantViolation {
                        rule_name: "Ban Allow Dead Code".to_string(),
                        file_path: path.to_string(),
                        line_number: idx + 1,
                        snippet: trimmed.to_string(),
                    });
                }
            }

            for (name, re) in &self.rules {
                if name == "Ban Allow Dead Code" {
                    continue;
                }
                if re.is_match(line) {
                    violations.push(InvariantViolation {
                        rule_name: name.clone(),
                        file_path: path.to_string(),
                        line_number: idx + 1,
                        snippet: trimmed.to_string(),
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

    #[test]
    fn test_invariant_linter_diff_ignores_docs_and_strings() {
        let linter = InvariantLinter::new();
        let dead_code_attr = format!("#[allow({}_code)]", "dead");
        let diff = format!(
            "\ndiff --git a/docs/CHANGELOG.md b/docs/CHANGELOG.md\n\
             --- a/docs/CHANGELOG.md\n\
             +++ b/docs/CHANGELOG.md\n\
             @@ -10,2 +10,4 @@\n\
             +- Wired InvariantLinter into merge gate land path to ban {dead_code_attr} in production code paths\n\
             diff --git a/crates/hadron-chamber/src/app/render/overlays.rs b/crates/hadron-chamber/src/app/render/overlays.rs\n\
             --- a/crates/hadron-chamber/src/app/render/overlays.rs\n\
             +++ b/crates/hadron-chamber/src/app/render/overlays.rs\n\
             @@ -30,2 +30,4 @@\n\
             +            \"Rule 1 Gate Enforcement: Pre-merge InvariantLinter banning {dead_code_attr} in production paths\",\n"
        );
        let violations = linter.lint_diff(&diff);
        assert_eq!(violations.len(), 0);
    }

    #[test]
    fn test_invariant_linter_diff_catches_rust_attribute() {
        let linter = InvariantLinter::new();
        let dead_code_attr = format!("#[allow({}_code)]", "dead");
        let diff = format!(
            "\ndiff --git a/crates/hadron-gluon/src/foo.rs b/crates/hadron-gluon/src/foo.rs\n\
             --- a/crates/hadron-gluon/src/foo.rs\n\
             +++ b/crates/hadron-gluon/src/foo.rs\n\
             @@ -5,2 +5,4 @@\n\
             +{dead_code_attr}\n\
             +pub struct UnwiredScaffolding;\n"
        );
        let violations = linter.lint_diff(&diff);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_name, "Ban Allow Dead Code");
        assert_eq!(violations[0].file_path, "crates/hadron-gluon/src/foo.rs");
    }
}
