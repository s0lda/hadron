//! The chamber's read-only view of git.
//!
//! Deliberately NOT `hadron_gluon::snapshot`. That module lives in the engine crate,
//! which links bundled SQLite, the tokio runtime, the file watcher and the CLI process
//! adapters — none of which the UI has any business carrying just to read a diff. The
//! chamber renders the field; it does not drive the swarm, so it must not depend on the
//! crate that does. Shelling out to git is the whole implementation.

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}

/// `git diff HEAD` for the working tree — what the Changes rail shows.
///
/// A repository with no commits yet has no HEAD to diff against, so it has no changes
/// to show rather than an error to report.
pub fn working_diff(repo_root: &Path) -> Option<Vec<FileDiff>> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    Some(parse_diff(&raw))
}

pub fn parse_diff(raw: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut current_file: Option<FileDiff> = None;
    let mut current_hunk: Option<Hunk> = None;

    for line in raw.lines() {
        if line.starts_with("diff --git ") {
            if let Some(mut file) = current_file.take() {
                if let Some(hunk) = current_hunk.take() {
                    file.hunks.push(hunk);
                }
                files.push(file);
            }

            // For cases where there are spaces, we will fallback to parsing +++ and ---,
            // but we can try to extract path from diff --git a/path b/path
            let path = if let Some(b_part) = line.split(" b/").last() {
                b_part.to_string()
            } else {
                String::new()
            };

            current_file = Some(FileDiff {
                path,
                added: 0,
                removed: 0,
                hunks: Vec::new(),
            });
        } else if line.starts_with("@@ ") {
            if let Some(file) = current_file.as_mut() {
                if let Some(hunk) = current_hunk.take() {
                    file.hunks.push(hunk);
                }
                current_hunk = Some(Hunk {
                    header: line.to_string(),
                    lines: Vec::new(),
                });
            }
        } else if line.starts_with("+++ b/") {
            if let Some(file) = current_file.as_mut() {
                file.path = line["+++ b/".len()..].to_string();
            }
        } else if line.starts_with("--- a/") {
            if let Some(file) = current_file.as_mut() {
                if file.path.is_empty() {
                    file.path = line["--- a/".len()..].to_string();
                }
            }
        } else if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("index ")
        {
            // skip other header lines
        } else if let Some(hunk) = current_hunk.as_mut() {
            if let Some(content) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine::Added(content.to_string()));
                if let Some(file) = current_file.as_mut() {
                    file.added += 1;
                }
            } else if let Some(content) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine::Removed(content.to_string()));
                if let Some(file) = current_file.as_mut() {
                    file.removed += 1;
                }
            } else if let Some(content) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine::Context(content.to_string()));
            }
            // we ignore \ No newline at end of file and similar lines
        }
    }

    if let Some(mut file) = current_file.take() {
        if let Some(hunk) = current_hunk.take() {
            file.hunks.push(hunk);
        }
        files.push(file);
    }

    files
}

/// The project root that owns a field path — `<root>/.hadron/field.jsonl` → `<root>`.
/// A field sitting outside a `.hadron/` directory is taken to be in the root already.
pub fn repo_root_of(field_path: &Path) -> &Path {
    let Some(parent) = field_path.parent() else {
        return field_path;
    };
    let root = if parent.file_name() == Some(std::ffi::OsStr::new(".hadron")) {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
}

/// A local branch and whether it has landed in the target branch (`main`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub head: String,
    pub is_current: bool,
    pub merged: bool,
}

/// Parse `git for-each-ref --format='%(refname:short) %(objectname:short)' refs/heads/`
/// output against a precomputed set of names already merged into the target branch
/// (one `git branch --merged` call covers every branch, instead of a
/// `merge-base --is-ancestor` subprocess per branch).
pub fn parse_branches(
    raw: &str,
    current: &str,
    merged: &std::collections::HashSet<String>,
) -> Vec<BranchInfo> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let name = parts.next()?.to_string();
            if name.is_empty() {
                return None;
            }
            let head = parts.next().unwrap_or("").trim().to_string();
            Some(BranchInfo {
                is_current: name == current,
                merged: merged.contains(&name),
                name,
                head,
            })
        })
        .collect()
}

/// Every local branch, with `merged` set against `target` (e.g. `"main"`).
pub fn list_branches(repo_root: &Path, target: &str) -> Vec<BranchInfo> {
    let current = run_git(
        repo_root,
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )
    .trim()
    .to_string();
    let refs = run_git(
        repo_root,
        &["for-each-ref", "--format=%(refname:short) %(objectname:short)", "refs/heads/"],
    );
    let merged_raw = run_git(
        repo_root,
        &["branch", "--merged", target, "--format=%(refname:short)"],
    );
    let merged: std::collections::HashSet<String> =
        merged_raw.lines().map(|s| s.trim().to_string()).collect();
    parse_branches(&refs, &current, &merged)
}

/// One `git worktree list --porcelain` entry — a checkout of this repo living
/// somewhere on disk, e.g. a quark's isolated `.hadron/trees/<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: String,
    pub head: String,
    /// `None` for a detached-HEAD worktree.
    pub branch: Option<String>,
}

/// Parse `git worktree list --porcelain` output — blank-line-separated blocks of
/// `worktree <path>` / `HEAD <sha>` / `branch refs/heads/<name>` (or `detached`).
pub fn parse_worktrees(raw: &str) -> Vec<WorktreeInfo> {
    let mut out = Vec::new();
    let mut current: Option<WorktreeInfo> = None;

    for line in raw.lines() {
        if line.is_empty() {
            out.extend(current.take());
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            out.extend(current.take());
            current = Some(WorktreeInfo { path: p.to_string(), head: String::new(), branch: None });
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            if let Some(entry) = current.as_mut() {
                entry.head = h.chars().take(8).collect();
            }
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if let Some(entry) = current.as_mut() {
                entry.branch = Some(b.to_string());
            }
        }
        // "detached" / "bare" / "locked" lines carry no data we render — skipped.
    }
    out.extend(current.take());
    out
}

/// Every worktree of this repo (the human's checkout plus every quark's).
pub fn list_worktrees(repo_root: &Path) -> Vec<WorktreeInfo> {
    let raw = run_git(repo_root, &["worktree", "list", "--porcelain"]);
    parse_worktrees(&raw)
}

/// A short ASCII commit graph (`git log --graph --oneline --decorate`) — rendered
/// verbatim in a monospace font rather than parsed, since git already draws the
/// graph characters and decorations (branch/tag labels) correctly.
pub fn commit_graph(repo_root: &Path, limit: usize) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args([
            "log",
            "--graph",
            "--oneline",
            "--decorate",
            "--all",
            &format!("-n{limit}"),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run a git subcommand in `repo_root`, returning stdout (empty on any failure —
/// callers treat "nothing" the same as "git couldn't answer", never an error to
/// surface, matching [`get_git_statuses`]'s existing best-effort convention).
fn run_git(repo_root: &Path, args: &[&str]) -> String {
    Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

pub fn get_git_statuses(repo_root: &Path) -> std::collections::HashMap<String, GitStatus> {
    let mut statuses = std::collections::HashMap::new();
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["status", "--porcelain"])
        .output();
    if let Ok(output) = out {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if line.len() < 4 {
                    continue;
                }
                let code = &line[0..2];
                let path_part = &line[3..];
                let path = if code.starts_with('R') {
                    if let Some(pos) = path_part.find(" -> ") {
                        &path_part[pos + 4..]
                    } else {
                        path_part
                    }
                } else {
                    path_part
                };
                let path = path.trim_matches('"').to_string();

                let status = if code.contains('D') {
                    GitStatus::Deleted
                } else if code.contains('A') || code.contains('?') {
                    GitStatus::Added
                } else if code.contains('M') || code.contains('R') || code.contains('T') {
                    GitStatus::Modified
                } else {
                    continue;
                };
                statuses.insert(path, status);
            }
        }
    }
    statuses
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_repo_root_is_the_parent_of_the_hadron_dir() {
        let field = PathBuf::from("/home/jake/dev/hadron/.hadron/field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("/home/jake/dev/hadron"));
    }

    #[test]
    fn a_field_outside_a_hadron_dir_is_already_in_the_root() {
        let field = PathBuf::from("/tmp/scratch/field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("/tmp/scratch"));
    }

    #[test]
    fn parse_branches_flags_current_and_merged() {
        let raw = "\
main abc1234
quark/acp-claude-2/01K feed00d
quark/acp-agy/01K dead000";
        let merged: std::collections::HashSet<String> =
            ["main".to_string(), "quark/acp-agy/01K".to_string()].into_iter().collect();
        let branches = parse_branches(raw, "quark/acp-claude-2/01K", &merged);

        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0], BranchInfo {
            name: "main".into(), head: "abc1234".into(), is_current: false, merged: true,
        });
        assert_eq!(branches[1], BranchInfo {
            name: "quark/acp-claude-2/01K".into(), head: "feed00d".into(), is_current: true, merged: false,
        });
        assert_eq!(branches[2], BranchInfo {
            name: "quark/acp-agy/01K".into(), head: "dead000".into(), is_current: false, merged: true,
        });
    }

    #[test]
    fn parse_worktrees_splits_blank_line_separated_blocks() {
        let raw = "\
worktree /home/jake/dev/hadron
HEAD f33de6e1234567890
branch refs/heads/main

worktree /home/jake/dev/hadron/.hadron/trees/acp-claude-2
HEAD abcdef0123456789
branch refs/heads/quark/acp-claude-2/01K

worktree /home/jake/dev/hadron/.hadron/trees/detached-scratch
HEAD 0011223344556677
detached
";
        let worktrees = parse_worktrees(raw);
        assert_eq!(worktrees.len(), 3);
        assert_eq!(worktrees[0], WorktreeInfo {
            path: "/home/jake/dev/hadron".into(), head: "f33de6e1".into(), branch: Some("main".into()),
        });
        assert_eq!(worktrees[1], WorktreeInfo {
            path: "/home/jake/dev/hadron/.hadron/trees/acp-claude-2".into(),
            head: "abcdef01".into(),
            branch: Some("quark/acp-claude-2/01K".into()),
        });
        assert_eq!(worktrees[2], WorktreeInfo {
            path: "/home/jake/dev/hadron/.hadron/trees/detached-scratch".into(),
            head: "00112233".into(),
            branch: None,
        });
    }

    #[test]
    fn parse_worktrees_of_empty_input_is_empty() {
        assert_eq!(parse_worktrees(""), Vec::new());
    }

    #[test]
    fn test_parse_diff() {
        let raw = "\
diff --git a/crates/ui/src/table/table.rs b/crates/ui/src/table/table.rs
index a1b2c3d..e4f5g6h 100644
--- a/crates/ui/src/table/table.rs
+++ b/crates/ui/src/table/table.rs
@@ -10,3 +10,4 @@
 fn foo() {
-    println!(\"old\");
+    println!(\"new\");
+    println!(\"added\");
 }";
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        let file = &files[0];
        assert_eq!(file.path, "crates/ui/src/table/table.rs");
        assert_eq!(file.added, 2);
        assert_eq!(file.removed, 1);
        assert_eq!(file.hunks.len(), 1);

        let hunk = &file.hunks[0];
        assert_eq!(hunk.header, "@@ -10,3 +10,4 @@");
        assert_eq!(hunk.lines.len(), 5);
        assert_eq!(hunk.lines[0], DiffLine::Context("fn foo() {".to_string()));
        assert_eq!(
            hunk.lines[1],
            DiffLine::Removed("    println!(\"old\");".to_string())
        );
        assert_eq!(
            hunk.lines[2],
            DiffLine::Added("    println!(\"new\");".to_string())
        );
        assert_eq!(
            hunk.lines[3],
            DiffLine::Added("    println!(\"added\");".to_string())
        );
    }
}
