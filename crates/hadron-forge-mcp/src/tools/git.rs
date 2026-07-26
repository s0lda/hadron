//! The **git** family: read-only history and diff queries against the worktree.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::git::{self, CommitEntry};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GitDiffArgs {
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GitLogArgs {
    pub path: Option<String>,
    pub limit: Option<usize>,
}

fn format_log(entries: Vec<CommitEntry>) -> String {
    if entries.is_empty() {
        return "no commits".to_string();
    }
    entries
        .into_iter()
        .map(|c| format!("{} {} {} {}", &c.hash[..c.hash.len().min(12)], c.date, c.author, c.subject))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tool_router(router = git_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_git_diff",
        description = "Diff of uncommitted changes in the worktree, optionally scoped to one path"
    )]
    pub async fn git_diff(&self, Parameters(args): Parameters<GitDiffArgs>) -> Json<ToolResponse> {
        match git::git_diff(&self.root, args.path.as_deref()) {
            Ok(diff) if diff.trim().is_empty() => {
                Json(ToolResponse::success(Some("no uncommitted changes".to_string())))
            }
            Ok(diff) => Json(ToolResponse::success(Some(diff))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_git_log",
        description = "Recent commits touching a path (or the whole tree), newest first: hash, date, author, subject"
    )]
    pub async fn git_log(&self, Parameters(args): Parameters<GitLogArgs>) -> Json<ToolResponse> {
        match git::git_log(&self.root, args.path.as_deref(), args.limit) {
            Ok(entries) => Json(ToolResponse::success(Some(format_log(entries)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn fixture_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-q", "-m", "add a.txt"]);
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        dir
    }

    #[tokio::test]
    async fn git_diff_tool_returns_the_uncommitted_change() {
        let dir = fixture_repo();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .git_diff(Parameters(GitDiffArgs { path: None }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("+two"));
    }

    #[tokio::test]
    async fn git_log_tool_returns_the_commit() {
        let dir = fixture_repo();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .git_log(Parameters(GitLogArgs { path: None, limit: None }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("add a.txt"));
    }

    #[tokio::test]
    async fn git_diff_tool_refuses_a_path_escaping_the_worktree() {
        let dir = fixture_repo();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .git_diff(Parameters(GitDiffArgs {
                path: Some("../../etc/passwd".to_string()),
            }))
            .await;
        assert!(!res.0.ok);
        assert!(res.0.reason.unwrap().contains("outside the worktree"));
    }
}
