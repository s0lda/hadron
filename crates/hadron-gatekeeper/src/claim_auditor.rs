//! Cross-check a quark's reported evidence against what the merge gate observed.
//!
//! Advisory: it reads prose, so it can only say whether a *test-result claim* in the
//! report agrees with the gate — never that the work is right.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimVerdict {
    /// The report's pass/fail claim agrees with the gate.
    Verified,
    /// No test-result claim to check, or a failure claim the gate did not reproduce.
    Unverified,
    /// The report claims a pass the gate did not reproduce.
    Contradicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimAuditReport {
    pub verdict: ClaimVerdict,
    pub claimed_command: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    Pass,
    Fail,
}

fn fail_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"test result: FAILED|\b[1-9][0-9]* failed\b|\bFAILED\b|panicked at").expect("static regex"))
}

fn pass_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"test result: ok|\b0 failed\b").expect("static regex"))
}

/// The `**Evidence**` bullet (up to the next `**…**` bullet), or the whole body
/// when the report has no such section.
fn evidence_section(body: &str) -> &str {
    let Some(start) = body.find("**Evidence**") else { return body };
    let rest = &body[start + "**Evidence**".len()..];
    let end = rest
        .match_indices('\n')
        .map(|(i, _)| i)
        .find(|&i| {
            let next = rest[i + 1..].trim_start();
            next.starts_with("- **") || next.starts_with("**")
        })
        .unwrap_or(rest.len());
    &rest[..end]
}

const TEST_RUNNERS: [&str; 6] = ["cargo test", "cargo nextest", "pytest", "npm test", "go test", "cargo check"];

fn claimed_command(evidence: &str) -> Option<String> {
    evidence
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .find(|span| TEST_RUNNERS.iter().any(|r| span.contains(r)))
        .map(str::to_string)
}

fn normalize(cmd: &str) -> String {
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Compare the test-result claim in a quark's report with the gate outcome.
pub fn audit_evidence_claims(message_body: &str, gate_passed: bool, gate_command: &str) -> ClaimAuditReport {
    let evidence = evidence_section(message_body);
    let claimed_command = claimed_command(evidence);
    // Any failure marker wins: "0 failed" next to "2 failed" is a failure claim.
    let claim = if fail_re().is_match(evidence) {
        Some(Claim::Fail)
    } else if pass_re().is_match(evidence) {
        Some(Claim::Pass)
    } else {
        None
    };
    let gate = if gate_passed { "passed" } else { "failed" };
    let (verdict, mut detail) = match (claim, gate_passed) {
        (None, _) => (ClaimVerdict::Unverified, format!("no test-result claim in the report; gate {gate}")),
        (Some(Claim::Pass), true) => (ClaimVerdict::Verified, "report claims a pass; gate passed".to_string()),
        (Some(Claim::Pass), false) => {
            (ClaimVerdict::Contradicted, "report claims a pass but the gate FAILED".to_string())
        }
        (Some(Claim::Fail), false) => (ClaimVerdict::Verified, "report admits failures; gate failed".to_string()),
        (Some(Claim::Fail), true) => (
            ClaimVerdict::Unverified,
            "report cites failures the gate did not reproduce (pre-existing or a narrower command?)".to_string(),
        ),
    };
    if let Some(cmd) = &claimed_command {
        if !gate_command.trim().is_empty() && normalize(cmd) != normalize(gate_command) {
            detail.push_str(&format!("; report ran `{cmd}`, gate ran `{gate_command}`"));
        }
    }
    ClaimAuditReport { verdict, claimed_command, detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GATE: &str = "cargo test --workspace";

    fn report(evidence: &str) -> String {
        format!("@orchestrator\n**Done**: thing\n\n- **Done**: edits\n- **Evidence**: {evidence}\n- **What I did not verify**: 3 failed upstream\n")
    }

    #[test]
    fn pass_claim_matches_or_contradicts_the_gate() {
        let body = report("`cargo test --workspace` → test result: ok. 12 passed; 0 failed");
        let ok = audit_evidence_claims(&body, true, GATE);
        assert_eq!(ok.verdict, ClaimVerdict::Verified);
        assert_eq!(ok.claimed_command.as_deref(), Some("cargo test --workspace"));
        assert!(!ok.detail.contains("gate ran"), "same command: {}", ok.detail);

        let bad = audit_evidence_claims(&body, false, GATE);
        assert_eq!(bad.verdict, ClaimVerdict::Contradicted);
    }

    #[test]
    fn failure_claims_and_missing_claims() {
        let admits = report("`cargo test -p x` → 1 failed (pre-existing), 0 failed in my crate");
        let r = audit_evidence_claims(&admits, false, GATE);
        assert_eq!(r.verdict, ClaimVerdict::Verified);
        assert!(r.detail.contains("report ran `cargo test -p x`"), "{}", r.detail);
        assert_eq!(audit_evidence_claims(&admits, true, GATE).verdict, ClaimVerdict::Unverified);

        let none = report("looked at it, seems fine");
        let r = audit_evidence_claims(&none, true, GATE);
        assert_eq!((r.verdict, r.claimed_command), (ClaimVerdict::Unverified, None));
    }

    #[test]
    fn only_the_evidence_bullet_is_read_when_present() {
        // "3 failed upstream" lives in a later bullet and must not flip the claim.
        let body = report("test result: ok. 5 passed; 0 failed");
        assert_eq!(audit_evidence_claims(&body, true, GATE).verdict, ClaimVerdict::Verified);
        // Without an Evidence bullet the whole body is read.
        assert_eq!(audit_evidence_claims("all good: test result: ok", true, "").verdict, ClaimVerdict::Verified);
    }
}
