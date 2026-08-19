//! Autonomous Invariant Synthesis Engine (Capability #19).
//!
//! Analyzes compiler errors (e.g., E0277, E0308, E0502), test panics, and merge conflict traces
//! to automatically synthesize structured, persistent Standard Model invariant rules.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvariantCategory {
    CompileError,
    BorrowChecker,
    TestFailure,
    RuntimePanic,
    MergeConflict,
    SecurityViolation,
}

impl InvariantCategory {
    pub fn directory_name(&self) -> &'static str {
        match self {
            Self::CompileError => "compiler",
            Self::BorrowChecker => "borrow_checker",
            Self::TestFailure => "tests",
            Self::RuntimePanic => "runtime",
            Self::MergeConflict => "merge",
            Self::SecurityViolation => "security",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesizedInvariant {
    pub slug: String,
    pub category: InvariantCategory,
    pub description: String,
    pub rule_markdown: String,
}

/// Analyzes failure diagnostic text and extracts root cause to synthesize a structured invariant.
pub fn synthesize_invariant(failure_text: &str, category: InvariantCategory) -> SynthesizedInvariant {
    let lines: Vec<&str> = failure_text.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    
    match category {
        InvariantCategory::BorrowChecker => {
            let error_line = lines.iter().find(|l| l.contains("cannot borrow") || l.contains("already borrowed")).unwrap_or(&"Borrow checker violation detected");
            let slug = "prevent-concurrent-mutable-borrow".to_string();
            let description = format!("Guard against {}", error_line.chars().take(80).collect::<String>());
            let rule_markdown = format!(
                "---\nname: {}\ndescription: {}\nmetadata:\n  type: project\n---\n\n## Invariant: {}\n\n**Why:** {}\n\n**How to apply:** Clone entities or extract immutable state before calling mutable update closures.\n",
                slug, description, description, failure_text.trim()
            );
            SynthesizedInvariant {
                slug,
                category,
                description,
                rule_markdown,
            }
        }
        InvariantCategory::CompileError => {
            let error_code = lines.iter().find_map(|l| {
                if let Some(pos) = l.find("error[E") {
                    let end = l[pos..].find(']')?;
                    Some(l[pos..pos + end + 1].to_string())
                } else {
                    None
                }
            }).unwrap_or_else(|| "compiler-error".to_string());

            let slug = format!("fix-{}", error_code.replace('[', "-").replace(']', "").to_ascii_lowercase());
            let first_err = lines.iter().find(|l| l.contains("error[") || l.contains("error:")).unwrap_or(&"Compiler diagnostic failure");
            let description = format!("Enforce type and signature constraints for {}", first_err.chars().take(70).collect::<String>());
            let rule_markdown = format!(
                "---\nname: {}\ndescription: {}\nmetadata:\n  type: project\n---\n\n## Invariant: {}\n\n**Why:** Diagnostic output reported `{}`.\n\n**How to apply:** Ensure required trait bounds, lifetimes, or method parameters match signatures before invoking.\n",
                slug, description, description, first_err
            );
            SynthesizedInvariant {
                slug,
                category,
                description,
                rule_markdown,
            }
        }
        InvariantCategory::TestFailure => {
            let panic_line = lines.iter().find(|l| l.contains("panicked at") || l.contains("FAILED") || l.contains("assertion failed")).unwrap_or(&"Test assertion failure");
            let slug = "prevent-test-assertion-regression".to_string();
            let description = format!("Prevent test regression: {}", panic_line.chars().take(75).collect::<String>());
            let rule_markdown = format!(
                "---\nname: {}\ndescription: {}\nmetadata:\n  type: project\n---\n\n## Invariant: {}\n\n**Why:** Test suite failure: `{}`.\n\n**How to apply:** Always verify invariants and boundary conditions before committing.\n",
                slug, description, description, panic_line
            );
            SynthesizedInvariant {
                slug,
                category,
                description,
                rule_markdown,
            }
        }
        _ => {
            let slug = "gate-failure-remediation".to_string();
            let description = "Enforce operational gate validation invariants".to_string();
            let rule_markdown = format!(
                "---\nname: {}\ndescription: {}\nmetadata:\n  type: project\n---\n\n## Invariant: Gate remediation rule\n\n**Why:** Gate check reported failure.\n\n**How to apply:** Validate workspace integrity before merge.\n",
                slug, description
            );
            SynthesizedInvariant {
                slug,
                category,
                description,
                rule_markdown,
            }
        }
    }
}

/// Persists a synthesized invariant to `.hadron/nucleus/invariants/<category>/<slug>.md`.
pub fn write_synthesized_invariant(
    repo_root: &Path,
    invariant: &SynthesizedInvariant,
) -> io::Result<PathBuf> {
    let invariants_dir = repo_root
        .join(".hadron")
        .join("nucleus")
        .join("invariants")
        .join(invariant.category.directory_name());
    
    fs::create_dir_all(&invariants_dir)?;
    let file_path = invariants_dir.join(format!("{}.md", invariant.slug));
    fs::write(&file_path, &invariant.rule_markdown)?;
    Ok(file_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_invariant_synthesis_from_compiler_error() {
        let compiler_output = r#"
error[E0277]: the trait bound `PopupMenuItem: From<&str>` is not satisfied
  --> crates/hadron-chamber/src/app/render/chat.rs:873:29
   |
873 |                         menu.item("Promote to Nucleus Lesson")
"#;
        let inv = synthesize_invariant(compiler_output, InvariantCategory::CompileError);
        assert_eq!(inv.slug, "fix-error-e0277");
        assert!(inv.description.contains("Enforce type and signature constraints"));
        assert!(inv.rule_markdown.contains("PopupMenuItem"));
        assert!(inv.rule_markdown.contains("**Why:**"));
        assert!(inv.rule_markdown.contains("**How to apply:**"));

        let temp = tempdir().unwrap();
        let path = write_synthesized_invariant(temp.path(), &inv).unwrap();
        assert!(path.exists());
        assert!(path.to_string_lossy().contains("compiler/fix-error-e0277.md"));
    }

    #[test]
    fn test_invariant_synthesis_from_borrow_checker() {
        let borrow_output = "error[E0502]: cannot borrow `*this` as mutable because it is also borrowed as immutable";
        let inv = synthesize_invariant(borrow_output, InvariantCategory::BorrowChecker);
        assert_eq!(inv.slug, "prevent-concurrent-mutable-borrow");
        assert!(inv.description.contains("cannot borrow"));
    }
}
