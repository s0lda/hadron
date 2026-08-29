//! Automated Pre-Gate Red Teaming (`@auditor`).
//!
//! Automated pre-merge audit quark that runs static analysis, secret scanning,
//! safety pattern detection, and invariant verification before queuing changes
//! for the Merge Gate.

use serde::{Deserialize, Serialize};

/// Severity level of an audit finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FindingSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Category of an audit finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FindingCategory {
    SecretLeak,
    CommandInjection,
    PathTraversal,
    InvariantViolation,
    ErrorSuppression,
    UnsafePattern,
    MissingVerification,
}

/// A specific finding identified during pre-gate red team analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedTeamFinding {
    pub severity: FindingSeverity,
    pub category: FindingCategory,
    pub file: String,
    pub line_number: Option<usize>,
    pub snippet: String,
    pub description: String,
    pub remediation: String,
}

/// The final verdict rendered by the Red Team Auditor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditVerdict {
    Approved,
    ApprovedWithWarnings { medium_count: usize, low_count: usize },
    Rejected { critical_count: usize, high_count: usize },
}

/// Complete report produced by the Red Team Auditor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedTeamReport {
    pub passed: bool,
    pub verdict: AuditVerdict,
    pub findings: Vec<RedTeamFinding>,
    pub files_audited: usize,
    pub lines_audited: usize,
}

impl RedTeamReport {
    /// Formats the audit report as Markdown.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Pre-Gate Red Team Audit Report\n\n");

        match &self.verdict {
            AuditVerdict::Approved => {
                out.push_str("✅ **Verdict: APPROVED** — Zero critical or high-risk findings detected.\n\n");
            }
            AuditVerdict::ApprovedWithWarnings { medium_count, low_count } => {
                out.push_str(&format!(
                    "⚠️ **Verdict: APPROVED WITH WARNINGS** — {} medium, {} low findings detected.\n\n",
                    medium_count, low_count
                ));
            }
            AuditVerdict::Rejected { critical_count, high_count } => {
                out.push_str(&format!(
                    "🚫 **Verdict: REJECTED** — {} critical, {} high severity findings must be resolved before Merge Gate queueing.\n\n",
                    critical_count, high_count
                ));
            }
        }

        out.push_str(&format!(
            "**Metrics**: {} files audited | {} lines evaluated | {} total findings\n\n",
            self.files_audited,
            self.lines_audited,
            self.findings.len()
        ));

        if !self.findings.is_empty() {
            out.push_str("| Severity | Category | File:Line | Description | Remediation |\n");
            out.push_str("|---|---|---|---|---|\n");
            for f in &self.findings {
                let sev_str = match f.severity {
                    FindingSeverity::Critical => "🔴 **CRITICAL**",
                    FindingSeverity::High => "🟠 **HIGH**",
                    FindingSeverity::Medium => "🟡 MEDIUM",
                    FindingSeverity::Low => "⚪ LOW",
                };
                let loc = match f.line_number {
                    Some(l) => format!("`{}:{}`", f.file, l),
                    None => format!("`{}`", f.file),
                };
                out.push_str(&format!(
                    "| {} | `{:?}` | {} | {} | {} |\n",
                    sev_str, f.category, loc, f.description, f.remediation
                ));
            }
            out.push('\n');
        }

        out
    }
}

/// Automated Red Team Auditor evaluating diffs and source trees.
#[derive(Debug, Clone, Default)]
pub struct RedTeamAuditor;

impl RedTeamAuditor {
    pub fn new() -> Self {
        Self
    }

    /// Audit a unified git diff before merge gating.
    pub fn audit_diff(&self, diff: &str) -> RedTeamReport {
        let mut findings = Vec::new();
        let mut files_audited = 0;
        let mut lines_audited = 0;
        let mut current_file = String::from("unknown");
        let mut current_line = 0;

        for line in diff.lines() {
            lines_audited += 1;

            if line.starts_with("+++ b/") || line.starts_with("+++ ") {
                current_file = line
                    .trim_start_matches("+++ b/")
                    .trim_start_matches("+++ ")
                    .trim()
                    .to_string();
                files_audited += 1;
                current_line = 0;
                continue;
            }

            if line.starts_with("@@ ") {
                // Parse line number from @@ -a,b +c,d @@
                if let Some(plus_idx) = line.find('+') {
                    let num_part = &line[plus_idx + 1..];
                    let num_str = num_part.split([',', ' ']).next().unwrap_or("0");
                    current_line = num_str.parse::<usize>().unwrap_or(0);
                }
                continue;
            }

            if line.starts_with('+') && !line.starts_with("+++") {
                current_line += 1;
                let added_text = &line[1..];
                self.scan_line(&current_file, current_line, added_text, &mut findings);
            } else if !line.starts_with('-') {
                current_line += 1;
            }
        }

        if files_audited == 0 && lines_audited > 0 {
            files_audited = 1;
        }

        let mut critical_count = 0;
        let mut high_count = 0;
        let mut medium_count = 0;
        let mut low_count = 0;

        for f in &findings {
            match f.severity {
                FindingSeverity::Critical => critical_count += 1,
                FindingSeverity::High => high_count += 1,
                FindingSeverity::Medium => medium_count += 1,
                FindingSeverity::Low => low_count += 1,
            }
        }

        let passed = critical_count == 0 && high_count == 0;
        let verdict = if !passed {
            AuditVerdict::Rejected {
                critical_count,
                high_count,
            }
        } else if medium_count > 0 || low_count > 0 {
            AuditVerdict::ApprovedWithWarnings {
                medium_count,
                low_count,
            }
        } else {
            AuditVerdict::Approved
        };

        RedTeamReport {
            passed,
            verdict,
            findings,
            files_audited,
            lines_audited,
        }
    }

    fn scan_line(&self, file: &str, line_no: usize, text: &str, findings: &mut Vec<RedTeamFinding>) {
        let trimmed = text.trim();

        // 1. Secret & Key Leakage
        if trimmed.contains("sk-ant-")
            || trimmed.contains("sk-proj-")
            || trimmed.contains("ghp_")
            || trimmed.contains("gho_")
            || trimmed.contains("xoxb-")
            || trimmed.contains("glpat-")
            || (trimmed.contains("AKIA") && trimmed.len() >= 20)
            || trimmed.contains("AWS_SECRET_ACCESS_KEY")
            || trimmed.contains("-----BEGIN RSA PRIVATE KEY-----")
            || trimmed.contains("-----BEGIN OPENSSH PRIVATE KEY-----")
        {
            findings.push(RedTeamFinding {
                severity: FindingSeverity::Critical,
                category: FindingCategory::SecretLeak,
                file: file.to_string(),
                line_number: Some(line_no),
                snippet: trimmed.to_string(),
                description: "Potential hardcoded secret or API credential detected in code.".to_string(),
                remediation: "Extract secrets to environment variables or the KeyringStore.".to_string(),
            });
        }

        // 2. Command Injection
        if (trimmed.contains("Command::new(\"sh\").arg(\"-c\")")
            || trimmed.contains("Command::new(\"bash\").arg(\"-c\")"))
            && (trimmed.contains("format!(") || trimmed.contains("+"))
        {
            findings.push(RedTeamFinding {
                severity: FindingSeverity::Critical,
                category: FindingCategory::CommandInjection,
                file: file.to_string(),
                line_number: Some(line_no),
                snippet: trimmed.to_string(),
                description: "Dynamic string interpolation inside shell command execution.".to_string(),
                remediation: "Pass arguments individually via `.arg(...)` or quote strictly.".to_string(),
            });
        }

        // 3. Dangerous Path Traversal
        if (trimmed.contains(".join(\"..\")") || trimmed.contains(".join(\"../") || trimmed.contains("Path::new(\"../"))
            && !file.contains("test")
        {
            findings.push(RedTeamFinding {
                severity: FindingSeverity::High,
                category: FindingCategory::PathTraversal,
                file: file.to_string(),
                line_number: Some(line_no),
                snippet: trimmed.to_string(),
                description: "Unsanitized relative path traversal `..` detected.".to_string(),
                remediation: "Enforce root containment with `dunce::canonicalize` or `Root::contains`.".to_string(),
            });
        }

        // 4. Critical Error Suppression / Swallowed Locks
        if trimmed.starts_with("let _ =")
            && (trimmed.contains(".lock().unwrap()")
                || trimmed.contains("fs::remove_dir_all")
                || trimmed.contains("fs::remove_file"))
        {
            findings.push(RedTeamFinding {
                severity: FindingSeverity::High,
                category: FindingCategory::ErrorSuppression,
                file: file.to_string(),
                line_number: Some(line_no),
                snippet: trimmed.to_string(),
                description: "Swallowed error on critical filesystem or lock operation (Rule 8).".to_string(),
                remediation: "Handle or propagate the `Result` with `?` instead of ignoring.".to_string(),
            });
        }

        // 5. Invariant Violations: GPUI Font Family Comma Stack
        if trimmed.contains("font_family") && trimmed.contains(',') && !trimmed.contains("//") {
            findings.push(RedTeamFinding {
                severity: FindingSeverity::Medium,
                category: FindingCategory::InvariantViolation,
                file: file.to_string(),
                line_number: Some(line_no),
                snippet: trimmed.to_string(),
                description: "GPUI font_family must be ONE literal family name, never a comma-separated CSS stack.".to_string(),
                remediation: "Use `app/mod.rs::font_family_with_a_real_bold` to match platform font DB.".to_string(),
            });
        }

        // 6. Invariant Violations: Absolute coordinate conflicts
        if trimmed.contains(".absolute()") && trimmed.contains(".left_0()") {
            findings.push(RedTeamFinding {
                severity: FindingSeverity::Medium,
                category: FindingCategory::InvariantViolation,
                file: file.to_string(),
                line_number: Some(line_no),
                snippet: trimmed.to_string(),
                description: "Absolute completion overlay with `left_0` blocks mouse events on parent.".to_string(),
                remediation: "Omit `left_0` on absolute overlays per invariant rules.".to_string(),
            });
        }

        // 7. Memory Leak Pattern
        if trimmed.contains("std::mem::forget(") || trimmed.contains("Box::leak(") {
            findings.push(RedTeamFinding {
                severity: FindingSeverity::Low,
                category: FindingCategory::UnsafePattern,
                file: file.to_string(),
                line_number: Some(line_no),
                snippet: trimmed.to_string(),
                description: "Manual resource leak pattern detected.".to_string(),
                remediation: "Use RAII Drop implementations to manage lifecycle deterministically.".to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_red_team_secrets_and_hardcoded_keys() {
        let diff = r#"
+++ b/crates/hadron-gluon/src/client.rs
@@ -10,2 +10,3 @@
+ let api_key = "sk-ant-1234567890abcdef1234567890";
+ let ghp = "ghp_123456789012345678901234567890123456";
"#;
        let auditor = RedTeamAuditor::new();
        let report = auditor.audit_diff(diff);
        assert!(!report.passed);
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.findings[0].category, FindingCategory::SecretLeak);
        assert_eq!(report.findings[0].severity, FindingSeverity::Critical);
        assert!(matches!(report.verdict, AuditVerdict::Rejected { critical_count: 2, .. }));
    }

    #[test]
    fn test_red_team_command_injection_and_swallowed_locks() {
        let diff = r#"
+++ b/crates/hadron-chamber/src/sys.rs
@@ -20,2 +20,4 @@
+ let _ = Command::new("sh").arg("-c").arg(format!("rm -rf {}", user_input));
+ let _ = mutex.lock().unwrap();
"#;
        let auditor = RedTeamAuditor::new();
        let report = auditor.audit_diff(diff);
        assert!(!report.passed);
        assert_eq!(report.findings.len(), 2);
        assert!(report.findings.iter().any(|f| f.category == FindingCategory::CommandInjection));
        assert!(report.findings.iter().any(|f| f.category == FindingCategory::ErrorSuppression));
    }

    #[test]
    fn test_red_team_invariant_violations() {
        let diff = r#"
+++ b/crates/hadron-chamber/src/theme.rs
@@ -5,2 +5,3 @@
+ let font_family = "Inter, -apple-system, sans-serif";
"#;
        let auditor = RedTeamAuditor::new();
        let report = auditor.audit_diff(diff);
        assert!(report.passed, "Medium finding should allow pass with warnings");
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].category, FindingCategory::InvariantViolation);
        assert!(matches!(report.verdict, AuditVerdict::ApprovedWithWarnings { medium_count: 1, .. }));

        let md = report.to_markdown();
        assert!(md.contains("APPROVED WITH WARNINGS"));
        assert!(md.contains("font_family"));
    }

    #[test]
    fn test_red_team_clean_diff_approval() {
        let diff = r#"
+++ b/crates/hadron-lattice/src/event.rs
@@ -100,2 +100,3 @@
+ let valid_event = Event::new("turn_complete");
"#;
        let auditor = RedTeamAuditor::new();
        let report = auditor.audit_diff(diff);
        assert!(report.passed);
        assert_eq!(report.verdict, AuditVerdict::Approved);
        assert!(report.findings.is_empty());
    }
}
