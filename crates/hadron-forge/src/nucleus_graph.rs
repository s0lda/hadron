//! Pure logic for the `nucleus_graph` tool family.
//! Note topology, wiki-link graph analysis, orphaned note detection, and Mermaid visualization for `.hadron/nucleus/`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};

use crate::file::{ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NucleusGraphAction {
    Topology,
    DeadLinks,
    Orphans,
    Mermaid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NucleusNode {
    pub slug: String,
    pub title: String,
    pub path: String,
    pub outgoing_links: Vec<String>,
    pub incoming_links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NucleusGraphReport {
    pub total_notes: usize,
    pub total_links: usize,
    pub orphaned_notes: Vec<String>,
    pub dead_links: Vec<(String, String)>,
    pub mermaid_diagram: Option<String>,
    pub summary: String,
}

fn strip_code_blocks_and_spans(content: &str) -> String {
    let mut clean = String::with_capacity(content.len());
    let mut in_block = false;

    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            in_block = !in_block;
            clean.push('\n');
            continue;
        }
        if in_block {
            clean.push('\n');
            continue;
        }
        let mut idx = 0;
        let bytes = line.as_bytes();
        while idx < bytes.len() {
            if bytes[idx] == b'`' {
                let mut tick_count = 1;
                while idx + tick_count < bytes.len() && bytes[idx + tick_count] == b'`' {
                    tick_count += 1;
                }
                let needle = &line[idx..idx + tick_count];
                let after = idx + tick_count;
                if let Some(rel_end) = line[after..].find(needle) {
                    idx = after + rel_end + tick_count;
                    continue;
                }
                clean.push_str(needle);
                idx = after;
            } else if let Some(ch) = line[idx..].chars().next() {
                clean.push(ch);
                idx += ch.len_utf8();
            } else {
                break;
            }
        }
        clean.push('\n');
    }
    clean
}

/// Extract links (`[[slug]]` and `[title](notes/slug.md)`) from markdown text.
pub fn extract_note_links(content: &str) -> Vec<String> {
    let content = strip_code_blocks_and_spans(content);
    let mut links = BTreeSet::new();

    // Wiki-links [[slug]]
    let mut cursor = 0;
    while let Some(start) = content[cursor..].find("[[") {
        let abs_start = cursor + start + 2;
        if let Some(end) = content[abs_start..].find("]]") {
            let slug = content[abs_start..abs_start + end].trim().to_string();
            if !slug.is_empty() {
                links.insert(slug);
            }
            cursor = abs_start + end + 2;
        } else {
            break;
        }
    }

    // Markdown links: (notes/slug.md) or (notes/slug)
    let mut cursor_md = 0;
    while let Some(start) = content[cursor_md..].find("](notes/") {
        let abs_start = cursor_md + start + 8;
        if let Some(end) = content[abs_start..].find(')') {
            let raw_target = &content[abs_start..abs_start + end];
            let slug = raw_target.strip_suffix(".md").unwrap_or(raw_target).trim().to_string();
            if !slug.is_empty() {
                links.insert(slug);
            }
            cursor_md = abs_start + end + 1;
        } else {
            break;
        }
    }

    links.into_iter().collect()
}

pub fn build_nucleus_graph(nucleus_dir: &Path) -> (BTreeMap<String, NucleusNode>, Vec<(String, String)>) {
    let mut nodes = BTreeMap::new();
    let mut all_slugs = BTreeSet::new();
    let notes_dir = nucleus_dir.join("notes");

    if let Ok(entries) = fs::read_dir(&notes_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let slug = stem.to_string();
                    all_slugs.insert(slug.clone());
                    if let Ok(content) = fs::read_to_string(&path) {
                        let outgoing = extract_note_links(&content);
                        nodes.insert(
                            slug.clone(),
                            NucleusNode {
                                slug: slug.clone(),
                                title: slug.clone(),
                                path: format!("notes/{}.md", slug),
                                outgoing_links: outgoing,
                                incoming_links: Vec::new(),
                            },
                        );
                    }
                }
            }
        }
    }

    // Parse invariants/*.md (and invariants.md if present) to record "invariants" incoming link
    let mut invariant_files = Vec::new();
    let invariants_dir = nucleus_dir.join("invariants");
    if invariants_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&invariants_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().map_or(false, |ext| ext == "md") {
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
        if let Ok(inv_content) = fs::read_to_string(&inv_file) {
            let inv_links = extract_note_links(&inv_content);
            for target in inv_links {
                if let Some(node) = nodes.get_mut(&target) {
                    if !node.incoming_links.contains(&"invariants".to_string()) {
                        node.incoming_links.push("invariants".to_string());
                    }
                }
            }
            for word in inv_content.split_whitespace() {
                let clean = word.trim_matches(|c: char| c == '`' || c == '(' || c == ')' || c == '[' || c == ']' || c == ',' || c == '.');
                if let Some(rest) = clean.strip_prefix("notes/") {
                    let slug = rest.strip_suffix(".md").unwrap_or(rest);
                    if let Some(node) = nodes.get_mut(slug) {
                        if !node.incoming_links.contains(&"invariants".to_string()) {
                            node.incoming_links.push("invariants".to_string());
                        }
                    }
                }
            }
        }
    }

    // Parse index.md to capture pointers and hook descriptions from the index
    let index_file = nucleus_dir.join("index.md");
    if let Ok(index_content) = fs::read_to_string(&index_file) {
        let index_links = extract_note_links(&index_content);
        for target in index_links {
            if let Some(node) = nodes.get_mut(&target) {
                if !node.incoming_links.contains(&"index".to_string()) {
                    node.incoming_links.push("index".to_string());
                }
            }
        }

        for line in index_content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let mut extracted_slug = None;
            if let Some(start) = trimmed.find("(notes/") {
                let rest = &trimmed[start + 7..];
                if let Some(end) = rest.find(')') {
                    let target = &rest[..end];
                    let slug = target.strip_suffix(".md").unwrap_or(target).trim();
                    if !slug.is_empty() {
                        extracted_slug = Some(slug.to_string());
                    }
                }
            } else if let Some(start) = trimmed.find("[[") {
                let rest = &trimmed[start + 2..];
                if let Some(end) = rest.find("]]") {
                    let slug = rest[..end].trim();
                    if !slug.is_empty() {
                        extracted_slug = Some(slug.to_string());
                    }
                }
            }

            if let Some(slug) = extracted_slug {
                let hook = if let Some(dash_idx) = trimmed.find('—') {
                    let after = &trimmed[dash_idx + '—'.len_utf8()..];
                    let raw_hook = after.split('→').next().unwrap_or(after).trim();
                    raw_hook.to_string()
                } else if let Some(dash_idx) = trimmed.find(" - ") {
                    let after = &trimmed[dash_idx + 3..];
                    let raw_hook = after.split('→').next().unwrap_or(after).trim();
                    raw_hook.to_string()
                } else {
                    String::new()
                };

                if !hook.is_empty() {
                    if let Some(node) = nodes.get_mut(&slug) {
                        node.title = format!("{} — {}", slug, hook);
                    }
                }
            }
        }
    }

    // Populate incoming links and detect dead links
    let mut dead_links = Vec::new();
    let outgoing_records: Vec<(String, Vec<String>)> = nodes
        .iter()
        .map(|(s, n)| (s.clone(), n.outgoing_links.clone()))
        .collect();

    for (from_slug, out_links) in outgoing_records {
        for target in out_links {
            if all_slugs.contains(&target) {
                if let Some(target_node) = nodes.get_mut(&target) {
                    target_node.incoming_links.push(from_slug.clone());
                }
            } else {
                dead_links.push((from_slug.clone(), target));
            }
        }
    }

    (nodes, dead_links)
}

/// Returns true if a node has no incoming links from index or invariants.
pub fn is_orphan_node(node: &NucleusNode) -> bool {
    !node.incoming_links.iter().any(|l| l == "index" || l == "invariants")
}

pub fn generate_mermaid_graph(nodes: &BTreeMap<String, NucleusNode>) -> String {
    let mut out = String::from("```mermaid\ngraph TD\n");
    for (slug, node) in nodes {
        let safe_slug = slug.replace('-', "_");
        out.push_str(&format!("  {}[\"{}\"]\n", safe_slug, slug));
        for target in &node.outgoing_links {
            let safe_target = target.replace('-', "_");
            out.push_str(&format!("  {} --> {}\n", safe_slug, safe_target));
        }
    }
    out.push_str("```\n");
    out
}

pub fn run_nucleus_graph(
    nucleus_root: &Root,
    action: NucleusGraphAction,
) -> Result<NucleusGraphReport, ForgeError> {
    let (nodes, dead_links) = build_nucleus_graph(nucleus_root.path());
    let mut total_links = 0;
    let mut orphaned = Vec::new();

    for (slug, node) in &nodes {
        total_links += node.outgoing_links.len();
        if is_orphan_node(node) {
            orphaned.push(slug.clone());
        }
    }

    let mermaid_diagram = match action {
        NucleusGraphAction::Mermaid | NucleusGraphAction::Topology => Some(generate_mermaid_graph(&nodes)),
        _ => None,
    };

    let summary = format!(
        "Nucleus Knowledge Graph: {} notes, {} link connections, {} orphaned note(s), {} dead link(s).",
        nodes.len(),
        total_links,
        orphaned.len(),
        dead_links.len()
    );

    Ok(NucleusGraphReport {
        total_notes: nodes.len(),
        total_links,
        orphaned_notes: orphaned,
        dead_links,
        mermaid_diagram,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_wiki_and_md_links() {
        let text = "Check [[foo-bar]] and also [other](notes/baz-qux.md).";
        let links = extract_note_links(text);
        assert!(links.contains(&"foo-bar".to_string()));
        assert!(links.contains(&"baz-qux".to_string()));
    }

    #[test]
    fn test_index_hook_enrichment() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus_dir = dir.path();
        let notes_dir = nucleus_dir.join("notes");
        fs::create_dir_all(&notes_dir).unwrap();
        fs::write(notes_dir.join("some-slug.md"), "Some note content").unwrap();
        fs::write(
            nucleus_dir.join("index.md"),
            "- [some-slug](notes/some-slug.md) — Software fallback constraint\n",
        )
        .unwrap();

        let (nodes, _) = build_nucleus_graph(nucleus_dir);
        assert!(nodes.contains_key("some-slug"));
        assert_eq!(
            nodes.get("some-slug").unwrap().title,
            "some-slug — Software fallback constraint"
        );
    }

    #[test]
    fn test_extract_links_strips_code_blocks_and_spans() {
        let text = "Valid [[real-note]] and `[[inline-bin]]` in code.\n```toml\n[[bin]]\nname = \"cli\"\n```\nAnd example in code.";
        let links = extract_note_links(text);
        assert_eq!(links, vec!["real-note".to_string()]);
    }

    #[test]
    fn test_invariants_incoming_links() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus_dir = dir.path();
        let notes_dir = nucleus_dir.join("notes");
        let inv_dir = nucleus_dir.join("invariants");
        fs::create_dir_all(&notes_dir).unwrap();
        fs::create_dir_all(&inv_dir).unwrap();
        fs::write(notes_dir.join("inv-note.md"), "Invariant note content").unwrap();
        fs::write(
            inv_dir.join("always.md"),
            "- Enforces constraint → `notes/inv-note.md`\n",
        )
        .unwrap();

        let (nodes, _) = build_nucleus_graph(nucleus_dir);
        assert!(nodes.contains_key("inv-note"));
        assert!(nodes.get("inv-note").unwrap().incoming_links.contains(&"invariants".to_string()));
    }

    #[test]
    fn test_is_orphan_node_unindexed_with_links() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus_dir = dir.path();
        let notes_dir = nucleus_dir.join("notes");
        fs::create_dir_all(&notes_dir).unwrap();
        fs::write(notes_dir.join("unindexed-a.md"), "Points to [[unindexed-b]]").unwrap();
        fs::write(notes_dir.join("unindexed-b.md"), "Target note").unwrap();

        let (nodes, _) = build_nucleus_graph(nucleus_dir);
        let node_a = nodes.get("unindexed-a").unwrap();
        let node_b = nodes.get("unindexed-b").unwrap();

        assert!(is_orphan_node(node_a));
        assert!(is_orphan_node(node_b));
    }
}

