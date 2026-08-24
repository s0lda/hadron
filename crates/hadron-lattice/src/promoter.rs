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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BugPostmortem {
    pub slug: String,
    pub symptom: String,
    pub root_cause: String,
    pub prevention_invariant: String,
    pub how_to_apply: Option<String>,
    pub target_files: Vec<String>,
    pub regression_test: Option<String>,
}

/// Promotes a structured bug postmortem to `.hadron/nucleus/notes/bug-<slug>.md`
/// and appends the standard routing pointer to `.hadron/nucleus/index.md`.
pub fn promote_bug_postmortem(
    repo_root: &Path,
    postmortem: &BugPostmortem,
) -> io::Result<PathBuf> {
    let nucleus_dir = repo_root.join(".hadron").join("nucleus");
    let notes_dir = nucleus_dir.join("notes");
    fs::create_dir_all(&notes_dir)?;

    let raw_slug = postmortem.slug.trim().to_ascii_lowercase();
    let base_slug = if let Some(stripped) = raw_slug.strip_prefix("bug-") {
        stripped
    } else {
        &raw_slug
    };

    let clean_base = base_slug
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>();
    let clean_slug = format!("bug-{}", clean_base);

    let note_path = notes_dir.join(format!("{}.md", clean_slug));

    let description = format!(
        "{} -> {}",
        postmortem.symptom.trim(),
        postmortem.prevention_invariant.trim()
    );

    let mut frontmatter = format!(
        "---\nname: {}\ndescription: \"{}\"\nmetadata:\n  type: postmortem\n",
        clean_slug,
        description.replace('"', "\\\"")
    );

    if !postmortem.target_files.is_empty() {
        frontmatter.push_str("  target_files:\n");
        for file in &postmortem.target_files {
            frontmatter.push_str(&format!("    - \"{}\"\n", file.trim().replace('"', "\\\"")));
        }
    }

    if let Some(ref reg_test) = postmortem.regression_test {
        frontmatter.push_str(&format!(
            "  regression_test: \"{}\"\n",
            reg_test.trim().replace('"', "\\\"")
        ));
    }

    frontmatter.push_str("---\n\n");

    let mut body = format!(
        "### Symptom\n{}\n\n### Root Cause\n{}\n\n### Prevention Invariant\n{}\n",
        postmortem.symptom.trim(),
        postmortem.root_cause.trim(),
        postmortem.prevention_invariant.trim()
    );

    if let Some(ref how_to) = postmortem.how_to_apply {
        body.push_str(&format!("\n### How to apply:\n{}\n", how_to.trim()));
    }

    let note_content = format!("{}{}", frontmatter, body);
    fs::write(&note_path, note_content)?;

    // Update index.md with pointer line if not already present
    let index_path = nucleus_dir.join("index.md");
    let hook_raw = format!(
        "{} -> {}",
        postmortem.symptom.trim(),
        postmortem.prevention_invariant.trim()
    );
    let hook = if hook_raw.len() > 95 {
        format!("{}…", &hook_raw[..95])
    } else {
        hook_raw
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

    #[test]
    fn test_promote_bug_postmortem() {
        let temp = tempdir().unwrap();
        let postmortem = BugPostmortem {
            slug: "chat-copy-action".to_string(),
            symptom: "Chat text copy failed in context menu".to_string(),
            root_cause: "Context menu was missing Copy action handler".to_string(),
            prevention_invariant: "Always register Copy at window root".to_string(),
            how_to_apply: Some("Verify default_key_bindings includes Copy.".to_string()),
            target_files: vec!["crates/hadron-chamber/src/app/render/chat.rs".to_string()],
            regression_test: Some("crates/hadron-chamber/src/app/mod.rs::tests::test_copy".to_string()),
        };

        let note_path = promote_bug_postmortem(temp.path(), &postmortem).unwrap();
        assert!(note_path.exists());

        let note_content = fs::read_to_string(&note_path).unwrap();
        assert!(note_content.contains("name: bug-chat-copy-action"));
        assert!(note_content.contains("type: postmortem"));
        assert!(note_content.contains("### Symptom\nChat text copy failed in context menu"));
        assert!(note_content.contains("### Root Cause\nContext menu was missing Copy action handler"));
        assert!(note_content.contains("### Prevention Invariant\nAlways register Copy at window root"));
        assert!(note_content.contains("### How to apply:\nVerify default_key_bindings includes Copy."));
        assert!(note_content.contains("crates/hadron-chamber/src/app/render/chat.rs"));

        let index_path = temp.path().join(".hadron").join("nucleus").join("index.md");
        assert!(index_path.exists());
        let index_content = fs::read_to_string(&index_path).unwrap();
        assert!(index_content.contains("- [bug-chat-copy-action](notes/bug-chat-copy-action.md) — Chat text copy failed in context menu -> Always register Copy at window root"));
    }
}
