//! The **e2e** family: visual and behavioral end-to-end verification.
//!
//! Exposes `hadron_forge_e2e_assert` to execute declarative browser scenarios, DOM checks,
//! and UI screenshot captures inside the worktree jail.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::e2e::{run_e2e_assertion_suite, E2eStep, E2eSuiteConfig};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum E2eStepArgs {
    Navigate {
        url: String,
    },
    Click {
        selector: String,
    },
    Fill {
        selector: String,
        value: String,
    },
    AssertText {
        selector: String,
        expected_contains: String,
    },
    AssertElementExists {
        selector: String,
    },
    AssertStatusCode {
        url: String,
        expected_status: u16,
    },
    Screenshot {
        output_path: String,
    },
    EvaluateScript {
        script: String,
        #[serde(default)]
        expected_contains: Option<String>,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct E2eAssertArgs {
    /// Test scenario or suite title.
    pub name: String,
    /// Ordered list of verification steps.
    pub steps: Vec<E2eStepArgs>,
    /// Optional base URL prefix (e.g. "http://localhost:3000").
    #[serde(default)]
    pub base_url: Option<String>,
    /// Optional timeout in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[tool_router(router = e2e_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_e2e_assert",
        description = "Execute a multi-step declarative E2E assertion suite (browser navigation, element clicks, form fills, text assertions, and screenshot validation)."
    )]
    pub async fn e2e_assert(
        &self,
        Parameters(args): Parameters<E2eAssertArgs>,
    ) -> Json<ToolResponse> {
        let steps = args
            .steps
            .into_iter()
            .map(|s| match s {
                E2eStepArgs::Navigate { url } => E2eStep::Navigate { url },
                E2eStepArgs::Click { selector } => E2eStep::Click { selector },
                E2eStepArgs::Fill { selector, value } => E2eStep::Fill { selector, value },
                E2eStepArgs::AssertText {
                    selector,
                    expected_contains,
                } => E2eStep::AssertText {
                    selector,
                    expected_contains,
                },
                E2eStepArgs::AssertElementExists { selector } => {
                    E2eStep::AssertElementExists { selector }
                }
                E2eStepArgs::AssertStatusCode {
                    url,
                    expected_status,
                } => E2eStep::AssertStatusCode {
                    url,
                    expected_status,
                },
                E2eStepArgs::Screenshot { output_path } => {
                    E2eStep::Screenshot { output_path }
                }
                E2eStepArgs::EvaluateScript {
                    script,
                    expected_contains,
                } => E2eStep::EvaluateScript {
                    script,
                    expected_contains,
                },
            })
            .collect();

        let config = E2eSuiteConfig {
            name: args.name,
            steps,
            base_url: args.base_url,
            timeout_ms: args.timeout_ms,
        };

        match run_e2e_assertion_suite(&self.root, &config) {
            Ok(report) => {
                let json = serde_json::to_string_pretty(&report)
                    .unwrap_or_else(|_| report.summary.clone());
                if report.ok {
                    Json(ToolResponse::success(Some(json)))
                } else {
                    Json(ToolResponse::error(format!(
                        "{}: {}",
                        report.summary, json
                    )))
                }
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn e2e_tool_runs_assertions_and_captures_screenshot() {
        let temp = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path());

        std::fs::write(
            temp.path().join("index.html"),
            "<h1>Welcome</h1><button id=\"submit\">Submit</button>",
        )
        .unwrap();

        let res = server
            .e2e_assert(Parameters(E2eAssertArgs {
                name: "Basic Page Check".into(),
                base_url: None,
                timeout_ms: Some(2000),
                steps: vec![
                    E2eStepArgs::Navigate {
                        url: "file://index.html".into(),
                    },
                    E2eStepArgs::AssertElementExists {
                        selector: "h1".into(),
                    },
                    E2eStepArgs::AssertText {
                        selector: "h1".into(),
                        expected_contains: "Welcome".into(),
                    },
                    E2eStepArgs::Click {
                        selector: "submit".into(),
                    },
                    E2eStepArgs::Screenshot {
                        output_path: "verify_page.png".into(),
                    },
                ],
            }))
            .await;

        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Basic Page Check"));
        assert!(temp
            .path()
            .join(".hadron/screenshots/verify_page.png")
            .exists());
    }
}
