use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::file::{resolve_jailed_path, ForgeError, Root};

/// Payload to author and register a new post-mortem note and 1-line index pointer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistillLessonInput {
    pub slug: String,
    pub description: String,
    pub fact: String,
    pub why: String,
    pub how_to_apply: String,
    pub section: Option<String>,
}

/// Result report from distilling a lesson into nucleus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistillLessonOutput {
    pub slug: String,
    pub note_path: String,
    pub index_line: String,
    pub index_appended: bool,
    pub summary: String,
}

/// Programmatically create a post-mortem note in `notes/<slug>.md` and register a 1-line pointer in `index.md`.
pub fn distill_lesson(
    nucleus_root: &Root,
    input: &DistillLessonInput,
) -> Result<DistillLessonOutput, ForgeError> {
    let clean_slug = input.slug.trim().to_lowercase().replace(' ', "-");
    if clean_slug.is_empty() || clean_slug.contains('/') || clean_slug.contains('\\') || clean_slug.contains("..") {
        return Err(ForgeError::Rejected(format!("invalid slug: {:?}", input.slug)));
    }

    let notes_dir = resolve_jailed_path(nucleus_root, "notes")?;
    if !notes_dir.exists() {
        std::fs::create_dir_all(&notes_dir).map_err(|e| ForgeError::Io(e.to_string()))?;
    }

    let note_rel = format!("notes/{clean_slug}.md");
    let note_file = resolve_jailed_path(nucleus_root, &note_rel)?;

    // 1. Format note content with Standard Model YAML frontmatter
    let note_content = format!(
        "---\nname: {clean_slug}\ndescription: {}\nmetadata:\n  type: project\n---\n{}\n\n**Why:** {}\n\n**How to apply:** {}\n",
        input.description.trim(),
        input.fact.trim(),
        input.why.trim(),
        input.how_to_apply.trim()
    );

    std::fs::write(&note_file, note_content).map_err(|e| ForgeError::Io(e.to_string()))?;

    // 2. Format 1-line index pointer (hook capped at ~100 characters)
    let raw_hook = input.description.trim();
    let hook = if raw_hook.chars().count() > 100 {
        format!("{}…", raw_hook.chars().take(99).collect::<String>())
    } else {
        raw_hook.to_string()
    };

    let index_line = format!("- [{clean_slug}](notes/{clean_slug}.md) — {hook}");

    let index_path = resolve_jailed_path(nucleus_root, "index.md")?;
    let mut index_appended = false;

    if index_path.exists() {
        let index_content = std::fs::read_to_string(&index_path).unwrap_or_default();
        let target_needle = format!("[{clean_slug}]");

        if index_content.contains(&target_needle) {
            // Replace existing line with updated hook
            let mut updated_lines = Vec::new();
            for line in index_content.lines() {
                if line.contains(&target_needle) {
                    updated_lines.push(index_line.clone());
                } else {
                    updated_lines.push(line.to_string());
                }
            }
            std::fs::write(&index_path, updated_lines.join("\n") + "\n").map_err(|e| ForgeError::Io(e.to_string()))?;
        } else {
            // Append under requested section if found, else append to bottom
            let target_section = input.section.as_deref().unwrap_or("Swarm Lessons");
            let section_header = format!("## {target_section}");

            let mut lines: Vec<String> = index_content.lines().map(ToString::to_string).collect();
            let mut inserted = false;

            if let Some(pos) = lines.iter().position(|l| l.trim() == section_header) {
                // Insert right after section header
                lines.insert(pos + 1, index_line.clone());
                inserted = true;
            }

            if !inserted {
                if !lines.iter().any(|l| l.starts_with("## ")) {
                    lines.push(format!("\n## {target_section}"));
                }
                lines.push(index_line.clone());
            }

            std::fs::write(&index_path, lines.join("\n") + "\n").map_err(|e| ForgeError::Io(e.to_string()))?;
            index_appended = true;
        }
    } else {
        let content = format!("# Memory index\n\n## Swarm Lessons\n{}\n", index_line);
        std::fs::write(&index_path, content).map_err(|e| ForgeError::Io(e.to_string()))?;
        index_appended = true;
    }

    let summary = format!("Distilled nucleus lesson '{clean_slug}' to notes/{clean_slug}.md and registered in index.md");

    Ok(DistillLessonOutput {
        slug: clean_slug,
        note_path: note_rel,
        index_line,
        index_appended,
        summary,
    })
}


/// Derive a project's `.hadron/nucleus` directory as a [`Root`].
///
/// `project` is the directory forge-mcp was given as `argv[1]` — the cwd of the turn.
/// The `--git-common-dir` walk is what makes a quark's LINKED worktree
/// (`.hadron/trees/<id>`) resolve to the main checkout's single shared nucleus rather
/// than growing a private empty one.
///
/// This used to resolve from `current_exe`, which answered about the repository the
/// BINARY was built in. That is not a hypothetical: in-repo the git call succeeds and
/// silently returns a different project's nucleus.
pub fn derive_nucleus_root(project: &Path) -> Result<Root, ForgeError> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(project)
        .args(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .map_err(|e| ForgeError::Io(e.to_string()))?;

    if !output.status.success() {
        return Err(ForgeError::Io(format!(
            "failed to find main git repo root from {}: {}",
            project.display(),
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

    /// `derive_nucleus_root` used to resolve from `current_exe`, so forge-mcp answered
    /// about the repository its own binary was built in, not the project the quark is
    /// working on. In-repo the git call SUCCEEDS and returns the wrong root, silently.
    /// The nucleus must come from the project path forge-mcp is given as argv[1].
    #[test]
    fn the_nucleus_root_follows_the_project_not_the_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("some-users-project");
        std::fs::create_dir_all(&project).unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&project)
            .status()
            .unwrap()
            .success());

        let root = derive_nucleus_root(&project).expect("a git checkout resolves");
        assert_eq!(
            root.path(),
            project.join(".hadron").join("nucleus"),
            "the nucleus must be the project's own, whatever repo the binary came from"
        );
    }

    /// The `--git-common-dir` walk is load-bearing, not incidental: a quark's turn runs
    /// in a LINKED worktree (`.hadron/trees/<id>`), and this is what makes every quark
    /// share one nucleus instead of silently growing a private empty one per worktree.
    #[test]
    fn a_linked_worktree_resolves_to_the_main_checkouts_nucleus() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main-checkout");
        std::fs::create_dir_all(&main).unwrap();
        let git = |args: &[&str], cwd: &std::path::Path| {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(cwd)
                .status()
                .unwrap()
                .success(), "git {args:?} failed");
        };
        git(&["init", "-q"], &main);
        git(&["config", "user.email", "t@t"], &main);
        git(&["config", "user.name", "t"], &main);
        std::fs::write(main.join("f"), "x").unwrap();
        git(&["add", "f"], &main);
        git(&["commit", "-qm", "one"], &main);
        let tree = tmp.path().join("tree");
        git(&["worktree", "add", "-q", tree.to_str().unwrap()], &main);

        let root = derive_nucleus_root(&tree).expect("a linked worktree resolves");
        assert_eq!(
            root.path(),
            main.join(".hadron").join("nucleus"),
            "a linked worktree must share the MAIN checkout's nucleus"
        );
    }

    #[test]
    fn distills_note_and_appends_single_line_to_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        std::fs::write(dir.path().join("index.md"), "# Memory index\n\n## Swarm Lessons\n").unwrap();

        let input = DistillLessonInput {
            slug: "test-distill-lesson".to_string(),
            description: "Test description retrieval key".to_string(),
            fact: "A single distilled invariant or lesson fact.".to_string(),
            why: "Preventing silent regressions across worktrees.".to_string(),
            how_to_apply: "Check the registry before adding new items.".to_string(),
            section: Some("Swarm Lessons".to_string()),
        };

        let output = distill_lesson(&root, &input).expect("distillation should succeed");
        assert_eq!(output.slug, "test-distill-lesson");
        assert!(output.index_appended);

        // Verify note file exists with frontmatter
        let note_content = std::fs::read_to_string(dir.path().join("notes").join("test-distill-lesson.md")).unwrap();
        assert!(note_content.contains("name: test-distill-lesson"));
        assert!(note_content.contains("description: Test description retrieval key"));
        assert!(note_content.contains("**Why:** Preventing silent regressions"));

        // Verify index.md has exactly one line pointer
        let index_content = std::fs::read_to_string(dir.path().join("index.md")).unwrap();
        assert!(index_content.contains("- [test-distill-lesson](notes/test-distill-lesson.md) — Test description retrieval key"));
    }
}

