//! AST-aware rebase conflict healer.
//!
//! Uses semantic AST merging (`hadron_forge::ast_merge`) to automatically resolve
//! non-overlapping Rust AST changes during a git rebase conflict before falling back
//! to an abort.

use std::path::Path;
use hadron_forge::ast_merge::{merge_rust_ast, AstMergeResult};
use crate::snapshot::git;

/// Attempts to resolve conflicting Rust source contents using AST block merge.
pub fn heal_conflicting_rust_content(base: &str, ours: &str, theirs: &str) -> Option<String> {
    match merge_rust_ast(base, ours, theirs) {
        AstMergeResult::Clean(merged) => Some(merged),
        AstMergeResult::Conflict(_) => None,
    }
}

/// Attempts to heal rebase conflicts in `worktree_path` if all conflicts are in `.rs` files
/// and can be cleanly reconciled via AST merging.
pub fn heal_rebase_conflicts(worktree_path: &Path) -> Result<bool, String> {
    // 1. Get list of unmerged files
    let unmerged_output = git(worktree_path, &["diff", "--name-only", "--diff-filter=U"])
        .map_err(|e| format!("Failed to inspect unmerged files: {e:#}"))?;
    
    let unmerged_files: Vec<&str> = unmerged_output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if unmerged_files.is_empty() {
        return Ok(false);
    }

    // Only heal if EVERY unmerged file is a Rust source file
    if unmerged_files.iter().any(|f| !f.ends_with(".rs")) {
        return Ok(false);
    }

    // 2. For each unmerged file, extract stage 1 (base), stage 2 (ours/upstream), stage 3 (theirs/patch)
    for rel_path in &unmerged_files {
        let base_spec = format!(":1:{rel_path}");
        let ours_spec = format!(":2:{rel_path}");
        let theirs_spec = format!(":3:{rel_path}");

        let base_content = git(worktree_path, &["show", &base_spec])
            .unwrap_or_default();
        let ours_content = git(worktree_path, &["show", &ours_spec])
            .unwrap_or_default();
        let theirs_content = git(worktree_path, &["show", &theirs_spec])
            .unwrap_or_default();

        match heal_conflicting_rust_content(&base_content, &ours_content, &theirs_content) {
            Some(resolved) => {
                let full_path = worktree_path.join(rel_path);
                if let Err(e) = std::fs::write(&full_path, resolved) {
                    return Err(format!("Failed to write healed AST merge to {rel_path}: {e}"));
                }
                if let Err(e) = git(worktree_path, &["add", rel_path]) {
                    return Err(format!("Failed to git add healed file {rel_path}: {e:#}"));
                }
            }
            None => {
                // Semantic AST conflict detected — cannot heal automatically
                return Ok(false);
            }
        }
    }

    // 3. Continue the rebase. If multiple commits conflict, this will need git rebase --continue
    // Set GIT_EDITOR=true to avoid launching interactive editor for rebase message
    let mut cmd = std::process::Command::new("git");
    cmd.args(["rebase", "--continue"])
        .current_dir(worktree_path)
        .env("GIT_EDITOR", "true");

    let status = cmd.status().map_err(|e| format!("Failed to spawn git rebase --continue: {e}"))?;
    if status.success() {
        Ok(true)
    } else {
        // If git rebase --continue still failed or hit a second conflict, report false
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heal_conflicting_rust_content_disjoint_fns() {
        let base = r#"
pub fn original() -> bool {
    true
}
"#;
        let ours = r#"
pub fn original() -> bool {
    true
}

pub fn feature_a() -> usize {
    42
}
"#;
        let theirs = r#"
pub fn original() -> bool {
    true
}

pub fn feature_b() -> &'static str {
    "hello"
}
"#;
        let healed = heal_conflicting_rust_content(base, ours, theirs);
        assert!(healed.is_some());
        let code = healed.unwrap();
        assert!(code.contains("pub fn feature_a"));
        assert!(code.contains("pub fn feature_b"));
    }

    #[test]
    fn test_heal_conflicting_rust_content_same_fn_conflict() {
        let base = r#"
pub fn compute() -> i32 {
    0
}
"#;
        let ours = r#"
pub fn compute() -> i32 {
    1
}
"#;
        let theirs = r#"
pub fn compute() -> i32 {
    2
}
"#;
        let healed = heal_conflicting_rust_content(base, ours, theirs);
        // Both modified compute() differently — should not heal automatically
        assert!(healed.is_none());
    }
}
