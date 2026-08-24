//! The **vcr** family: deterministic HTTP/RPC cassette recording and offline replay proxy.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::vcr::{self, VcrMode};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VcrArgs {
    pub action: String,
    pub cassette_name: Option<String>,
    pub port: Option<u16>,
    pub mode: Option<String>,
}

#[tool_router(router = vcr_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_vcr",
        description = "Deterministic HTTP/RPC cassette recorder and offline replay proxy for hermetic integration testing"
    )]
    pub async fn vcr(&self, Parameters(args): Parameters<VcrArgs>) -> Json<ToolResponse> {
        let action = args.action.as_str();
        match action {
            "start_proxy" | "record_cassette" | "replay_cassette" => {
                let name = match args.cassette_name {
                    Some(n) => n,
                    None => {
                        return Json(ToolResponse::error(
                            "cassette_name is required to start VCR proxy",
                        ))
                    }
                };
                let mode = if action == "replay_cassette" || args.mode.as_deref() == Some("replay") {
                    VcrMode::Replay
                } else if args.mode.as_deref() == Some("verify") {
                    VcrMode::Verify
                } else {
                    VcrMode::Record
                };

                match self
                    .vcr_manager
                    .start_proxy(self.root.clone(), name, mode, args.port)
                    .await
                {
                    Ok(summary) => {
                        let text = format!(
                            "### VCR Proxy Started\n\n- **Port:** `{}`\n- **Proxy URL:** `{}`\n- **Cassette Name:** `{}`\n- **Mode:** `{:?}`\n- **Status:** Running\n",
                            summary.port, summary.url, summary.cassette_name, summary.mode
                        );
                        Json(ToolResponse::success(Some(text)))
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "stop_proxy" => {
                let port = match args.port {
                    Some(p) => p,
                    None => return Json(ToolResponse::error("port is required to stop VCR proxy")),
                };
                match self.vcr_manager.stop_proxy(port).await {
                    Ok(stopped) => {
                        if stopped {
                            Json(ToolResponse::success(Some(format!(
                                "Stopped VCR proxy on port {port}."
                            ))))
                        } else {
                            Json(ToolResponse::error(format!(
                                "No active VCR proxy running on port {port}"
                            )))
                        }
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "list_cassettes" => match vcr::list_cassettes(&self.root) {
                Ok(list) => {
                    let mut text = format!("### Saved VCR Cassettes ({})\n\n", list.len());
                    for c in list {
                        text.push_str(&format!(
                            "- `{}` — {} interactions recorded\n",
                            c.name, c.interactions_count
                        ));
                    }
                    Json(ToolResponse::success(Some(text)))
                }
                Err(e) => Json(ToolResponse::error(e.to_string())),
            },
            other => Json(ToolResponse::error(format!(
                "Unknown VCR action '{}'. Expected: start_proxy, record_cassette, replay_cassette, stop_proxy, list_cassettes",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn vcr_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());

        let res = server
            .vcr(Parameters(VcrArgs {
                action: "list_cassettes".to_string(),
                cassette_name: None,
                port: None,
                mode: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Saved VCR Cassettes"));
    }
}
