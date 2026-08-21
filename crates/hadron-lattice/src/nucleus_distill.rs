use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateLesson {
    pub slug: String,
    pub description: String,
    pub fact_markdown: String,
}

pub fn distill_failure_recovery(error_tail: &str, fix_diff: &str) -> Option<CandidateLesson> {
    if error_tail.trim().is_empty() || fix_diff.trim().is_empty() {
        return None;
    }

    let mut slug = "failure-recovery".to_string();
    let mut desc = "Post-mortem distillation of build or test failure recovery".to_string();

    if error_tail.contains("unresolved import") || error_tail.contains("cannot find") {
        slug = "missing-export-or-module-seam".to_string();
        desc = "Verify crate module exports and public visibility before test gate dispatch".to_string();
    } else if error_tail.contains("panicked at") || error_tail.contains("assertion failed") {
        slug = "runtime-assertion-guard".to_string();
        desc = "Runtime assertion triggered due to unhandled invariant edge case".to_string();
    } else if error_tail.contains("E0308") || error_tail.contains("mismatched types") {
        slug = "type-contract-agreement".to_string();
        desc = "Type system contract divergence resolved during merge gate cycle".to_string();
    }

    let fact_markdown = format!(
        "---\nname: {slug}\ndescription: {desc}\nmetadata:\n  type: project\n---\n\
        ### Failure Signal\n```\n{}\n```\n\n\
        ### Remediation Pattern\n```diff\n{}\n```\n\n\
        **Why:** Automated distillation captures compiler/test fixes preventing duplicate swarm turns.\n\
        **How to apply:** Check module tree, trait bounds, and type invariants before submitting to merge gate.\n",
        error_tail.lines().take(5).collect::<Vec<_>>().join("\n"),
        fix_diff.lines().take(10).collect::<Vec<_>>().join("\n")
    );

    Some(CandidateLesson {
        slug,
        description: desc,
        fact_markdown,
    })
}

pub fn write_distilled_lesson(repo_root: &Path, lesson: &CandidateLesson) -> Result<PathBuf> {
    let nucleus_dir = repo_root.join(".hadron").join("nucleus");
    let notes_dir = nucleus_dir.join("notes");
    fs::create_dir_all(&notes_dir)
        .with_context(|| format!("Failed to create notes directory at {:?}", notes_dir))?;

    let note_path = notes_dir.join(format!("{}.md", lesson.slug));
    fs::write(&note_path, &lesson.fact_markdown)?;

    let index_path = nucleus_dir.join("index.md");
    let hook = if lesson.description.len() > 95 {
        format!("{}…", &lesson.description[..94])
    } else {
        lesson.description.clone()
    };

    let pointer_line = format!("- [{slug}](notes/{slug}.md) — {hook}\n", slug = lesson.slug);

    if index_path.exists() {
        let existing = fs::read_to_string(&index_path)?;
        if !existing.contains(&lesson.slug) {
            let mut updated = existing;
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(&pointer_line);
            fs::write(&index_path, updated)?;
        }
    } else {
        fs::write(&index_path, format!("# Memory Index\n\n{pointer_line}"))?;
    }

    Ok(note_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_distill_failure_and_write_note() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let error = "error[E0432]: unresolved import `hadron_forge::visual_smoke`\n --> crates/hadron-forge/tests/visual_smoke_tests.rs:1:19";
        let diff = "+pub mod visual_smoke;\n+pub use visual_smoke::*;";

        let lesson = distill_failure_recovery(error, diff).expect("distilled lesson");
        assert_eq!(lesson.slug, "missing-export-or-module-seam");

        let path = write_distilled_lesson(root, &lesson).expect("write lesson");
        assert!(path.exists());

        let index_content = fs::read_to_string(root.join(".hadron/nucleus/index.md")).unwrap();
        assert!(index_content.contains("- [missing-export-or-module-seam](notes/missing-export-or-module-seam.md)"));
    }
}
