//! One-click message and lesson promotion into nucleus memory (Capability #20).
//!
//! Enforces Standard Model Rule 9 invariants:
//! - Strictly pointer lines in `index.md` (`- [<slug>](notes/<slug>.md) — <hook>`)
//! - Strict YAML frontmatter with `name`, `description`, `metadata.type`
//! - Fact body stored in `.hadron/nucleus/notes/<slug>.md`

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionRequest {
    pub slug: String,
    pub description: String,
    pub fact: String,
    pub note_type: Option<String>,
}

/// Promotes a message, lesson, or gate failure to a new nucleus note in `.hadron/nucleus/notes/<slug>.md`
/// and appends the routing pointer to `.hadron/nucleus/index.md`.
pub fn promote_to_note(
    repo_root: &Path,
    req: &PromotionRequest,
) -> io::Result<PathBuf> {
    let nucleus_dir = repo_root.join(".hadron").join("nucleus");
    let notes_dir = nucleus_dir.join("notes");
    fs::create_dir_all(&notes_dir)?;

    let clean_slug = req
        .slug
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>();

    let note_path = notes_dir.join(format!("{}.md", clean_slug));
    let note_type = req.note_type.as_deref().unwrap_or("project");

    let note_content = format!(
        "---\nname: {}\ndescription: {}\nmetadata:\n  type: {}\n---\n\n{}\n",
        clean_slug,
        req.description.trim(),
        note_type,
        req.fact.trim()
    );

    fs::write(&note_path, note_content)?;

    // Update index.md with pointer line if not already present
    let index_path = nucleus_dir.join("index.md");
    let hook = if req.description.len() > 95 {
        format!("{}…", &req.description[..95])
    } else {
        req.description.clone()
    };
    let pointer_line = format!("- [{slug}](notes/{slug}.md) — {hook}\n", slug = clean_slug);

    let existing_index = fs::read_to_string(&index_path).unwrap_or_default();
    if !existing_index.contains(&format!("[{}]", clean_slug)) {
        let mut new_index = existing_index;
        if !new_index.ends_with('\n') && !new_index.is_empty() {
            new_index.push('\n');
        }
        new_index.push_str(&pointer_line);
        fs::write(&index_path, new_index)?;
    }

    Ok(note_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_promote_to_note() {
        let temp = tempdir().unwrap();
        let req = PromotionRequest {
            slug: "test-cache-rule".to_string(),
            description: "A rule about caching compiled rlibs".to_string(),
            fact: "Always touch lib.rs when compiling across worktrees.\n\n**Why:** Target dirs share rlibs.".to_string(),
            note_type: Some("project".to_string()),
        };

        let note_path = promote_to_note(temp.path(), &req).unwrap();
        assert!(note_path.exists());

        let note_content = fs::read_to_string(&note_path).unwrap();
        assert!(note_content.contains("name: test-cache-rule"));
        assert!(note_content.contains("description: A rule about caching compiled rlibs"));
        assert!(note_content.contains("type: project"));
        assert!(note_content.contains("**Why:** Target dirs share rlibs."));

        let index_path = temp.path().join(".hadron").join("nucleus").join("index.md");
        assert!(index_path.exists());
        let index_content = fs::read_to_string(&index_path).unwrap();
        assert!(index_content.contains("- [test-cache-rule](notes/test-cache-rule.md) — A rule about caching compiled rlibs"));
    }
}
