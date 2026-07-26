use std::path::{Path, PathBuf};
use crate::file::{resolve_jailed_path, ForgeError, Root};

/// Derive the main repository's `.hadron/nucleus` directory as a [`Root`].
///
/// Finds the main repository root using `current_exe` and `git --git-common-dir`,
/// ensuringLinked worktree checkouts find the single shared nucleus root.
pub fn derive_nucleus_root() -> Result<Root, ForgeError> {
    let exe = std::env::current_exe().map_err(|e| ForgeError::Io(e.to_string()))?;
    let near = exe.parent().unwrap_or(&exe);
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(near)
        .args(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .map_err(|e| ForgeError::Io(e.to_string()))?;

    if !output.status.success() {
        return Err(ForgeError::Io(format!(
            "failed to find main git repo root from {}: {}",
            near.display(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let git_common = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let repo_root = git_common
        .parent()
        .ok_or_else(|| ForgeError::Io(format!("git common dir has no parent: {}", git_common.display())))?;

    let nucleus_dir = repo_root.join(".hadron").join("nucleus");
    Ok(Root::new(nucleus_dir))
}

/// Query the nucleus index and notes for a keyword or phrase.
///
/// If `subpath` is `Some(path)`, queries only that specific relative path within `nucleus_root`.
/// If `subpath` is `None`, queries `index.md` and all `.md` files under `notes/`.
pub fn query_nucleus(
    nucleus_root: &Root,
    query: &str,
    subpath: Option<&str>,
) -> Result<String, ForgeError> {
    if let Some(target) = subpath {
        let full_path = resolve_jailed_path(nucleus_root, target)?;
        return search_single_file(nucleus_root, &full_path, query);
    }

    let mut matches = Vec::new();

    // 1. Search index.md
    if let Ok(index_path) = resolve_jailed_path(nucleus_root, "index.md") {
        if index_path.exists() {
            if let Ok(res) = search_single_file(nucleus_root, &index_path, query) {
                if !res.is_empty() {
                    matches.push(res);
                }
            }
        }
    }

    // 2. Search notes/*.md
    if let Ok(notes_dir) = resolve_jailed_path(nucleus_root, "notes") {
        if notes_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&notes_dir) {
                let mut note_paths: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().map_or(false, |ext| ext == "md"))
                    .collect();
                note_paths.sort();

                for note_path in note_paths {
                    let rel_name = note_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let rel_str = format!("notes/{rel_name}");
                    if let Ok(jailed_path) = resolve_jailed_path(nucleus_root, &rel_str) {
                        if let Ok(res) = search_single_file(nucleus_root, &jailed_path, query) {
                            if !res.is_empty() {
                                matches.push(res);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(matches.join("\n"))
}

fn search_single_file(
    nucleus_root: &Root,
    file_path: &Path,
    query: &str,
) -> Result<String, ForgeError> {
    let content = std::fs::read_to_string(file_path).map_err(|e| ForgeError::Io(e.to_string()))?;
    let query_lower = query.to_lowercase();
    let rel_display = file_path
        .strip_prefix(nucleus_root.path())
        .unwrap_or(file_path)
        .to_string_lossy();

    let mut line_matches = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if line.to_lowercase().contains(&query_lower) {
            line_matches.push(format!("{}:{}: {}", rel_display, idx + 1, line));
        }
    }

    Ok(line_matches.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nucleus_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        let res = query_nucleus(&root, "compiled", Some("../outside.md"));
        assert!(
            matches!(res, Err(ForgeError::OutsideRoot)),
            "expected OutsideRoot, got {res:?}"
        );
    }

    #[test]
    fn queries_index_and_notes_by_keyword() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        std::fs::write(
            dir.path().join("index.md"),
            "- [test-slug](notes/test-slug.md) — A test lesson\n",
        )
        .unwrap();

        std::fs::create_dir_all(dir.path().join("notes")).unwrap();
        std::fs::write(
            dir.path().join("notes").join("test-slug.md"),
            "---\nname: test-slug\n---\nDetail about compiled tests.\n",
        )
        .unwrap();

        let res = query_nucleus(&root, "compiled", None).unwrap();
        assert!(res.contains("notes/test-slug.md:4: Detail about compiled tests."));

        let res_index = query_nucleus(&root, "test lesson", Some("index.md")).unwrap();
        assert!(res_index.contains("index.md:1: - [test-slug]"));
    }

    #[test]
    fn derive_nucleus_root_finds_real_nucleus() {
        let root = derive_nucleus_root().unwrap();
        assert!(root.path().to_string_lossy().ends_with(".hadron/nucleus"));
    }
}
