//! The **security_audit** family: static security vulnerability scanner, secret detector, and injection analyzer.
//!
//! Exposes `hadron_forge_security_audit` to scan source files before commit/merge for secrets,
//! command injections, path traversals, and insecure host bindings.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::security_audit::{
    run_security_audit, SecurityAuditConfig, SecuritySeverity,
};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SecurityAuditArgs {
    /// Optional specific target paths or directories to scan (defaults to entire workspace).
    #[serde(default)]
    pub target_paths: Option<Vec<String>>,
    /// Minimum severity threshold to fail the audit ("critical", "high", "medium", "low", "info"). Defaults to "high".
    #[serde(default)]
    pub fail_on_severity: Option<String>,
    /// Ignore patterns or subdirectories.
    #[serde(default)]
    pub ignore_patterns: Option<Vec<String>>,
    /// Check for hardcoded API keys and credentials (defaults to true).
    #[serde(default = "default_true")]
    pub check_secrets: bool,
    /// Check for command injection and raw SQL vulnerabilities (defaults to true).
    #[serde(default = "default_true")]
    pub check_injections: bool,
    /// Check for un-jailed path traversal vulnerabilities (defaults to true).
    #[serde(default = "default_true")]
    pub check_path_traversals: bool,
    /// Check for insecure origins and dangerous JS APIs (defaults to true).
    #[serde(default = "default_true")]
    pub check_insecure_origins: bool,
}

fn default_true() -> bool {
    true
}

#[tool_router(router = security_audit_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_security_audit",
        description = "Perform a static security audit across source files to detect hardcoded API keys/secrets, command injections, SQL injections, path traversals, and insecure bindings."
    )]
    pub async fn security_audit(
        &self,
        Parameters(args): Parameters<SecurityAuditArgs>,
    ) -> Json<ToolResponse> {
        let fail_severity = args.fail_on_severity.as_deref().and_then(|s| match s {
            "critical" => Some(SecuritySeverity::Critical),
            "high" => Some(SecuritySeverity::High),
            "medium" => Some(SecuritySeverity::Medium),
            "low" => Some(SecuritySeverity::Low),
            "info" => Some(SecuritySeverity::Info),
            _ => None,
        });

        let config = SecurityAuditConfig {
            target_paths: args.target_paths,
            fail_on_severity: fail_severity,
            ignore_patterns: args.ignore_patterns,
            check_secrets: args.check_secrets,
            check_injections: args.check_injections,
            check_path_traversals: args.check_path_traversals,
            check_insecure_origins: args.check_insecure_origins,
        };

        match run_security_audit(&self.root, &config) {
            Ok(report) => {
                let json = serde_json::to_string_pretty(&report)
                    .unwrap_or_else(|_| report.summary.clone());
                Json(ToolResponse::success(Some(json)))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn security_audit_tool_scans_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path());

        let res = server
            .security_audit(Parameters(SecurityAuditArgs {
                target_paths: None,
                fail_on_severity: Some("high".into()),
                ignore_patterns: None,
                check_secrets: true,
                check_injections: true,
                check_path_traversals: true,
                check_insecure_origins: true,
            }))
            .await;

        assert!(res.0.ok);
        let blocks = res.0.blocks.unwrap();
        assert!(blocks.contains("scanned_files_count"));
    }
}
