//! The **pty_pairing** tool: shared terminal session inspection and live annotations.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::pty_pairing::{PtyAnnotation, PtyPairingBroker};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PtyPairingArgs {
    pub action: String, // "annotate" | "list_annotations"
    pub session_id: String,
    pub author_quark: Option<String>,
    pub row: Option<usize>,
    pub col: Option<usize>,
    pub text: Option<String>,
    pub color_hint: Option<String>,
}

#[tool_router(router = pty_pairing_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_pty_pairing",
        description = "Manage multi-quark paired PTY sessions and live terminal annotations"
    )]
    pub async fn pty_pairing(
        &self,
        Parameters(args): Parameters<PtyPairingArgs>,
    ) -> Json<ToolResponse> {
        let mut broker = PtyPairingBroker::new();
        match args.action.as_str() {
            "annotate" => {
                let annotation = PtyAnnotation {
                    annotation_id: format!("ann-{}", hadron_lattice::Ulid::new()),
                    author_quark: args.author_quark.unwrap_or_else(|| "unknown".into()),
                    row: args.row.unwrap_or(0),
                    col: args.col.unwrap_or(0),
                    text: args.text.unwrap_or_default(),
                    color_hint: args.color_hint,
                };
                broker.add_annotation(&args.session_id, annotation.clone());
                match serde_json::to_string_pretty(&annotation) {
                    Ok(json) => Json(ToolResponse::success(Some(json))),
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "list_annotations" => {
                let annotations = broker.get_annotations(&args.session_id);
                match serde_json::to_string_pretty(&annotations) {
                    Ok(json) => Json(ToolResponse::success(Some(json))),
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            other => Json(ToolResponse::error(format!("Unknown action: {}", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pty_pairing_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .pty_pairing(Parameters(PtyPairingArgs {
                action: "annotate".into(),
                session_id: "pty-1".into(),
                author_quark: Some("agy".into()),
                row: Some(1),
                col: Some(1),
                text: Some("hello".into()),
                color_hint: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("ann-"));
    }
}
