//! Ephemeral Scout Quarks (Zero-Footprint Sub-Workers).
//!
//! Runs lightweight, read-only workspace keyword & symbol inspections across repository
//! files without allocating persistent git worktrees or durable roster seats.

use std::fs;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

const MAX_SCOUT_FILE_BYTES: u64 = 512 * 1024;
const MAX_SCOUT_FINDINGS: usize = 25;

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

fn extract_search_tokens(description: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "from", "into", "find", "search", "look", "check", "where",
        "what", "how", "all", "files", "file", "function", "in", "on", "to", "of", "is", "are",
    ];
    let raw: Vec<String> = description
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .map(|s| s.trim_matches('-').to_ascii_lowercase())
        .filter(|s| s.len() >= 2)
        .collect();

    let mut filtered: Vec<String> = Vec::new();
    for tok in &raw {
        if !STOPWORDS.contains(&tok.as_str()) && !filtered.contains(tok) {
            filtered.push(tok.clone());
        }
    }
    if filtered.is_empty() {
        for tok in raw {
            if !filtered.contains(&tok) {
                filtered.push(tok);
            }
        }
    }
    filtered
}

fn should_skip_dir(name: &str, rel_prefix: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | "dist" | "build" | ".next" | "__pycache__" | "venv"
    ) || (rel_prefix == ".hadron" && name == "trees")
        || name == ".hadron"
}

fn scan_repo_sync(repo_root: &Path, tokens: &[String]) -> (usize, Vec<(usize, String)>) {
    let mut files_scanned = 0usize;
    let mut scored_hits: Vec<(usize, String)> = Vec::new();

    fn walk(
        root: &Path,
        dir: &Path,
        rel_prefix: &str,
        tokens: &[String],
        files_scanned: &mut usize,
        scored_hits: &mut Vec<(usize, String)>,
    ) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut sorted: Vec<_> = entries.flatten().collect();
        sorted.sort_by_key(|e| e.file_name());

        for entry in sorted {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = if rel_prefix.is_empty() {
                name.clone()
            } else {
                format!("{rel_prefix}/{name}")
            };
            let path = entry.path();

            if ft.is_dir() {
                if !should_skip_dir(&name, rel_prefix) {
                    walk(root, &path, &rel, tokens, files_scanned, scored_hits);
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.len() > MAX_SCOUT_FILE_BYTES {
                continue;
            }

            let Ok(mut file) = fs::File::open(&path) else {
                continue;
            };
            let mut buf = Vec::with_capacity(meta.len() as usize);
            if file.read_to_end(&mut buf).is_err() {
                continue;
            }
            let probe_len = buf.len().min(4096);
            if buf[..probe_len].contains(&0) {
                continue;
            }
            let Ok(text) = String::from_utf8(buf) else {
                continue;
            };
            *files_scanned += 1;

            if tokens.is_empty() {
                continue;
            }

            let rel_lower = rel.to_ascii_lowercase();
            let path_bonus = tokens.iter().filter(|t| rel_lower.contains(t.as_str())).count();

            let mut file_line_hits = 0usize;
            for (idx, line) in text.lines().enumerate() {
                if file_line_hits >= 3 {
                    break;
                }
                let line_lower = line.to_ascii_lowercase();
                let line_matches = tokens.iter().filter(|t| line_lower.contains(t.as_str())).count();
                if line_matches > 0 {
                    let score = line_matches * 2 + path_bonus;
                    let snippet: String = line.trim().chars().take(120).collect();
                    scored_hits.push((score, format!("{rel}:{}: {snippet}", idx + 1)));
                    file_line_hits += 1;
                }
            }
            if file_line_hits == 0 && path_bonus > 0 {
                scored_hits.push((path_bonus, format!("{rel}:1: (filename match)")));
            }
        }
    }

    walk(
        repo_root,
        repo_root,
        "",
        tokens,
        &mut files_scanned,
        &mut scored_hits,
    );
    scored_hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    (files_scanned, scored_hits)
}

/// Run an ephemeral read-only scout inspection without creating any worktrees or git refs.
pub async fn spawn_ephemeral_scout(
    repo_root: &Path,
    query: &ScoutInvocation,
) -> anyhow::Result<ScoutResult> {
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

    let root = repo_root.to_path_buf();
    let tokens = extract_search_tokens(&query.task_description);
    let task_desc = query.task_description.clone();

    let (files_scanned, scored_hits) =
        tokio::task::spawn_blocking(move || scan_repo_sync(&root, &tokens))
            .await
            .map_err(|e| anyhow::anyhow!("scout worker panicked: {e}"))?;

    let total_hits = scored_hits.len();
    let findings: Vec<String> = scored_hits
        .into_iter()
        .take(MAX_SCOUT_FINDINGS)
        .map(|(_, line)| line)
        .collect();

    Ok(ScoutResult {
        summary: format!(
            "Completed scout query '{}' ({} match(es) across {} scanned file(s))",
            task_desc, total_hits, files_scanned
        ),
        findings,
        created_no_worktree: true,
    })
}
