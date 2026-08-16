//! Pure logic for the `secret_vault` tool family.
//! Sandboxed credential proxy, log/text secret masking, and repo secret leak detection.

use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};

use crate::file::{resolve_jailed_path, ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretVaultAction {
    MaskText,
    AuditRepo,
    VerifyClean,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretFinding {
    pub file: String,
    pub line: usize,
    pub secret_type: String,
    pub masked_snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretVaultReport {
    pub findings: Vec<SecretFinding>,
    pub masked_text: Option<String>,
    pub is_clean: bool,
    pub summary: String,
}

/// Known regex-like patterns for common secret keys.
const SECRET_PREFIXES: &[(&str, &str)] = &[
    ("sk-ant-", "Anthropic API Key"),
    ("sk-proj-", "OpenAI Project Key"),
    ("AIzaSy", "Google Gemini API Key"),
    ("ghp_", "GitHub Personal Access Token"),
    ("gho_", "GitHub OAuth Token"),
    ("AKIA", "AWS Access Key ID"),
    ("ASIA", "AWS Temporary Access Key"),
    ("xoxb-", "Slack Bot Token"),
    ("xoxp-", "Slack User Token"),
    ("-----BEGIN PRIVATE KEY-----", "RSA/Ed25519 Private Key"),
    ("-----BEGIN OPENSSH PRIVATE KEY-----", "OpenSSH Private Key"),
];

/// Mask potential secrets inside a string.
pub fn mask_secrets(input: &str) -> String {
    let mut result = input.to_string();

    for (prefix, _) in SECRET_PREFIXES {
        let mut start_idx = 0;
        while let Some(pos) = result[start_idx..].find(prefix) {
            let actual_pos = start_idx + pos;
            // Find end of token (whitespace, quote, newline)
            let end_pos = result[actual_pos..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';')
                .map(|e| actual_pos + e)
                .unwrap_or(result.len());

            let token_len = end_pos - actual_pos;
            if token_len > prefix.len() {
                let mask = format!("{}[REDACTED_SECRET_{}B]", prefix, token_len - prefix.len());
                result.replace_range(actual_pos..end_pos, &mask);
                start_idx = actual_pos + mask.len();
            } else {
                start_idx = actual_pos + prefix.len();
            }

            if start_idx >= result.len() {
                break;
            }
        }
    }

    result
}

/// Scan a single string or file lines for leaked secrets.
pub fn scan_for_secrets(file_rel: &str, content: &str) -> Vec<SecretFinding> {
    let mut findings = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        for (prefix, secret_type) in SECRET_PREFIXES {
            if line.contains(prefix) {
                findings.push(SecretFinding {
                    file: file_rel.to_string(),
                    line: line_no + 1,
                    secret_type: secret_type.to_string(),
                    masked_snippet: mask_secrets(line.trim()),
                });
            }
        }
    }
    findings
}

/// Audit repo files for hardcoded secrets.
pub fn audit_directory_for_secrets(root: &Root, scan_paths: Option<Vec<String>>) -> Result<Vec<SecretFinding>, ForgeError> {
    let mut all_findings = Vec::new();
    let paths_to_check = match scan_paths {
        Some(p) => p,
        None => vec![
            "src".to_string(),
            "crates".to_string(),
            "config".to_string(),
            "tests".to_string(),
        ],
    };

    for target in paths_to_check {
        let abs = match resolve_jailed_path(root, &target) {
            Ok(p) => p,
            Err(_) => continue,
        };

        if abs.is_file() {
            if let Ok(content) = fs::read_to_string(&abs) {
                all_findings.extend(scan_for_secrets(&target, &content));
            }
        } else if abs.is_dir() {
            for entry in walkdir_simple(&abs) {
                if let Ok(content) = fs::read_to_string(&entry) {
                    let rel = entry.strip_prefix(root.path()).unwrap_or(&entry).to_string_lossy();
                    all_findings.extend(scan_for_secrets(&rel, &content));
                }
            }
        }
    }

    Ok(all_findings)
}

fn walkdir_simple(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    files.extend(walkdir_simple(&path));
                }
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files
}

pub fn run_secret_vault(
    root: &Root,
    action: SecretVaultAction,
    text: Option<&str>,
    scan_paths: Option<Vec<String>>,
) -> Result<SecretVaultReport, ForgeError> {
    match action {
        SecretVaultAction::MaskText => {
            let raw = text.unwrap_or_default();
            let masked = mask_secrets(raw);
            Ok(SecretVaultReport {
                findings: vec![],
                masked_text: Some(masked),
                is_clean: true,
                summary: "Text sanitized and sensitive credentials masked successfully.".to_string(),
            })
        }
        SecretVaultAction::AuditRepo | SecretVaultAction::VerifyClean => {
            let findings = audit_directory_for_secrets(root, scan_paths)?;
            let is_clean = findings.is_empty();
            let summary = if is_clean {
                "Secret Audit Clean: No unmasked API keys, tokens, or private credentials detected in audited paths.".to_string()
            } else {
                format!("Secret Audit Warning: Found {} potential leaked secret(s) in codebase!", findings.len())
            };
            Ok(SecretVaultReport {
                findings,
                masked_text: None,
                is_clean,
                summary,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_secrets_redacts_known_keys() {
        let text = "My token is ghp_1234567890abcdef1234567890 and key is AIzaSyDummyKeyXYZ123 end.";
        let masked = mask_secrets(text);
        assert!(!masked.contains("ghp_1234567890abcdef1234567890"));
        assert!(!masked.contains("AIzaSyDummyKeyXYZ123"));
        assert!(masked.contains("ghp_[REDACTED_SECRET_"));
        assert!(masked.contains("AIzaSy[REDACTED_SECRET_"));
    }
}
