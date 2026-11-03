//! The **clipboard** family: shared inter-quark artifact clipboard (`hadron_lattice::clipboard`).
//!
//! Allows quarks across isolated git worktrees to store, retrieve, and list named text/code/JSON
//! clips in `.hadron/clipboard/clips.json` without cluttering the field transcript.

use super::{ForgeMcpServer, ToolResponse};
use hadron_lattice::clipboard::{get_clip, list_clips, put_clip};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipPutArgs {
    /// Unique clip key (e.g. "api-schema", "diff-preview")
    pub key: String,
    /// Full text, code, or JSON payload to store
    pub content: String,
    /// Author identifier (e.g. quark ID or "human")
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipGetArgs {
    /// Clip key to retrieve
    pub key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipListArgs {}

#[tool_router(router = clipboard_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_clip_put",
        description = "Store or update a named clip in the shared inter-quark artifact clipboard (.hadron/clipboard/clips.json)"
    )]
    pub async fn clip_put(
        &self,
        Parameters(args): Parameters<ClipPutArgs>,
    ) -> Json<ToolResponse> {
        let root = self.root.path().to_path_buf();
        let key = args.key;
        let content = args.content;
        let author = args.author.unwrap_or_else(|| "quark".to_string());

        let res = tokio::task::spawn_blocking(move || put_clip(&root, &key, &content, &author)).await;
        match res {
            Ok(Ok(entry)) => Json(ToolResponse::success(Some(format!(
                "Stored clip `{}` ({} bytes, author: {})",
                entry.key,
                entry.content.len(),
                entry.author
            )))),
            Ok(Err(e)) => Json(ToolResponse::error(format!("Failed to store clip: {e}"))),
            Err(e) => Json(ToolResponse::error(format!("Clipboard put task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_clip_get",
        description = "Retrieve a named clip from the shared inter-quark artifact clipboard (.hadron/clipboard/clips.json)"
    )]
    pub async fn clip_get(
        &self,
        Parameters(args): Parameters<ClipGetArgs>,
    ) -> Json<ToolResponse> {
        let root = self.root.path().to_path_buf();
        let key = args.key;

        let res = tokio::task::spawn_blocking(move || get_clip(&root, &key)).await;
        match res {
            Ok(Ok(Some(entry))) => Json(ToolResponse::success(Some(entry.content))),
            Ok(Ok(None)) => Json(ToolResponse::error("Clip not found")),
            Ok(Err(e)) => Json(ToolResponse::error(format!("Failed to read clip: {e}"))),
            Err(e) => Json(ToolResponse::error(format!("Clipboard get task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_clip_list",
        description = "List all clips stored in the shared inter-quark artifact clipboard"
    )]
    pub async fn clip_list(
        &self,
        Parameters(_args): Parameters<ClipListArgs>,
    ) -> Json<ToolResponse> {
        let root = self.root.path().to_path_buf();

        let res = tokio::task::spawn_blocking(move || list_clips(&root)).await;
        match res {
            Ok(Ok(entries)) => {
                let summary: Vec<serde_json::Value> = entries
                    .into_iter()
                    .map(|e| {
                        serde_json::json!({
                            "key": e.key,
                            "author": e.author,
                            "bytes": e.content.len(),
                            "updated_at": e.updated_at,
                        })
                    })
                    .collect();
                let json =
                    serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "[]".to_string());
                Json(ToolResponse::success(Some(json)))
            }
            Ok(Err(e)) => Json(ToolResponse::error(format!("Failed to list clips: {e}"))),
            Err(e) => Json(ToolResponse::error(format!("Clipboard list task failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn clipboard_mcp_tools_lifecycle() {
        let dir = tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());

        let put_res = server
            .clip_put(Parameters(ClipPutArgs {
                key: "schema-v1".into(),
                content: "{\"version\":1}".into(),
                author: Some("cli-agy".into()),
            }))
            .await;
        assert!(put_res.0.ok);

        let get_res = server
            .clip_get(Parameters(ClipGetArgs {
                key: "schema-v1".into(),
            }))
            .await;
        assert!(get_res.0.ok);
        assert_eq!(get_res.0.blocks.as_deref(), Some("{\"version\":1}"));

        let list_res = server.clip_list(Parameters(ClipListArgs {})).await;
        assert!(list_res.0.ok);
        assert!(list_res.0.blocks.unwrap().contains("schema-v1"));
    }
}
