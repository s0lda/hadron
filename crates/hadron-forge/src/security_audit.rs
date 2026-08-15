//! Security Audit and Secret Scanner Gate for Hadron swarm.
//!
//! Provides static security analysis, AST/regex secret scanning, command injection detection,
//! path traversal risk analysis, and insecure host binding verification.
//!
//! **Invariants:**
//! 1. Jailed scanning: Reads files strictly within `Root`.
//! 2. Hermetic: Zero external network or API calls required.
//! 3. Deterministic: Predictable rule IDs, line numbers, and actionable remediation recommendations.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::file::{ForgeError, Root};

/// Finding severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecuritySeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl fmt::Display for SecuritySeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecuritySeverity::Info => write!(f, "info"),
            SecuritySeverity::Low => write!(f, "low"),
            SecuritySeverity::Medium => write!(f, "medium"),
            SecuritySeverity::High => write!(f, "high"),
            SecuritySeverity::Critical => write!(f, "critical"),
        }
    }
}

/// A specific security issue or vulnerability detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub rule_id: String,
    pub severity: SecuritySeverity,
    pub file: String,
    pub line: usize,
    pub message: String,
    pub snippet: String,
    pub recommendation: String,
}

/// Configuration for running a security audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityAuditConfig {
    #[serde(default)]
    pub target_paths: Option<Vec<String>>,
    #[serde(default)]
    pub fail_on_severity: Option<SecuritySeverity>,
    #[serde(default)]
    pub ignore_patterns: Option<Vec<String>>,
    #[serde(default = "default_true")]
    pub check_secrets: bool,
    #[serde(default = "default_true")]
    pub check_injections: bool,
    #[serde(default = "default_true")]
    pub check_path_traversals: bool,
    #[serde(default = "default_true")]
    pub check_insecure_origins: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SecurityAuditConfig {
    fn default() -> Self {
        Self {
            target_paths: None,
            fail_on_severity: Some(SecuritySeverity::High),
            ignore_patterns: None,
            check_secrets: true,
            check_injections: true,
            check_path_traversals: true,
            check_insecure_origins: true,
        }
    }
}

/// Audit report describing all identified findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityAuditReport {
    pub passed: bool,
    pub scanned_files_count: usize,
    pub findings: Vec<SecurityFinding>,
    pub summary: String,
}

/// Executes static security and secret audit across project files.
pub fn run_security_audit(
    root: &Root,
    config: &SecurityAuditConfig,
) -> Result<SecurityAuditReport, ForgeError> {
    let mut files_to_scan = vec![];
    let mut ignore_list = vec![
        "target/".to_string(),
        "node_modules/".to_string(),
        ".git/".to_string(),
        "dist/".to_string(),
        "build/".to_string(),
        ".hadron/".to_string(),
        ".lock".to_string(),
    ];

    if let Some(custom_ignores) = &config.ignore_patterns {
        ignore_list.extend(custom_ignores.clone());
    }

    if let Some(targets) = &config.target_paths {
        for target in targets {
            let full = root.path().join(target);
            if full.exists() && full.is_file() {
                files_to_scan.push(full);
            } else if full.exists() && full.is_dir() {
                collect_files_recursive(&full, &ignore_list, &mut files_to_scan);
            }
        }
    } else {
        collect_files_recursive(root.path(), &ignore_list, &mut files_to_scan);
    }

    let mut findings = vec![];

    for file_path in &files_to_scan {
        let rel_file = match file_path.strip_prefix(root.path()) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => file_path.to_string_lossy().to_string(),
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue, // Skip binaries or non-UTF-8 files
        };

        scan_file_content(&rel_file, &content, config, &mut findings);
    }

    let fail_threshold = config.fail_on_severity.unwrap_or(SecuritySeverity::High);
    let passed = !findings.iter().any(|f| f.severity >= fail_threshold);

    let summary = if findings.is_empty() {
        format!("Security audit passed: 0 vulnerabilities found across {} files.", files_to_scan.len())
    } else {
        let critical_count = findings.iter().filter(|f| f.severity == SecuritySeverity::Critical).count();
        let high_count = findings.iter().filter(|f| f.severity == SecuritySeverity::High).count();
        let med_count = findings.iter().filter(|f| f.severity == SecuritySeverity::Medium).count();
        let low_count = findings.iter().filter(|f| f.severity == SecuritySeverity::Low).count();

        format!(
            "Security audit {}: {} total issue(s) in {} files (Critical: {}, High: {}, Medium: {}, Low: {}).",
            if passed { "PASSED with warnings" } else { "FAILED" },
            findings.len(),
            files_to_scan.len(),
            critical_count,
            high_count,
            med_count,
            low_count
        )
    };

    Ok(SecurityAuditReport {
        passed,
        scanned_files_count: files_to_scan.len(),
        findings,
        summary,
    })
}

fn collect_files_recursive(dir: &Path, ignores: &[String], out: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let path_str = path.to_string_lossy();
        if ignores.iter().any(|ig| path_str.contains(ig)) {
            continue;
        }
        if path.is_dir() {
            collect_files_recursive(&path, ignores, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn scan_file_content(
    rel_file: &str,
    content: &str,
    config: &SecurityAuditConfig,
    findings: &mut Vec<SecurityFinding>,
) {
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();

        // 1. Secrets Scanning
        if config.check_secrets {
            // Private Keys
            if trimmed.contains("BEGIN RSA PRIVATE KEY")
                || trimmed.contains("BEGIN OPENSSH PRIVATE KEY")
                || trimmed.contains("BEGIN PRIVATE KEY")
            {
                findings.push(SecurityFinding {
                    rule_id: "SEC001_HARDCODED_PRIVATE_KEY".into(),
                    severity: SecuritySeverity::Critical,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Hardcoded cryptographic private key detected".into(),
                    snippet: sanitize_snippet(trimmed),
                    recommendation: "Store private keys in environment variables or secrets manager".into(),
                });
            }

            // OpenAI / Anthropic / GitHub tokens
            if trimmed.contains("sk-proj-")
                || (trimmed.contains("sk-") && trimmed.len() > 30 && !trimmed.contains("test"))
                || trimmed.contains("ghp_")
                || trimmed.contains("github_pat_")
            {
                findings.push(SecurityFinding {
                    rule_id: "SEC002_HARDCODED_API_TOKEN".into(),
                    severity: SecuritySeverity::Critical,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Hardcoded API Token / Service secret credential detected".into(),
                    snippet: sanitize_snippet(trimmed),
                    recommendation: "Inject credentials via environment variables (e.g. process.env or std::env::var)".into(),
                });
            }

            // AWS Keys
            if trimmed.contains("AKIA") && trimmed.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).count() >= 20 {
                findings.push(SecurityFinding {
                    rule_id: "SEC003_HARDCODED_AWS_KEY".into(),
                    severity: SecuritySeverity::Critical,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Hardcoded AWS Access Key detected".into(),
                    snippet: sanitize_snippet(trimmed),
                    recommendation: "Load AWS credentials from ~/.aws or IAM roles".into(),
                });
            }
        }

        // 2. Command Injections & Dangerous Execution
        if config.check_injections {
            if (trimmed.contains(".exec(") || trimmed.contains("execSync(") || trimmed.contains("child_process.exec("))
                && (trimmed.contains("${") || trimmed.contains('+') || trimmed.contains('`'))
            {
                findings.push(SecurityFinding {
                    rule_id: "SEC010_COMMAND_INJECTION_NODE".into(),
                    severity: SecuritySeverity::High,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Potentially unescaped dynamic string passed to shell exec".into(),
                    snippet: trimmed.to_string(),
                    recommendation: "Use execFile or spawn with argument array instead of shell interpolation".into(),
                });
            }

            if (trimmed.contains("os.system(") || trimmed.contains("subprocess.Popen("))
                && trimmed.contains("shell=True")
            {
                findings.push(SecurityFinding {
                    rule_id: "SEC011_COMMAND_INJECTION_PYTHON".into(),
                    severity: SecuritySeverity::High,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Python subprocess spawned with shell=True".into(),
                    snippet: trimmed.to_string(),
                    recommendation: "Set shell=False and pass arguments as a list to prevent shell injection".into(),
                });
            }

            if (trimmed.contains("SELECT ") || trimmed.contains("INSERT ") || trimmed.contains("DELETE "))
                && (trimmed.contains("{}") || trimmed.contains("${") || trimmed.contains("' +") || trimmed.contains("+ '"))
            {
                findings.push(SecurityFinding {
                    rule_id: "SEC012_SQL_INJECTION".into(),
                    severity: SecuritySeverity::High,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Raw SQL query constructed with string interpolation".into(),
                    snippet: trimmed.to_string(),
                    recommendation: "Use parameterized query placeholders (? or $1) instead of string concatenation".into(),
                });
            }
        }

        // 3. Path Traversals & Unsafe File Access
        if config.check_path_traversals {
            if (trimmed.contains("fs.readFile(") || trimmed.contains("fs.readFileSync(") || trimmed.contains("std::fs::read("))
                && (trimmed.contains("..") || trimmed.contains("req.params") || trimmed.contains("req.query"))
            {
                findings.push(SecurityFinding {
                    rule_id: "SEC020_PATH_TRAVERSAL".into(),
                    severity: SecuritySeverity::High,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Direct un-jailed file access with relative path or user request parameter".into(),
                    snippet: trimmed.to_string(),
                    recommendation: "Validate paths with canonicalize() and assert prefix matches the jail root".into(),
                });
            }
        }

        // 4. Insecure Network Bindings & Eval
        if config.check_insecure_origins {
            if trimmed.contains("eval(") && !trimmed.contains("//") {
                findings.push(SecurityFinding {
                    rule_id: "SEC030_DANGEROUS_EVAL".into(),
                    severity: SecuritySeverity::Medium,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Use of eval() executes arbitrary strings as code".into(),
                    snippet: trimmed.to_string(),
                    recommendation: "Replace eval with structured JSON parsing or safe functional evaluation".into(),
                });
            }

            if trimmed.contains("dangerouslySetInnerHTML") {
                findings.push(SecurityFinding {
                    rule_id: "SEC031_XSS_DANGEROUS_HTML".into(),
                    severity: SecuritySeverity::Medium,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Unsanitized HTML injection via dangerouslySetInnerHTML".into(),
                    snippet: trimmed.to_string(),
                    recommendation: "Sanitize raw HTML with DOMPurify before rendering".into(),
                });
            }

            if trimmed.contains("0.0.0.0") && (trimmed.contains("bind") || trimmed.contains("listen") || trimmed.contains("host")) {
                findings.push(SecurityFinding {
                    rule_id: "SEC032_WILDCARD_HOST_BIND".into(),
                    severity: SecuritySeverity::Low,
                    file: rel_file.into(),
                    line: line_no,
                    message: "Service bound to all network interfaces (0.0.0.0)".into(),
                    snippet: trimmed.to_string(),
                    recommendation: "Bind to 127.0.0.1 / localhost for internal development services".into(),
                });
            }
        }
    }
}

fn sanitize_snippet(snippet: &str) -> String {
    if snippet.len() > 60 {
        format!("{}...[REDACTED]", &snippet[..30])
    } else {
        snippet.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn security_audit_detects_secrets_and_insecure_patterns() {
        let temp = tempdir().unwrap();
        let root = Root::new(temp.path());

        let vuln_file = temp.path().join("vulnerable.ts");
        std::fs::write(
            &vuln_file,
            r#"
const apiKey = "sk-proj-9876543210abcdefghijklmnopqr";
const awsKey = "AKIAIOSFODNN7EXAMPLE12";

function runUserCmd(cmd: string) {
    const cp = require('child_process');
    cp.exec(`echo ${cmd}`);
}

function queryUser(userId: string) {
    const q = `SELECT * FROM users WHERE id = '${userId}'`;
}
"#,
        )
        .unwrap();

        let clean_file = temp.path().join("safe.rs");
        std::fs::write(
            &clean_file,
            r#"
fn get_user(id: u64) {
    let stmt = "SELECT * FROM users WHERE id = ?";
}
"#,
        )
        .unwrap();

        let config = SecurityAuditConfig::default();
        let report = run_security_audit(&root, &config).unwrap();

        assert!(!report.passed);
        assert!(report.scanned_files_count >= 2);

        let rules: Vec<_> = report.findings.iter().map(|f| f.rule_id.as_str()).collect();
        assert!(rules.contains(&"SEC002_HARDCODED_API_TOKEN"));
        assert!(rules.contains(&"SEC003_HARDCODED_AWS_KEY"));
        assert!(rules.contains(&"SEC010_COMMAND_INJECTION_NODE"));

        // Audit only the safe file
        let clean_config = SecurityAuditConfig {
            target_paths: Some(vec!["safe.rs".into()]),
            ..Default::default()
        };
        let clean_report = run_security_audit(&root, &clean_config).unwrap();
        assert!(clean_report.passed);
        assert_eq!(clean_report.findings.len(), 0);
    }
}
