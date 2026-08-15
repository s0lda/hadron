//! Cross-worktree peer inspector MCP tool.

use super::{ForgeMcpServer, ToolResponse};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PeerInspectArgs {
    /// Specific peer id to inspect (e.g. "acp-claude" or "cli-agy"). If omitted, lists all sibling worktrees.
    pub peer_id: Option<String>,
}

#[tool_router(router = peers_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_peer_inspect",
        description = "Inspect sibling quark worktrees across .hadron/trees/* (branch, latest commit, dirty files, commits ahead)"
    )]
    pub async fn peer_inspect(&self, Parameters(args): Parameters<PeerInspectArgs>) -> Json<ToolResponse> {
        let root = self.root.clone();
        let peer_id = args.peer_id;

        let res = tokio::task::spawn_blocking(move || {
            if let Some(id) = peer_id {
                hadron_forge::peers::inspect_peer_worktree(&root, &id)
                    .map(|info| serde_json::to_string_pretty(&info).unwrap_or_default())
            } else {
                hadron_forge::peers::list_peer_worktrees(&root)
                    .map(|list| serde_json::to_string_pretty(&list).unwrap_or_default())
            }
        })
        .await;

        match res {
            Ok(Ok(json)) => Json(ToolResponse::success(Some(json))),
            Ok(Err(e)) => Json(ToolResponse::error(e.to_string())),
            Err(e) => Json(ToolResponse::error(format!("Peer inspection task failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn fixture_multitree_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main-repo");
        std::fs::create_dir_all(&main).unwrap();

        let run = |args: &[&str], cwd: &std::path::Path| {
            let status = Command::new("git")
                .args(args)
                .current_dir(cwd)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed in {}", cwd.display());
        };

        run(&["init", "-q"], &main);
        run(&["config", "user.email", "test@test.com"], &main);
        run(&["config", "user.name", "Tester"], &main);
        std::fs::write(main.join("file.txt"), "hello\n").unwrap();
        run(&["add", "file.txt"], &main);
        run(&["commit", "-q", "-m", "initial commit"], &main);

        let trees = main.join(".hadron").join("trees");
        std::fs::create_dir_all(&trees).unwrap();

        let peer_a = trees.join("peer-alpha");
        run(&["worktree", "add", "-q", "-b", "quark/peer-alpha/feat1", peer_a.to_str().unwrap()], &main);

        (tmp, main, peer_a)
    }

    #[tokio::test]
    async fn peer_inspect_tool_executes() {
        let (_tmp, _main, peer_a) = fixture_multitree_repo();
        let server = ForgeMcpServer::new(&peer_a);

        let res = server
            .peer_inspect(Parameters(PeerInspectArgs { peer_id: None }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("peer-alpha"));
    }
}
