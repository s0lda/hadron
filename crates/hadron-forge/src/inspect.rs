//! The **inspect** family's pure core: read a slice of a file, list a
//! directory, search text recursively. No shell, no native tools — every path
//! goes through [`resolve_jailed_path`], the same jail `edit` already uses.

use std::path::Path;

use crate::file::{resolve_jailed_path, ForgeError, Root};

const DEFAULT_READ_LIMIT: usize = 2000;
const GREP_SKIP_DIRS: &[&str] = &["target", ".git", "node_modules"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSlice {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
}

/// `offset`/`limit` are 1-indexed line numbers, matching the host's own Read
/// tool convention. Missing `offset` starts at line 1; missing `limit` caps at
/// [`DEFAULT_READ_LIMIT`] so a quark cannot accidentally pull a huge file whole.
pub fn read_file(
    root: &Root,
    rel_path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<FileSlice, ForgeError> {
    let full_path = resolve_jailed_path(root, rel_path)?;
    let content = std::fs::read_to_string(&full_path).map_err(|_| ForgeError::NotFound)?;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start_idx = offset.unwrap_or(1).max(1) - 1;
    let start_idx = start_idx.min(total_lines);
    let take = limit.unwrap_or(DEFAULT_READ_LIMIT).max(1);
    let end_idx = (start_idx + take).min(total_lines);
    let text = lines[start_idx..end_idx].join("\n");
    Ok(FileSlice {
        text,
        start_line: start_idx + 1,
        end_line: end_idx,
        total_lines,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntryInfo {
    pub name: String,
    pub is_dir: bool,
}

pub fn list_dir(root: &Root, rel_path: &str) -> Result<Vec<DirEntryInfo>, ForgeError> {
    let full_path = resolve_jailed_path(root, rel_path)?;
    let mut entries: Vec<DirEntryInfo> = std::fs::read_dir(&full_path)
        .map_err(|_| ForgeError::NotFound)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            Some(DirEntryInfo {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: file_type.is_dir(),
            })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
}

/// Literal substring search, recursively from `rel_path` (root if `None`).
/// Symlinked entries are skipped outright during the walk — the jail already
/// resolves symlinks for the *starting* path, but a directory discovered
/// mid-walk must never be followed, or the walk escapes root one hop at a time.
pub fn grep(root: &Root, pattern: &str, rel_path: Option<&str>) -> Result<Vec<GrepMatch>, ForgeError> {
    let start = resolve_jailed_path(root, rel_path.unwrap_or(""))?;
    let canonical_root = root.path().canonicalize().map_err(|e| ForgeError::Io(e.to_string()))?;
    let mut matches = Vec::new();
    grep_walk(&start, &canonical_root, pattern, &mut matches)?;
    Ok(matches)
}

fn grep_walk(
    dir: &Path,
    canonical_root: &Path,
    pattern: &str,
    matches: &mut Vec<GrepMatch>,
) -> Result<(), ForgeError> {
    if dir.is_file() {
        grep_file(dir, canonical_root, pattern, matches);
        return Ok(());
    }
    let read_dir = std::fs::read_dir(dir).map_err(|_| ForgeError::NotFound)?;
    for entry in read_dir.filter_map(|e| e.ok()) {
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if GREP_SKIP_DIRS.contains(&name) {
                    continue;
                }
            }
            grep_walk(&path, canonical_root, pattern, matches)?;
        } else if file_type.is_file() {
            grep_file(&path, canonical_root, pattern, matches);
        }
    }
    Ok(())
}

fn grep_file(path: &Path, canonical_root: &Path, pattern: &str, matches: &mut Vec<GrepMatch>) {
    let Ok(content) = std::fs::read_to_string(path) else { return };
    let rel = path
        .strip_prefix(canonical_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    for (i, line) in content.lines().enumerate() {
        if line.contains(pattern) {
            matches.push(GrepMatch {
                path: rel.clone(),
                line: i + 1,
                text: line.to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_returns_the_requested_slice() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        std::fs::write(dir.path().join("f.txt"), "l1\nl2\nl3\nl4\nl5\n").unwrap();

        let slice = read_file(&root, "f.txt", Some(2), Some(2)).unwrap();
        assert_eq!(slice.text, "l2\nl3");
        assert_eq!(slice.start_line, 2);
        assert_eq!(slice.end_line, 3);
        assert_eq!(slice.total_lines, 5);
    }

    #[test]
    fn read_file_rejects_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        assert!(matches!(
            read_file(&root, "../evil.txt", None, None),
            Err(ForgeError::OutsideRoot)
        ));
    }

    #[test]
    fn list_dir_lists_entries_sorted_with_dir_flag() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        std::fs::write(dir.path().join("a.txt"), "hi").unwrap();
        std::fs::create_dir(dir.path().join("b")).unwrap();

        let entries = list_dir(&root, "").unwrap();
        assert_eq!(
            entries,
            vec![
                DirEntryInfo { name: "a.txt".into(), is_dir: false },
                DirEntryInfo { name: "b".into(), is_dir: true },
            ]
        );
    }

    #[test]
    fn list_dir_rejects_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        assert!(matches!(list_dir(&root, ".."), Err(ForgeError::OutsideRoot)));
    }

    #[test]
    fn grep_finds_matches_recursively_and_skips_target_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "fn needle() {}\n").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/generated.rs"), "fn needle() {}\n").unwrap();

        let matches = grep(&root, "needle", None).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "src/lib.rs");
        assert_eq!(matches[0].line, 1);
    }

    #[test]
    fn grep_rejects_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        assert!(matches!(
            grep(&root, "needle", Some("../")),
            Err(ForgeError::OutsideRoot)
        ));
    }
}
