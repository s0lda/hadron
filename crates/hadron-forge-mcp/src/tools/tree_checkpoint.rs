//! The **tree_checkpoint** family: zero-commit atomic worktree snapshots, diffing, and restoration.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::tree_checkpoint::{self, CheckpointReport, TreeCheckpointAction};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TreeCheckpointArgs {
    pub action: String,
    pub label: Option<String>,
    pub checkpoint_id: Option<String>,
    pub keep: Option<usize>,
}

fn format_checkpoint_report(report: CheckpointReport) -> String {
    let mut out = format!("### Worktree Micro-Checkpoint Report\n\n{}\n\n", report.summary);
    if let Some(ckpt) = report.checkpoint {
        out.push_str(&format!(
            "- **Checkpoint ID:** `{}`\n- **Label:** {}\n- **Files Count:** {}\n- **Total Bytes:** {}\n",
            ckpt.id, ckpt.label, ckpt.files_count, ckpt.total_bytes
        ));
    }
    if !report.list.is_empty() {
        out.push_str("#### Available Checkpoints:\n");
        for c in report.list {
            out.push_str(&format!(
                "- `{}` ({}) — {} files, {} bytes\n",
                c.id, c.label, c.files_count, c.total_bytes
            ));
        }
        out.push('\n');
    }
    if let Some(diff) = report.diff {
        out.push_str("```diff\n");
        out.push_str(&diff);
        out.push_str("\n```\n");
    }
    if !report.restored_files.is_empty() {
        out.push_str("#### Restored Files:\n");
        for f in report.restored_files {
            out.push_str(&format!("- `{}`\n", f));
        }
    }
    out
}

#[tool_router(router = tree_checkpoint_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_tree_checkpoint",
        description = "Fast zero-commit atomic worktree micro-checkpointing for snapshot save, restore, diffing, listing, and pruning"
    )]
    pub async fn tree_checkpoint(
        &self,
        Parameters(args): Parameters<TreeCheckpointArgs>,
    ) -> Json<ToolResponse> {
        let action = match args.action.as_str() {
            "save" => TreeCheckpointAction::Save,
            "restore" => TreeCheckpointAction::Restore,
            "diff" => TreeCheckpointAction::Diff,
            "list" => TreeCheckpointAction::List,
            "prune" => TreeCheckpointAction::Prune,
            other => {
                return Json(ToolResponse::error(format!(
                    "Unknown tree checkpoint action '{}'. Expected: save, restore, diff, list, prune",
                    other
                )))
            }
        };

        match tree_checkpoint::run_tree_checkpoint(
            &self.root,
            action,
            args.label.as_deref(),
            args.checkpoint_id.as_deref(),
            args.keep,
        ) {
            Ok(report) => Json(ToolResponse::success(Some(format_checkpoint_report(report)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tree_checkpoint_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "test content").unwrap();
        let server = ForgeMcpServer::new(dir.path());

        // Save
        let res = server
            .tree_checkpoint(Parameters(TreeCheckpointArgs {
                action: "save".to_string(),
                label: Some("unit test save".to_string()),
                checkpoint_id: None,
                keep: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Saved checkpoint"));

        // List
        let list_res = server
            .tree_checkpoint(Parameters(TreeCheckpointArgs {
                action: "list".to_string(),
                label: None,
                checkpoint_id: None,
                keep: None,
            }))
            .await;
        assert!(list_res.0.ok);
        assert!(list_res.0.blocks.unwrap().contains("unit test save"));
    }
}
