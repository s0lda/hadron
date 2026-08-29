#[derive(Debug, Clone)]
pub struct RedTeamReport {
    pub passed: bool,
    pub findings: Vec<String>,
}

pub struct RedTeamAuditor;

impl RedTeamAuditor {
    pub fn audit_diff(diff: &str) -> RedTeamReport {
        let mut findings = Vec::new();
        for line in diff.lines() {
            if line.starts_with('+')
                && (line.contains("sk-")
                    || line.contains("ghp_")
                    || line.contains("AWS_SECRET"))
            {
                findings.push(format!("Potential hardcoded secret detected: {}", line));
            }
        }
        let passed = findings.is_empty();
        RedTeamReport { passed, findings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_red_team_secrets_and_hardcoded_keys() {
        let diff = "+ let api_key = \"sk-ant-1234567890abcdef\";";
        let report = RedTeamAuditor::audit_diff(diff);
        assert!(!report.passed);
        assert!(report.findings[0].contains("Potential hardcoded secret"));
    }
}
