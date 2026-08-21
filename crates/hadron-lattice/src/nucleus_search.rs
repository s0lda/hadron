use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NucleusSearchResult {
    pub slug: String,
    pub score: f32,
    pub description: String,
    pub excerpt: String,
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|t| t.len() >= 3)
        .map(|s| s.to_string())
        .collect()
}

pub fn query_nucleus_semantic(
    repo_root: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<NucleusSearchResult>> {
    let notes_dir = repo_root.join(".hadron").join("nucleus").join("notes");
    if !notes_dir.exists() {
        return Ok(Vec::new());
    }

    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return Ok(Vec::new());
    }

    let query_set: HashSet<String> = query_tokens.iter().cloned().collect();
    let mut results = Vec::new();

    for entry in fs::read_dir(notes_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }

        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Extract description from frontmatter if present
        let mut description = String::new();
        let mut in_frontmatter = false;
        let mut body_lines = Vec::new();

        for line in content.lines() {
            if line.trim() == "---" {
                in_frontmatter = !in_frontmatter;
                continue;
            }
            if in_frontmatter {
                if let Some(desc) = line.strip_prefix("description:") {
                    description = desc.trim().to_string();
                }
            } else {
                body_lines.push(line);
            }
        }

        let body = body_lines.join("\n");
        let note_tokens = tokenize(&format!("{slug} {description} {body}"));
        if note_tokens.is_empty() {
            continue;
        }

        let mut token_counts = HashMap::new();
        for tok in &note_tokens {
            *token_counts.entry(tok.clone()).or_insert(0usize) += 1;
        }

        let mut match_score = 0.0f32;
        for q in &query_tokens {
            if let Some(&count) = token_counts.get(q) {
                match_score += 1.0 + (count as f32).ln();
            }
            if slug.contains(q) {
                match_score += 3.0;
            }
            if description.to_lowercase().contains(q) {
                match_score += 2.0;
            }
        }

        if match_score > 0.0 {
            // Find most relevant excerpt
            let mut best_line = String::new();
            let mut max_line_matches = 0;
            for line in &body_lines {
                let l_lower = line.to_lowercase();
                let matches = query_set.iter().filter(|&q| l_lower.contains(q)).count();
                if matches > max_line_matches {
                    max_line_matches = matches;
                    best_line = line.trim().to_string();
                }
            }

            let excerpt = if !best_line.is_empty() {
                best_line
            } else {
                body.chars().take(120).collect::<String>()
            };

            results.push(NucleusSearchResult {
                slug,
                score: match_score,
                description,
                excerpt,
            });
        }
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_nucleus_search_ranking() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let notes = root.join(".hadron").join("nucleus").join("notes");
        fs::create_dir_all(&notes).unwrap();

        fs::write(
            notes.join("compiled-is-not-running.md"),
            "---\nname: compiled-is-not-running\ndescription: Find the caller before reporting a feature works\n---\nPassing tests only prove compilation. A real caller site must execute.",
        )
        .unwrap();

        fs::write(
            notes.join("the-gate-rebases.md"),
            "---\nname: the-gate-rebases\ndescription: Merge gate rebases before running tests\n---\nSync rebase happens first before running cargo test runner.",
        )
        .unwrap();

        let results = query_nucleus_semantic(root, "compiled caller running", 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].slug, "compiled-is-not-running");
        assert!(results[0].score > 0.0);
    }
}
