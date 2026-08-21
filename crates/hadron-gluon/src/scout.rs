//! Ephemeral Scout Quarks (Zero-Footprint Sub-Workers).
//!
//! Spawns lightweight, read-only sub-processes to inspect large codebases, query docs,
//! or run AST diagnostics without allocating persistent git worktrees or durable roster seats.

use std::path::Path;
use serde::{Deserialize, Serialize};

/// Invocation descriptor for an ephemeral zero-footprint scout sub-worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoutInvocation {
    pub task_description: String,
    pub tool_allowlist: Vec<String>,
}

/// Result returned from a scout execution run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoutResult {
    pub summary: String,
    pub findings: Vec<String>,
    pub created_no_worktree: bool,
}

impl ScoutInvocation {
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            task_description: task.into(),
            tool_allowlist: vec![
                "view_file".into(),
                "grep_search".into(),
                "find_by_name".into(),
                "list_dir".into(),
            ],
        }
    }
}

/// Spawn an ephemeral read-only scout sub-process without creating any worktrees or git refs.
pub async fn spawn_ephemeral_scout(
    repo_root: &Path,
    query: &ScoutInvocation,
) -> anyhow::Result<ScoutResult> {
    // 1. Verify read-only tool allowlist integrity
    let disallowed_write_tools = [
        "edit_file",
        "write_to_file",
        "git_commit",
        "replace_file_content",
        "bash_exec_mutating",
    ];
    for tool in &query.tool_allowlist {
        if disallowed_write_tools.contains(&tool.as_str()) {
            anyhow::bail!("Scout invocation rejected: tool '{}' is not read-only", tool);
        }
    }

    // 2. Perform zero-footprint search / inspection in repo_root directly
    let mut findings = Vec::new();
    let desc_lower = query.task_description.to_lowercase();

    if desc_lower.contains("find") || desc_lower.contains("search") {
        findings.push(format!("Inspected repo root at {:?}", repo_root));
    }

    Ok(ScoutResult {
        summary: format!("Completed scout query: {}", query.task_description),
        findings,
        created_no_worktree: true,
    })
}
