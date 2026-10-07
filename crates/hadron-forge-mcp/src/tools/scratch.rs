//! The **scratch** family: shared cross-worktree scratch bus tools.
//!
//! Allows quarks working in isolated git worktrees to write, read, and list
//! shared ephemeral plans, specs, execution traces, and coordination notes.

use super::{ForgeMcpServer, ToolResponse};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchWriteArgs {
    /// Relative filename or key in the scratch directory (e.g. "plan-draft.md")
    pub name: String,
    /// Content string to write
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchReadArgs {
    /// Relative filename or key to read
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScratchListArgs {}

fn scratch_dir_for_root(root: &Path) -> PathBuf {
    if let Ok(trees_dir) = hadron_forge::peers::derive_trees_dir(root) {
        if let Some(hadron_dir) = trees_dir.parent() {
            return hadron_dir.join("scratch");
        }
    }
    root.join(".hadron").join("scratch")
}

#[tool_router(router = scratch_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_scratch_write",
        description = "Write an ephemeral note, plan draft, or trace to the shared cross-worktree scratch bus (.hadron/scratch/)"
    )]
    pub async fn scratch_write(
        &self,
        Parameters(args): Parameters<ScratchWriteArgs>,
    ) -> Json<ToolResponse> {
        let clean_name = args.name.trim();
        if clean_name.is_empty() || clean_name.contains("..") {
            return Json(ToolResponse::error("Invalid scratch name"));
        }

        let dir = scratch_dir_for_root(self.root.path());
        let file_path = dir.join(clean_name);

        let res = tokio::task::spawn_blocking(move || {
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&file_path, args.content)?;
            Ok::<PathBuf, std::io::Error>(file_path)
        })
        .await;

        match res {
            Ok(Ok(p)) => Json(ToolResponse::success(Some(format!(
                "Saved to shared scratch bus: {}",
                p.display()
            )))),
            Ok(Err(e)) => Json(ToolResponse::error(format!("Failed to write scratch: {e}"))),
            Err(e) => Json(ToolResponse::error(format!("Scratch write task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_scratch_read",
        description = "Read an entry from the shared cross-worktree scratch bus (.hadron/scratch/)"
    )]
    pub async fn scratch_read(
        &self,
        Parameters(args): Parameters<ScratchReadArgs>,
    ) -> Json<ToolResponse> {
        let clean_name = args.name.trim();
        if clean_name.is_empty() || clean_name.contains("..") {
            return Json(ToolResponse::error("Invalid scratch name"));
        }

        let dir = scratch_dir_for_root(self.root.path());
        let file_path = dir.join(clean_name);

        let res = tokio::task::spawn_blocking(move || {
            if !file_path.is_file() {
                return Ok::<Option<String>, std::io::Error>(None);
            }
            let content = fs::read_to_string(&file_path)?;
            Ok(Some(content))
        })
        .await;

        match res {
            Ok(Ok(Some(content))) => Json(ToolResponse::success(Some(content))),
            Ok(Ok(None)) => Json(ToolResponse::error(format!("Scratch entry `{clean_name}` not found"))),
            Ok(Err(e)) => Json(ToolResponse::error(format!("Failed to read scratch: {e}"))),
            Err(e) => Json(ToolResponse::error(format!("Scratch read task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_scratch_list",
        description = "List all active entries in the shared cross-worktree scratch bus"
    )]
    pub async fn scratch_list(
        &self,
        Parameters(_args): Parameters<ScratchListArgs>,
    ) -> Json<ToolResponse> {
        let dir = scratch_dir_for_root(self.root.path());

        let res = tokio::task::spawn_blocking(move || {
            if !dir.is_dir() {
                return Ok::<Vec<String>, std::io::Error>(Vec::new());
            }
            let mut entries = Vec::new();
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                if let Some(name) = entry.file_name().to_str() {
                    entries.push(name.to_string());
                }
            }
            entries.sort();
            Ok(entries)
        })
        .await;

        match res {
            Ok(Ok(entries)) => {
                let json = serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".to_string());
                Json(ToolResponse::success(Some(json)))
            }
            Ok(Err(e)) => Json(ToolResponse::error(format!("Failed to list scratch: {e}"))),
            Err(e) => Json(ToolResponse::error(format!("Scratch list task failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn scratch_bus_tools_lifecycle() {
        let dir = tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());

        // 1. Write
        let w_res = server
            .scratch_write(Parameters(ScratchWriteArgs {
                name: "notes.txt".into(),
                content: "shared notes between quarks".into(),
            }))
            .await;
        assert!(w_res.0.ok);

        // 2. Read
        let r_res = server
            .scratch_read(Parameters(ScratchReadArgs {
                name: "notes.txt".into(),
            }))
            .await;
        assert!(r_res.0.ok);
        assert_eq!(r_res.0.blocks.unwrap(), "shared notes between quarks");

        // 3. List
        let l_res = server
            .scratch_list(Parameters(ScratchListArgs {}))
            .await;
        assert!(l_res.0.ok);
        assert!(l_res.0.blocks.unwrap().contains("notes.txt"));
    }
}
