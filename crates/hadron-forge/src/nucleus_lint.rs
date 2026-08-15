//! Nucleus memory ecosystem and budget linter.
//!
//! Enforces Standard Model Rule 9:
//! 1. Byte-budget guard for `index.md` (default 32 KB threshold).
//! 2. Pointer routing integrity: `- [slug](notes/slug.md)` targets must exist.
//! 3. Note frontmatter validation: `name`, `description`, `metadata.type`.
//! 4. Orphan note detection: flags `.md` files in `notes/` missing from `index.md`.

use std::collections::HashSet;
use serde::{Deserialize, Serialize};

use crate::file::{ForgeError, Root};

const DEFAULT_INDEX_BUDGET_BYTES: usize = 32 * 1024; // 32 KB

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteLintError {
    pub note_file: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NucleusLintReport {
    pub ok: bool,
    pub index_bytes: usize,
    pub index_budget_bytes: usize,
    pub budget_percent: f64,
    pub pointer_count: usize,
    pub note_count: usize,
    pub broken_pointers: Vec<String>,
    pub orphan_notes: Vec<String>,
    pub frontmatter_errors: Vec<NoteLintError>,
    pub warnings: Vec<String>,
    pub summary: String,
}

/// Validate frontmatter format for a single note content string.
pub fn validate_note_frontmatter(slug: &str, content: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        errors.push("Missing leading '---' frontmatter delimiter".to_string());
        return errors;
    }

    let rest = &trimmed[3..];
    let Some(end_idx) = rest.find("---") else {
        errors.push("Missing closing '---' frontmatter delimiter".to_string());
        return errors;
    };

    let yaml_block = &rest[..end_idx];
    let mut has_name = false;
    let mut has_description = false;
    let mut has_metadata_type = false;

    for line in yaml_block.lines() {
        let l = line.trim();
        if l.starts_with("name:") {
            let val = l["name:".len()..].trim();
            if val.is_empty() {
                errors.push("Empty 'name' in frontmatter".to_string());
            } else {
                has_name = true;
                if val != slug {
                    errors.push(format!("'name: {val}' does not match note slug '{slug}'"));
                }
            }
        } else if l.starts_with("description:") {
            let val = l["description:".len()..].trim();
            if val.is_empty() {
                errors.push("Empty 'description' in frontmatter".to_string());
            } else {
                has_description = true;
            }
        } else if l.starts_with("type:") {
            let val = l["type:".len()..].trim();
            let valid_types = ["user", "feedback", "project", "reference"];
            if valid_types.contains(&val) {
                has_metadata_type = true;
            } else {
                errors.push(format!("Invalid metadata type '{val}', expected one of: {:?}", valid_types));
            }
        }
    }

    if !has_name {
        errors.push("Missing 'name:' field in frontmatter".to_string());
    }
    if !has_description {
        errors.push("Missing 'description:' field in frontmatter".to_string());
    }
    if !has_metadata_type {
        errors.push("Missing or invalid 'metadata.type' in frontmatter".to_string());
    }

    errors
}

/// Lint the nucleus index and notes against budget and Standard Model invariants.
pub fn lint_nucleus(
    nucleus_root: &Root,
    budget_kb: Option<usize>,
) -> Result<NucleusLintReport, ForgeError> {
    let root_path = nucleus_root.path();
    let index_path = root_path.join("index.md");
    let notes_dir = root_path.join("notes");

    let index_budget_bytes = budget_kb.map(|k| k * 1024).unwrap_or(DEFAULT_INDEX_BUDGET_BYTES);

    let (index_bytes, index_content) = if index_path.exists() {
        let bytes = std::fs::read(&index_path).map_err(|e| ForgeError::Io(e.to_string()))?;
        let content = String::from_utf8_lossy(&bytes).to_string();
        (bytes.len(), content)
    } else {
        (0, String::new())
    };

    let budget_percent = if index_budget_bytes > 0 {
        ((index_bytes as f64) / (index_budget_bytes as f64)) * 100.0
    } else {
        0.0
    };

    let mut pointer_count = 0;
    let mut referenced_slugs = HashSet::new();
    let mut broken_pointers = Vec::new();
    let mut warnings = Vec::new();

    if index_bytes > index_budget_bytes {
        warnings.push(format!(
            "index.md is {} bytes (budget is {} bytes, {:.1}% of budget)",
            index_bytes, index_budget_bytes, budget_percent
        ));
    }

    // Parse index pointers: `- [slug](notes/slug.md) — hook`
    for (line_no, line) in index_content.lines().enumerate() {
        let l = line.trim();
        if l.starts_with("- [") {
            pointer_count += 1;
            if let (Some(start_brack), Some(end_brack), Some(start_paren), Some(end_paren)) = (
                l.find('['),
                l.find(']'),
                l.find('('),
                l.find(')'),
            ) {
                if start_brack < end_brack && end_brack < start_paren && start_paren < end_paren {
                    let slug = &l[start_brack + 1..end_brack];
                    let target_rel = &l[start_paren + 1..end_paren];
                    referenced_slugs.insert(slug.to_string());

                    let target_file = root_path.join(target_rel);
                    if !target_file.exists() {
                        broken_pointers.push(format!(
                            "Line {}: [{}] targets non-existent file '{}'",
                            line_no + 1, slug, target_rel
                        ));
                    }

                    // Check hook length recommendation
                    if let Some(hook_pos) = l.find("—") {
                        let hook = l[hook_pos + '—'.len_utf8()..].trim();
                        if hook.len() > 140 {
                            warnings.push(format!(
                                "Line {}: hook for [{}] is long ({} chars, recommended ~100)",
                                line_no + 1, slug, hook.len()
                            ));
                        }
                    }
                }
            }
        }
    }

    // Check notes on disk and validate frontmatter
    let mut note_count = 0;
    let mut disk_slugs = HashSet::new();
    let mut frontmatter_errors = Vec::new();

    if notes_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&notes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().map_or(false, |ext| ext == "md") {
                    note_count += 1;
                    let file_stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                    disk_slugs.insert(file_stem.clone());

                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let errs = validate_note_frontmatter(&file_stem, &content);
                        for err in errs {
                            frontmatter_errors.push(NoteLintError {
                                note_file: format!("notes/{file_stem}.md"),
                                error: err,
                            });
                        }
                    }
                }
            }
        }
    }

    // Check orphan notes (in notes/ but not in index.md)
    let mut orphan_notes: Vec<String> = disk_slugs
        .difference(&referenced_slugs)
        .map(|slug| format!("notes/{slug}.md"))
        .collect();
    orphan_notes.sort();

    let ok = index_bytes <= index_budget_bytes
        && broken_pointers.is_empty()
        && frontmatter_errors.is_empty();

    let summary = if ok {
        format!(
            "Nucleus lint PASSED: index.md {}/{} bytes ({:.1}%), {} pointers, {} notes",
            index_bytes, index_budget_bytes, budget_percent, pointer_count, note_count
        )
    } else {
        format!(
            "Nucleus lint FAILED: {} broken pointer(s), {} frontmatter error(s), budget usage {:.1}%",
            broken_pointers.len(), frontmatter_errors.len(), budget_percent
        )
    };

    Ok(NucleusLintReport {
        ok,
        index_bytes,
        index_budget_bytes,
        budget_percent,
        pointer_count,
        note_count,
        broken_pointers,
        orphan_notes,
        frontmatter_errors,
        warnings,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_nucleus() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("index.md");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();

        std::fs::write(
            &index,
            "- [sample-lesson](notes/sample-lesson.md) — A sample hook\n",
        )
        .unwrap();

        std::fs::write(
            notes.join("sample-lesson.md"),
            "---\nname: sample-lesson\ndescription: Retrieval key\nmetadata:\n  type: project\n---\nLesson content.\n",
        )
        .unwrap();

        dir
    }

    #[test]
    fn lint_nucleus_passes_on_valid_structure() {
        let dir = fixture_nucleus();
        let root = Root::new(dir.path());
        let report = lint_nucleus(&root, None).unwrap();
        assert!(report.ok);
        assert_eq!(report.pointer_count, 1);
        assert_eq!(report.note_count, 1);
        assert!(report.broken_pointers.is_empty());
        assert!(report.frontmatter_errors.is_empty());
    }

    #[test]
    fn lint_nucleus_flags_broken_pointer() {
        let dir = fixture_nucleus();
        let root = Root::new(dir.path());
        std::fs::write(
            dir.path().join("index.md"),
            "- [missing-note](notes/missing-note.md) — Hook for missing\n",
        )
        .unwrap();

        let report = lint_nucleus(&root, None).unwrap();
        assert!(!report.ok);
        assert_eq!(report.broken_pointers.len(), 1);
    }

    #[test]
    fn lint_nucleus_flags_frontmatter_errors() {
        let dir = fixture_nucleus();
        let root = Root::new(dir.path());
        std::fs::write(
            dir.path().join("notes").join("sample-lesson.md"),
            "Invalid non-yaml note\n",
        )
        .unwrap();

        let report = lint_nucleus(&root, None).unwrap();
        assert!(!report.ok);
        assert!(!report.frontmatter_errors.is_empty());
    }
}
