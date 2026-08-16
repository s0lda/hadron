//! The **secret_vault** family: credential sandboxing, masking and leak auditing.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::secret_vault::{self, SecretVaultAction, SecretVaultReport};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SecretVaultArgs {
    pub action: String,
    pub text: Option<String>,
    pub scan_paths: Option<Vec<String>>,
}

fn format_secret_vault(report: SecretVaultReport) -> String {
    let mut out = format!("### Secret Vault Report\n\n{}\n\n", report.summary);
    if let Some(masked) = report.masked_text {
        out.push_str("#### Sanitized / Masked Text:\n```\n");
        out.push_str(&masked);
        out.push_str("\n```\n");
    }
    if !report.findings.is_empty() {
        out.push_str("#### Potential Secrets Detected:\n");
        for f in report.findings {
            out.push_str(&format!("- `{}:{}` [{}]\n  `{}`\n", f.file, f.line, f.secret_type, f.masked_snippet));
        }
    }
    out
}

#[tool_router(router = secret_vault_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_secret_vault",
        description = "Mask credentials and audit repository files for leaked secrets and unmasked API keys"
    )]
    pub async fn secret_vault(
        &self,
        Parameters(args): Parameters<SecretVaultArgs>,
    ) -> Json<ToolResponse> {
        let action = match args.action.as_str() {
            "mask_text" => SecretVaultAction::MaskText,
            "audit_repo" => SecretVaultAction::AuditRepo,
            "verify_clean" => SecretVaultAction::VerifyClean,
            other => {
                return Json(ToolResponse::error(format!(
                    "Unknown secret_vault action '{}'. Expected: mask_text, audit_repo, verify_clean",
                    other
                )))
            }
        };

        match secret_vault::run_secret_vault(
            &self.root,
            action,
            args.text.as_deref(),
            args.scan_paths,
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_secret_vault(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn secret_vault_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .secret_vault(Parameters(SecretVaultArgs {
                action: "mask_text".to_string(),
                text: Some("API Key sk-ant-1234567890abcdef".to_string()),
                scan_paths: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("[REDACTED_SECRET_"));
    }
}
