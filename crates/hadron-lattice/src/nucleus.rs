//! Dynamic nucleus prompt injector.
//!
//! Loads `.hadron/nucleus/notes/*.md` and ranks them by overlap with the
//! target file paths and the current query, returning the top notes that fit
//! within a byte budget. The chamber, engine, or adapter then injects those
//! notes into the prompt that goes to a quark.
//!
//! See `Dynamic Smart Nucleus Prompt Injector` in
//! `.hadron/docs/plans/2026-08-13-hadron-next-gen-capabilities.md`.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One nucleus note as stored on disk and injected into a prompt.
///
/// `slug` and `description` are the routing metadata (the index line in
/// `index.md` is exactly these two); `content` is the body of the note that
/// only gets paid for on the turns its line says matters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NucleusNote {
    pub slug: String,
    pub description: String,
    pub content: String,
}

impl NucleusNote {
    /// Estimate the byte size of this note when rendered in prompt context.
    pub fn byte_len(&self) -> usize {
        self.slug.len() + self.description.len() + self.content.len() + 16
    }
}

/// Ranks and slices a corpus of [`NucleusNote`]s to fit a prompt byte budget.
#[derive(Debug, Clone, Default)]
pub struct DynamicNucleusInjector {
    notes: Vec<NucleusNote>,
}

impl DynamicNucleusInjector {
    /// Create a new injector from an in-memory collection of notes.
    pub fn new(notes: Vec<NucleusNote>) -> Self {
        Self { notes }
    }

    /// Access the underlying note collection.
    pub fn notes(&self) -> &[NucleusNote] {
        &self.notes
    }

    /// Parse a single markdown note file with optional YAML frontmatter.
    pub fn parse_note(raw: &str, fallback_slug: &str) -> NucleusNote {
        let mut slug = fallback_slug.to_string();
        let mut description = String::new();
        let mut body = raw;

        if let Some(rest) = raw.strip_prefix("---\n") {
            if let Some(end_idx) = rest.find("\n---\n") {
                let frontmatter = &rest[..end_idx];
                body = &rest[end_idx + 5..]; // skip "\n---\n"

                for line in frontmatter.lines() {
                    let trimmed = line.trim();
                    if let Some(val) = trimmed.strip_prefix("name:") {
                        let parsed = val.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !parsed.is_empty() {
                            slug = parsed;
                        }
                    } else if let Some(val) = trimmed.strip_prefix("description:") {
                        description = val.trim().trim_matches('"').trim_matches('\'').to_string();
                    }
                }
            }
        }

        NucleusNote {
            slug,
            description,
            content: body.trim().to_string(),
        }
    }

    /// Load all `.md` notes from a directory (such as `.hadron/nucleus/notes` or `.hadron/nucleus`).
    pub fn load_from_dir(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir_path = dir.as_ref();
        let notes_dir = if dir_path.join("notes").is_dir() {
            dir_path.join("notes")
        } else {
            dir_path.to_path_buf()
        };

        if !notes_dir.exists() {
            return Ok(Self { notes: Vec::new() });
        }

        let mut notes = Vec::new();
        let read_dir = fs::read_dir(&notes_dir)?;
        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                let fallback_slug = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                if fallback_slug == "index" || fallback_slug == "features" || fallback_slug == "invariants" {
                    continue;
                }
                if let Ok(content) = fs::read_to_string(&path) {
                    notes.push(Self::parse_note(&content, fallback_slug));
                }
            }
        }

        notes.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(Self { notes })
    }

    /// Select the most relevant notes given target files and a query, constrained to `budget_bytes`.
    pub fn select_relevant_notes(
        &self,
        target_files: &[&str],
        query: &str,
        budget_bytes: usize,
    ) -> Vec<NucleusNote> {
        if budget_bytes == 0 || self.notes.is_empty() {
            return Vec::new();
        }

        // Tokenize query and target files into query terms
        let mut terms = HashSet::new();
        for file in target_files {
            for token in tokenize(file) {
                terms.insert(token);
            }
        }
        for token in tokenize(query) {
            terms.insert(token);
        }

        // Score each note
        let mut scored: Vec<(i64, &NucleusNote)> = self
            .notes
            .iter()
            .map(|note| {
                let score = score_note(note, &terms);
                (score, note)
            })
            .collect();

        // Sort descending by score, breaking ties by slug ascending
        scored.sort_by(|(s1, n1), (s2, n2)| {
            s2.cmp(s1).then_with(|| n1.slug.cmp(&n2.slug))
        });

        let mut selected = Vec::new();
        let mut used_bytes = 0;

        for (score, note) in scored {
            let note_len = note.byte_len();
            if used_bytes + note_len <= budget_bytes {
                if score > 0 || selected.is_empty() {
                    used_bytes += note_len;
                    selected.push(note.clone());
                }
            }
        }

        selected
    }

    /// Format selected notes into a readable prompt segment.
    pub fn format_notes_section(notes: &[NucleusNote]) -> String {
        if notes.is_empty() {
            return String::new();
        }
        let mut out = String::from("# Dynamic Nucleus Context (Relevant Lessons)\n\n");
        for note in notes {
            out.push_str(&format!("## {}\n", note.slug));
            if !note.description.is_empty() {
                out.push_str(&format!("*{}*\n\n", note.description));
            }
            out.push_str(&format!("{}\n\n", note.content));
        }
        out
    }
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 2)
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

fn score_note(note: &NucleusNote, terms: &HashSet<String>) -> i64 {
    if terms.is_empty() {
        return 0;
    }

    let mut score = 0i64;
    let slug_tokens = tokenize(&note.slug);
    let desc_tokens = tokenize(&note.description);
    let content_tokens = tokenize(&note.content);

    for term in terms {
        for st in &slug_tokens {
            if st == term {
                score += 10;
            } else if st.contains(term) || term.contains(st) {
                score += 4;
            }
        }
        for dt in &desc_tokens {
            if dt == term {
                score += 5;
            } else if dt.contains(term) || term.contains(dt) {
                score += 2;
            }
        }
        for ct in &content_tokens {
            if ct == term {
                score += 1;
            }
        }
    }

    score
}

/// Extracts the title from a research markdown document.
/// Matches `# Research: <Topic>` or `# <Title>`.
pub fn parse_research_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# Research:") {
            let t = rest.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            let t = rest.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parse_research_title_finds_title() {
        let doc1 = "# Research: Dynamic Theme Engine\n\n- **Date**: 2026-08-21\n";
        assert_eq!(
            parse_research_title(doc1),
            Some("Dynamic Theme Engine".to_string())
        );

        let doc2 = "# Custom Color Palettes\n\n## 1. Executive Summary";
        assert_eq!(
            parse_research_title(doc2),
            Some("Custom Color Palettes".to_string())
        );

        let doc3 = "No header line here\njust text";
        assert_eq!(parse_research_title(doc3), None);
    }

    #[test]
    fn parse_note_extracts_frontmatter_and_content() {
        let raw = r#"---
name: sample-note-slug
description: "A description of the lesson"
metadata:
  type: feedback
---

This is the body of the note explaining the lesson.
"#;
        let note = DynamicNucleusInjector::parse_note(raw, "fallback");
        assert_eq!(note.slug, "sample-note-slug");
        assert_eq!(note.description, "A description of the lesson");
        assert_eq!(
            note.content,
            "This is the body of the note explaining the lesson."
        );
    }

    #[test]
    fn parse_note_falls_back_when_no_frontmatter() {
        let raw = "Just raw markdown content without yaml.";
        let note = DynamicNucleusInjector::parse_note(raw, "my-fallback-slug");
        assert_eq!(note.slug, "my-fallback-slug");
        assert_eq!(note.description, "");
        assert_eq!(note.content, raw);
    }

    #[test]
    fn dynamic_injector_respects_byte_budget_and_ranks_by_relevance() {
        let note1 = NucleusNote {
            slug: "chat-rendering-pipeline".to_string(),
            description: "Chat viewport rendering constraints in GPUI".to_string(),
            content: "Always check char boundaries in chat rows.".to_string(),
        };
        let note2 = NucleusNote {
            slug: "git-worktree-isolation".to_string(),
            description: "Worktree path rules and target isolation".to_string(),
            content: "Never build target inside child tree.".to_string(),
        };
        let note3 = NucleusNote {
            slug: "unrelated-audio-decoder".to_string(),
            description: "Audio buffer overflow".to_string(),
            content: "Unrelated details.".to_string(),
        };

        let injector = DynamicNucleusInjector::new(vec![note1.clone(), note2.clone(), note3.clone()]);
        let notes = injector.select_relevant_notes(
            &["crates/hadron-chamber/src/app/render/chat.rs"],
            "chat render font boundaries",
            1000,
        );

        assert!(!notes.is_empty());
        assert_eq!(notes[0].slug, "chat-rendering-pipeline");
        let total_bytes: usize = notes.iter().map(|n| n.byte_len()).sum();
        assert!(total_bytes <= 1000);
    }

    #[test]
    fn load_from_dir_loads_notes_and_skips_indexes() {
        let dir = tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        fs::create_dir_all(&notes_dir).unwrap();

        let mut f1 = fs::File::create(notes_dir.join("index.md")).unwrap();
        writeln!(f1, "# Index").unwrap();

        let mut f2 = fs::File::create(notes_dir.join("test-lesson.md")).unwrap();
        writeln!(
            f2,
            "---\nname: test-lesson\ndescription: \"Lesson test\"\n---\n\nContent here."
        )
        .unwrap();

        let injector = DynamicNucleusInjector::load_from_dir(dir.path()).unwrap();
        assert_eq!(injector.notes().len(), 1);
        assert_eq!(injector.notes()[0].slug, "test-lesson");
    }

    #[test]
    fn zero_budget_returns_empty() {
        let note = NucleusNote {
            slug: "test".to_string(),
            description: "test".to_string(),
            content: "test".to_string(),
        };
        let injector = DynamicNucleusInjector::new(vec![note]);
        let notes = injector.select_relevant_notes(&["file.rs"], "query", 0);
        assert!(notes.is_empty());
    }

    #[test]
    fn formats_notes_section_properly() {
        let note = NucleusNote {
            slug: "test-slug".to_string(),
            description: "Desc".to_string(),
            content: "Body".to_string(),
        };
        let formatted = DynamicNucleusInjector::format_notes_section(&[note]);
        assert!(formatted.contains("## test-slug"));
        assert!(formatted.contains("*Desc*"));
        assert!(formatted.contains("Body"));
    }
}
