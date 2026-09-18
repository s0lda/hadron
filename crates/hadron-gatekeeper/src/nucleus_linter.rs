use std::collections::HashSet;
use std::path::{Path, PathBuf};
use regex::Regex;

/// Health report returned by the Nucleus integrity linter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NucleusHealthReport {
    pub index_bytes: usize,
    pub index_budget: usize,
    pub budget_exceeded: bool,
    pub orphaned_notes: Vec<PathBuf>,
    pub broken_links: Vec<String>,
    pub total_notes: usize,
    pub indexed_notes: usize,
}

impl NucleusHealthReport {
    /// Returns true if the nucleus is fully healthy (no broken links, no orphans, under budget).
    pub fn is_healthy(&self) -> bool {
        !self.budget_exceeded && self.orphaned_notes.is_empty() && self.broken_links.is_empty()
    }
}

/// Automated health linter auditing `.hadron/nucleus/` for integrity.
pub struct NucleusIntegrityLinter {
    byte_budget: usize,
}

impl NucleusIntegrityLinter {
    /// Standard Model Rule 9: 32 KB prompt budget for `.hadron/nucleus/index.md`.
    pub const DEFAULT_INDEX_BUDGET: usize = 32 * 1024;

    pub fn new(byte_budget: usize) -> Self {
        Self { byte_budget }
    }

    pub fn default_budget() -> Self {
        Self::new(Self::DEFAULT_INDEX_BUDGET)
    }

    /// Creates a linter configured with the team budget resolved for `repo_root`.
    pub fn for_repo(repo_root: &Path) -> Self {
        let team = hadron_lattice::load_team_for_repo(repo_root);
        Self::new(team.nucleus_index_budget_bytes())
    }

    /// Lints the given nucleus directory containing `index.md` and `notes/`.
    pub fn lint(&self, nucleus_dir: &Path) -> NucleusHealthReport {
        let index_path = nucleus_dir.join("index.md");
        let notes_dir = nucleus_dir.join("notes");

        let mut index_bytes = 0;
        let mut referenced_notes = HashSet::new();
        let mut broken_links = Vec::new();

        if index_path.exists() {
            if let Ok(metadata) = std::fs::metadata(&index_path) {
                index_bytes = metadata.len() as usize;
            }
            if let Ok(content) = std::fs::read_to_string(&index_path) {
                let re = Regex::new(r"\[([^\]]+)\]\((notes/[^\)]+\.md)\)").unwrap();
                for line in content.lines() {
                    let trimmed = line.trim();
                    // Real lesson pointers start with a list item: `- [...]` or `* [...]`
                    if (!trimmed.starts_with("- [") && !trimmed.starts_with("* ["))
                        || trimmed.starts_with("- [`")
                        || trimmed.contains("<slug>")
                    {
                        continue;
                    }
                    for cap in re.captures_iter(trimmed) {
                        let rel_path = &cap[2];
                        if rel_path.contains('<') || rel_path.contains('>') {
                            continue;
                        }
                        let target_file = nucleus_dir.join(rel_path);
                        let note_filename = Path::new(rel_path)
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        referenced_notes.insert(note_filename);

                        if !target_file.exists() {
                            broken_links.push(rel_path.to_string());
                        }
                    }
                }
            }
        }

        // Scan invariants for referenced notes (e.g. `invariants/always.md` or `invariants/*.md`).
        // Lessons promoted to permanent invariants per Rule 9 keep their notes in notes/.
        let inv_re = Regex::new(r"notes/([a-zA-Z0-9_\-\.]+\.md)").unwrap();
        let mut invariant_files = Vec::new();
        let invariants_dir = nucleus_dir.join("invariants");
        if invariants_dir.exists() && invariants_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&invariants_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                        invariant_files.push(path);
                    }
                }
            }
        }
        let top_invariants = nucleus_dir.join("invariants.md");
        if top_invariants.is_file() {
            invariant_files.push(top_invariants);
        }

        for inv_file in invariant_files {
            if let Ok(inv_content) = std::fs::read_to_string(&inv_file) {
                for cap in inv_re.captures_iter(&inv_content) {
                    let note_filename = cap[1].to_string();
                    if note_filename.contains('<') || note_filename.contains('>') {
                        continue;
                    }
                    referenced_notes.insert(note_filename.clone());
                    let target_file = notes_dir.join(&note_filename);
                    if !target_file.exists() {
                        let broken = format!("notes/{note_filename}");
                        if !broken_links.contains(&broken) {
                            broken_links.push(broken);
                        }
                    }
                }
            }
        }

        let mut existing_notes = Vec::new();
        if notes_dir.exists() && notes_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&notes_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                        existing_notes.push(path);
                    }
                }
            }
        }

        let mut orphaned_notes = Vec::new();
        for note_path in &existing_notes {
            if let Some(file_name) = note_path.file_name().and_then(|s| s.to_str()) {
                if !referenced_notes.contains(file_name) {
                    orphaned_notes.push(note_path.clone());
                }
            }
        }

        orphaned_notes.sort();
        broken_links.sort();
        broken_links.dedup();

        NucleusHealthReport {
            index_bytes,
            index_budget: self.byte_budget,
            budget_exceeded: index_bytes > self.byte_budget,
            orphaned_notes,
            broken_links,
            total_notes: existing_notes.len(),
            indexed_notes: referenced_notes.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_nucleus_linter_detects_broken_links_and_orphans() {
        let dir = tempdir().unwrap();
        let nucleus = dir.path();
        let notes = nucleus.join("notes");
        std::fs::create_dir_all(&notes).unwrap();

        // Create existing note1.md and orphan.md
        std::fs::write(notes.join("note1.md"), "Fact 1").unwrap();
        std::fs::write(notes.join("orphan.md"), "Orphaned fact").unwrap();

        // index.md references note1.md and a non-existent broken.md
        let index_content = "- [note1](notes/note1.md) — valid lesson\n- [broken](notes/broken.md) — missing note\n";
        std::fs::write(nucleus.join("index.md"), index_content).unwrap();

        let linter = NucleusIntegrityLinter::new(1024);
        let report = linter.lint(nucleus);

        assert!(!report.is_healthy());
        assert!(!report.budget_exceeded);
        assert_eq!(report.total_notes, 2);
        assert_eq!(report.broken_links, vec!["notes/broken.md".to_string()]);
        assert_eq!(report.orphaned_notes, vec![notes.join("orphan.md")]);
    }

    #[test]
    fn test_nucleus_linter_budget_overrun() {
        let dir = tempdir().unwrap();
        let nucleus = dir.path();
        let notes = nucleus.join("notes");
        std::fs::create_dir_all(&notes).unwrap();

        let index_content = "A".repeat(200);
        std::fs::write(nucleus.join("index.md"), index_content).unwrap();

        let linter = NucleusIntegrityLinter::new(100);
        let report = linter.lint(nucleus);

        assert!(report.budget_exceeded);
        assert_eq!(report.index_bytes, 200);
        assert_eq!(report.index_budget, 100);
        assert!(!report.is_healthy());
    }

    #[test]
    fn test_nucleus_linter_healthy_repo() {
        let dir = tempdir().unwrap();
        let nucleus = dir.path();
        let notes = nucleus.join("notes");
        std::fs::create_dir_all(&notes).unwrap();

        std::fs::write(notes.join("fact.md"), "Fact").unwrap();
        let index_content = "- [fact](notes/fact.md) — short hook\n";
        std::fs::write(nucleus.join("index.md"), index_content).unwrap();

        let linter = NucleusIntegrityLinter::default_budget();
        let report = linter.lint(nucleus);

        assert!(report.is_healthy());
        assert_eq!(report.broken_links.len(), 0);
        assert_eq!(report.orphaned_notes.len(), 0);
        assert!(!report.budget_exceeded);
        assert_eq!(report.total_notes, 1);
        assert_eq!(report.indexed_notes, 1);
    }

    #[test]
    fn test_nucleus_linter_ignores_template_placeholder_and_checks_invariants() {
        let dir = tempdir().unwrap();
        let nucleus = dir.path();
        let notes = nucleus.join("notes");
        let invariants = nucleus.join("invariants");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::create_dir_all(&invariants).unwrap();

        // Note referenced only by invariant
        std::fs::write(notes.join("inv-rule.md"), "Invariant Rule").unwrap();

        // index.md has preamble with template line:
        // Shape: `- [<slug>](notes/<slug>.md) — <hook>`
        // and no broken links
        let index_content = "# Index\n\nShape: `- [<slug>](notes/<slug>.md) — <hook>`\n\n## Section\n";
        std::fs::write(nucleus.join("index.md"), index_content).unwrap();

        // Invariants references inv-rule.md
        let inv_content = "- Rule: enforce lock → notes/inv-rule.md\n";
        std::fs::write(invariants.join("always.md"), inv_content).unwrap();

        let linter = NucleusIntegrityLinter::default_budget();
        let report = linter.lint(nucleus);

        assert!(report.is_healthy());
        assert_eq!(report.broken_links.len(), 0);
        assert_eq!(report.orphaned_notes.len(), 0);
        assert_eq!(report.indexed_notes, 1);
    }
}
