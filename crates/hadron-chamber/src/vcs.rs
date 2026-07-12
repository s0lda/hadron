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
        } else if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("index ") {
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
        assert_eq!(hunk.lines[1], DiffLine::Removed("    println!(\"old\");".to_string()));
        assert_eq!(hunk.lines[2], DiffLine::Added("    println!(\"new\");".to_string()));
        assert_eq!(hunk.lines[3], DiffLine::Added("    println!(\"added\");".to_string()));
    }
}
