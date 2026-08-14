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
}
