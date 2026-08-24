//! Nucleus health auto-linter and budget enforcer.
//!
//! Validates:
//! 1. `index.md` size against the 32 KB budget limit.
//! 2. Broken/orphaned index links (lines in `index.md` without corresponding `notes/<slug>.md`).
//! 3. Unindexed notes (files in `notes/` without entry in `index.md`).
//! 4. Missing feature entrypoint files listed in `features.md`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Default budget ceiling for `index.md` in bytes (32 KB).
pub const DEFAULT_INDEX_BUDGET_BYTES: usize = 32 * 1024;

/// Specific linting issue detected in the Nucleus knowledge base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintIssue {
    /// `index.md` exceeded the allowed budget.
    IndexBudgetExceeded { size_bytes: usize, budget_bytes: usize },
    /// `index.md` contains a pointer to a note file that does not exist.
    OrphanedIndexPointer { slug: String, target_path: String },
    /// A note file exists in `notes/` but is not referenced in `index.md`.
    UnindexedNote { slug: String, path: PathBuf },
    /// An entrypoint path listed in `features.md` does not exist on disk.
    MissingFeatureEntrypoint { feature: String, entrypoint: String },
    /// Note file could not be read or has malformed metadata.
    MalformedNote { path: PathBuf, reason: String },
    /// Index pointer hook line exceeds recommended length.
    HookTooLong { slug: String, hook_length: usize, max_length: usize },
    /// A postmortem note contains a candidate invariant suitable for promotion.
    PostmortemPromotionCandidate { slug: String, invariant: String },
}

/// Aggregated report of nucleus health.
#[derive(Debug, Clone, Default)]
pub struct LintReport {
    pub issues: Vec<LintIssue>,
    pub index_bytes: usize,
    pub note_count: usize,
    pub feature_count: usize,
}

impl LintReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Nucleus health linter.
pub struct NucleusLinter {
    budget_bytes: usize,
}

impl Default for NucleusLinter {
    fn default() -> Self {
        Self {
            budget_bytes: DEFAULT_INDEX_BUDGET_BYTES,
        }
    }
}

impl NucleusLinter {
    pub fn new(budget_bytes: usize) -> Self {
        Self { budget_bytes }
    }

    /// Run full nucleus lint against a `.hadron/nucleus` directory and its parent workspace root.
    pub fn lint(&self, nucleus_dir: impl AsRef<Path>, repo_root: Option<&Path>) -> LintReport {
        let n_dir = nucleus_dir.as_ref();
        let mut report = LintReport::default();

        let index_path = n_dir.join("index.md");
        let mut indexed_slugs = HashSet::new();

        if index_path.is_file() {
            if let Ok(content) = fs::read_to_string(&index_path) {
                report.index_bytes = content.len();
                if content.len() > self.budget_bytes {
                    report.issues.push(LintIssue::IndexBudgetExceeded {
                        size_bytes: content.len(),
                        budget_bytes: self.budget_bytes,
                    });
                }

                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("- [") {
                        if let Some(bracket_end) = rest.find(']') {
                            let slug = &rest[..bracket_end];
                            indexed_slugs.insert(slug.to_string());

                            let after_bracket = &rest[bracket_end + 1..];
                            if let Some(paren_start) = after_bracket.find('(') {
                                if let Some(paren_end) = after_bracket[paren_start + 1..].find(')') {
                                    let rel_target = &after_bracket[paren_start + 1..paren_start + 1 + paren_end];
                                    let full_target = n_dir.join(rel_target);
                                    if !full_target.is_file() {
                                        report.issues.push(LintIssue::OrphanedIndexPointer {
                                            slug: slug.to_string(),
                                            target_path: rel_target.to_string(),
                                        });
                                    }

                                    // Check hook length (Standard Model Rule 9: ~100 characters max)
                                    let after_paren = &after_bracket[paren_start + 1 + paren_end + 1..];
                                    if let Some(dash_idx) = after_paren.find('—').or_else(|| after_paren.find("--")) {
                                        let hook = after_paren[dash_idx..].trim_start_matches(|c| c == '—' || c == '-' || c == ' ').trim();
                                        if hook.chars().count() > 100 {
                                            report.issues.push(LintIssue::HookTooLong {
                                                slug: slug.to_string(),
                                                hook_length: hook.chars().count(),
                                                max_length: 100,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let notes_dir = n_dir.join("notes");
        if notes_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&notes_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("md") {
                        report.note_count += 1;
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            if !indexed_slugs.contains(stem) {
                                report.issues.push(LintIssue::UnindexedNote {
                                    slug: stem.to_string(),
                                    path: path.clone(),
                                });
                            }

                            // Detect candidate invariant promotions in postmortems
                            if stem.starts_with("bug-") {
                                if let Ok(note_body) = fs::read_to_string(&path) {
                                    if note_body.contains("type: postmortem") && note_body.contains("### Prevention Invariant") {
                                        if let Some(inv_start) = note_body.find("### Prevention Invariant") {
                                            let after = &note_body[inv_start + "### Prevention Invariant".len()..];
                                            let inv_text = after.lines()
                                                .skip_while(|l| l.trim().is_empty())
                                                .take_while(|l| !l.starts_with('#'))
                                                .collect::<Vec<_>>()
                                                .join(" ")
                                                .trim()
                                                .to_string();
                                            if !inv_text.is_empty() {
                                                report.issues.push(LintIssue::PostmortemPromotionCandidate {
                                                    slug: stem.to_string(),
                                                    invariant: inv_text,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let features_path = n_dir.join("features.md");
        if features_path.is_file() {
            if let Ok(content) = fs::read_to_string(&features_path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('|') && !trimmed.contains("---|---") && !trimmed.contains("Feature |") && !trimmed.contains("Feature|") {
                        let parts: Vec<&str> = trimmed.split('|').map(|s| s.trim()).collect();
                        // Format: | **Feature Name** | Description | Status | Entrypoint Files |
                        if parts.len() >= 5 {
                            let feature = parts[1].trim_matches('*').trim();
                            let entrypoints = parts[4];
                            if !feature.is_empty() && !entrypoints.is_empty() && entrypoints != "Entrypoint Files" {
                                report.feature_count += 1;
                                if let Some(root) = repo_root {
                                    for ep in entrypoints.split(',') {
                                        let ep_clean = ep.trim().trim_matches('`').trim();
                                        if !ep_clean.is_empty() && !ep_clean.starts_with("Cargo.toml") {
                                            let ep_path = root.join(ep_clean);
                                            // Handle cases with line/symbol annotations or parent dirs
                                            let clean_file = ep_clean.split(':').next().unwrap_or(ep_clean).trim();
                                            let clean_path = root.join(clean_file);
                                            if !clean_path.exists() && !ep_path.exists() {
                                                report.issues.push(LintIssue::MissingFeatureEntrypoint {
                                                    feature: feature.to_string(),
                                                    entrypoint: ep_clean.to_string(),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        report
    }
}

/// Promotes a postmortem's prevention invariant to `invariants/always.md` (or `invariants.md`)
/// and prunes the pointer from `index.md`.
pub fn promote_postmortem_to_invariants(repo_root: &Path, slug: &str) -> std::io::Result<()> {
    let nucleus_dir = repo_root.join(".hadron").join("nucleus");
    let note_path = nucleus_dir.join("notes").join(format!("{}.md", slug));
    if !note_path.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Postmortem note {} not found", note_path.display()),
        ));
    }

    let note_content = fs::read_to_string(&note_path)?;
    let invariant_text = if let Some(pos) = note_content.find("### Prevention Invariant") {
        let after = &note_content[pos + "### Prevention Invariant".len()..];
        after.lines()
            .skip_while(|l| l.trim().is_empty())
            .take_while(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Note has no ### Prevention Invariant section",
        ));
    };

    // Append to invariants file
    let invariants_dir = nucleus_dir.join("invariants");
    fs::create_dir_all(&invariants_dir)?;
    let invariants_path = invariants_dir.join("always.md");
    let title = slug.trim_start_matches("bug-").replace('-', " ");
    let mut inv_content = fs::read_to_string(&invariants_path).unwrap_or_default();
    if !inv_content.ends_with('\n') && !inv_content.is_empty() {
        inv_content.push('\n');
    }
    inv_content.push_str(&format!("- **{}**: {}\n", title, invariant_text));
    fs::write(&invariants_path, inv_content)?;

    // Prune pointer from index.md
    let index_path = nucleus_dir.join("index.md");
    if index_path.is_file() {
        let index_content = fs::read_to_string(&index_path)?;
        let pruned: Vec<&str> = index_content
            .lines()
            .filter(|l| !l.contains(&format!("[{}]", slug)))
            .collect();
        fs::write(&index_path, pruned.join("\n") + "\n")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn lint_detects_orphaned_pointers_and_unindexed_notes() {
        let tmp = tempdir().unwrap();
        let nucleus = tmp.path().join(".hadron/nucleus");
        let notes = nucleus.join("notes");
        fs::create_dir_all(&notes).unwrap();

        // 1. Create index.md with one valid and one orphaned pointer
        let index_content = "\
# Memory index
- [valid-note](notes/valid-note.md) — A valid note hook
- [missing-note](notes/missing-note.md) — Pointer to missing file
";
        fs::write(nucleus.join("index.md"), index_content).unwrap();

        // 2. Create valid-note.md and an unindexed-note.md
        fs::write(notes.join("valid-note.md"), "---\nname: valid-note\n---\nValid note body").unwrap();
        fs::write(notes.join("unindexed-note.md"), "---\nname: unindexed-note\n---\nUnindexed body").unwrap();

        let linter = NucleusLinter::default();
        let report = linter.lint(&nucleus, None);

        assert_eq!(report.note_count, 2);
        assert!(!report.is_clean());

        let has_orphaned = report.issues.iter().any(|i| matches!(i, LintIssue::OrphanedIndexPointer { slug, .. } if slug == "missing-note"));
        let has_unindexed = report.issues.iter().any(|i| matches!(i, LintIssue::UnindexedNote { slug, .. } if slug == "unindexed-note"));

        assert!(has_orphaned);
        assert!(has_unindexed);
    }

    #[test]
    fn lint_detects_budget_overruns() {
        let tmp = tempdir().unwrap();
        let nucleus = tmp.path().join("nucleus");
        fs::create_dir_all(&nucleus).unwrap();

        let large_index = "a".repeat(1000);
        fs::write(nucleus.join("index.md"), &large_index).unwrap();

        let linter = NucleusLinter::new(500); // 500 byte limit
        let report = linter.lint(&nucleus, None);

        let budget_issue = report.issues.iter().any(|i| matches!(i, LintIssue::IndexBudgetExceeded { size_bytes: 1000, budget_bytes: 500 }));
        assert!(budget_issue);
    }

    #[test]
    fn test_promote_postmortem_to_invariants() {
        let tmp = tempdir().unwrap();
        let nucleus = tmp.path().join(".hadron/nucleus");
        let notes = nucleus.join("notes");
        fs::create_dir_all(&notes).unwrap();

        let index_content = "- [bug-sample](notes/bug-sample.md) — symptom -> invariant\n";
        fs::write(nucleus.join("index.md"), index_content).unwrap();

        let note_content = "\
---
name: bug-sample
metadata:
  type: postmortem
---

### Symptom
Something crashed

### Prevention Invariant
Never divide by zero
";
        fs::write(notes.join("bug-sample.md"), note_content).unwrap();

        promote_postmortem_to_invariants(tmp.path(), "bug-sample").unwrap();

        let invariants_file = nucleus.join("invariants").join("always.md");
        assert!(invariants_file.is_file());
        let inv_text = fs::read_to_string(invariants_file).unwrap();
        assert!(inv_text.contains("Never divide by zero"));

        let index_after = fs::read_to_string(nucleus.join("index.md")).unwrap();
        assert!(!index_after.contains("bug-sample"));
    }
}
