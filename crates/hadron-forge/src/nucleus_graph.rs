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

/// Extract links (`[[slug]]` and `[title](notes/slug.md)`) from markdown text.
pub fn extract_note_links(content: &str) -> Vec<String> {
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

    // Parse index.md to capture pointers from the index
    let index_file = nucleus_dir.join("index.md");
    if let Ok(index_content) = fs::read_to_string(index_file) {
        let index_links = extract_note_links(&index_content);
        for target in index_links {
            if let Some(node) = nodes.get_mut(&target) {
                node.incoming_links.push("index".to_string());
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
        if node.incoming_links.is_empty() && node.outgoing_links.is_empty() {
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
}
