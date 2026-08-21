use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewVerdict {
    Approved,
    ChangesRequested,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewAspect {
    Security,
    Architecture,
    CodeSimplicity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineAnnotation {
    pub file: String,
    pub line: usize,
    pub comment: String,
    pub severity: String, // "info" | "warning" | "error"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AspectResult {
    pub aspect: ReviewAspect,
    pub passed: bool,
    pub score: u8, // 0 - 100
    pub summary: String,
    pub annotations: Vec<LineAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewBoardVerdict {
    pub security_pass: bool,
    pub architecture_pass: bool,
    pub code_simplicity_score: u8,
    pub line_annotations: Vec<LineAnnotation>,
    pub final_verdict: ReviewVerdict,
    pub aspect_reports: Vec<AspectResult>,
}

impl ReviewBoardVerdict {
    pub fn new(
        security_pass: bool,
        architecture_pass: bool,
        code_simplicity_score: u8,
        line_annotations: Vec<LineAnnotation>,
        aspect_reports: Vec<AspectResult>,
    ) -> Self {
        let final_verdict = if !security_pass || !architecture_pass || code_simplicity_score < 50 {
            ReviewVerdict::Blocked
        } else if line_annotations.iter().any(|a| a.severity == "error" || a.severity == "warning") {
            ReviewVerdict::ChangesRequested
        } else {
            ReviewVerdict::Approved
        };

        Self {
            security_pass,
            architecture_pass,
            code_simplicity_score,
            line_annotations,
            final_verdict,
            aspect_reports,
        }
    }

    pub fn to_markdown(&self) -> String {
        let verdict_badge = match self.final_verdict {
            ReviewVerdict::Approved => "✅ **APPROVED**",
            ReviewVerdict::ChangesRequested => "⚠️ **CHANGES REQUESTED**",
            ReviewVerdict::Blocked => "🛑 **BLOCKED**",
        };

        let mut md = format!("### Peer Review Board Verdict: {verdict_badge}\n\n");
        md.push_str("| Aspect | Status | Score |\n|---|---|---|\n");
        md.push_str(&format!(
            "| Security Review | {} | {} |\n",
            if self.security_pass { "Pass" } else { "Fail" },
            if self.security_pass { "100" } else { "0" }
        ));
        md.push_str(&format!(
            "| Architecture & SSOT | {} | {} |\n",
            if self.architecture_pass { "Pass" } else { "Fail" },
            if self.architecture_pass { "100" } else { "0" }
        ));
        md.push_str(&format!(
            "| Code Simplicity | {} | {}/100 |\n\n",
            if self.code_simplicity_score >= 50 { "Pass" } else { "Fail" },
            self.code_simplicity_score
        ));

        if !self.line_annotations.is_empty() {
            md.push_str("#### Line Annotations\n");
            for ann in &self.line_annotations {
                md.push_str(&format!(
                    "- `{}#L{}` [{}] {}\n",
                    ann.file, ann.line, ann.severity.to_uppercase(), ann.comment
                ));
            }
        }

        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_verdict_synthesis_and_markdown() {
        let annotations = vec![LineAnnotation {
            file: "crates/hadron-gluon/src/engine.rs".into(),
            line: 42,
            comment: "Avoid unchecked unwraps on lock acquisition".into(),
            severity: "warning".into(),
        }];

        let aspects = vec![
            AspectResult {
                aspect: ReviewAspect::Security,
                passed: true,
                score: 100,
                summary: "No new attack surface".into(),
                annotations: vec![],
            },
            AspectResult {
                aspect: ReviewAspect::Architecture,
                passed: true,
                score: 100,
                summary: "Adheres to SSOT".into(),
                annotations: vec![],
            },
            AspectResult {
                aspect: ReviewAspect::CodeSimplicity,
                passed: true,
                score: 85,
                summary: "Clean and concise".into(),
                annotations: annotations.clone(),
            },
        ];

        let verdict = ReviewBoardVerdict::new(true, true, 85, annotations, aspects);
        assert_eq!(verdict.final_verdict, ReviewVerdict::ChangesRequested);
        let md = verdict.to_markdown();
        assert!(md.contains("CHANGES REQUESTED"));
        assert!(md.contains("crates/hadron-gluon/src/engine.rs#L42"));
    }

    #[test]
    fn test_blocked_when_security_fails() {
        let verdict = ReviewBoardVerdict::new(false, true, 90, vec![], vec![]);
        assert_eq!(verdict.final_verdict, ReviewVerdict::Blocked);
    }
}
