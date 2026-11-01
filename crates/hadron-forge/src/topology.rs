use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};

/// A node in the spatial architecture canvas graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TopologyNode {
    Crate { name: String, path: String },
    Quark { id: String, role: String, state: String },
    Worktree { path: String, branch: String },
    Wiretap { channel: String, packet_count: usize },
}

/// Intra-crate module or source file representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateModuleNode {
    pub name: String,
    pub rel_path: String,
    pub imports: Vec<String>,
}

/// A directed edge in the spatial architecture canvas graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologyEdge {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
}

/// Interactive Spatial Topology Graph for GPUI visualizer and Forge tooling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceTopologyGraph {
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
    #[serde(default)]
    pub crate_modules: Vec<CrateModuleNode>,
}

impl WorkspaceTopologyGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            crate_modules: Vec::new(),
        }
    }

    pub fn add_crate(&mut self, name: &str, path: &str) {
        self.nodes.push(TopologyNode::Crate {
            name: name.to_string(),
            path: path.to_string(),
        });
    }

    pub fn add_quark(&mut self, id: &str, role: &str, state: &str) {
        self.nodes.push(TopologyNode::Quark {
            id: id.to_string(),
            role: role.to_string(),
            state: state.to_string(),
        });
    }

    pub fn add_worktree(&mut self, path: &str, branch: &str) {
        self.nodes.push(TopologyNode::Worktree {
            path: path.to_string(),
            branch: branch.to_string(),
        });
    }

    pub fn add_dependency(&mut self, from_crate: &str, to_crate: &str) {
        self.edges.push(TopologyEdge {
            source_id: from_crate.to_string(),
            target_id: to_crate.to_string(),
            relation: "depends_on".to_string(),
        });
    }

    pub fn add_quark_worktree_link(&mut self, quark_id: &str, worktree_branch: &str) {
        self.edges.push(TopologyEdge {
            source_id: quark_id.to_string(),
            target_id: worktree_branch.to_string(),
            relation: "operates_in".to_string(),
        });
    }

    /// Dynamically scans `crates/*/Cargo.toml` in `repo_root` to extract crates, inter-crate dependencies,
    /// and intra-crate modules/imports. Falls back to default Hadron crates if `crates/` does not exist.
    pub fn from_workspace(repo_root: &Path) -> Self {
        let mut graph = Self::new();
        let crates_dir = repo_root.join("crates");

        let mut scanned_crates: Vec<(String, String, std::path::PathBuf)> = Vec::new();

        let workspace_members = parse_workspace_members(&repo_root.join("Cargo.toml"));

        if crates_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&crates_dir) {
                for entry in entries.flatten() {
                    let dir_path = entry.path();
                    if dir_path.is_dir() {
                        let cargo_toml = dir_path.join("Cargo.toml");
                        if cargo_toml.is_file() {
                            let dir_name = dir_path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                            if dir_name.is_empty() || dir_name.starts_with('.') || dir_name == "gpui-component" {
                                continue;
                            }
                            let rel_crate_path = format!("crates/{}", dir_name);
                            if !workspace_members.is_empty() && !workspace_members.contains(&rel_crate_path) {
                                continue;
                            }
                            let mut crate_name = dir_name.clone();
                            if let Ok(content) = fs::read_to_string(&cargo_toml) {
                                if let Some(pkg_name) = parse_package_name(&content) {
                                    crate_name = pkg_name;
                                }
                            }
                            scanned_crates.push((crate_name, rel_crate_path, dir_path));
                        }
                    }
                }
            }
        }

        if scanned_crates.is_empty() {
            // Fallback for temporary or non-standard directory structures
            graph.add_crate("hadron-chamber", "crates/hadron-chamber");
            graph.add_crate("hadron-gluon", "crates/hadron-gluon");
            graph.add_crate("hadron-lattice", "crates/hadron-lattice");
            graph.add_crate("hadron-gatekeeper", "crates/hadron-gatekeeper");
            graph.add_crate("hadron-forge", "crates/hadron-forge");
            graph.add_crate("hadron-forge-mcp", "crates/hadron-forge-mcp");

            graph.add_dependency("hadron-chamber", "hadron-gluon");
            graph.add_dependency("hadron-gluon", "hadron-lattice");
            graph.add_dependency("hadron-gluon", "hadron-gatekeeper");
            graph.add_dependency("hadron-gluon", "hadron-forge");
            graph.add_dependency("hadron-forge-mcp", "hadron-forge");
            return graph;
        }

        // Sort scanned crates predictably
        scanned_crates.sort_by(|a, b| a.0.cmp(&b.0));

        let known_crate_names: Vec<String> = scanned_crates.iter().map(|(n, _, _)| n.clone()).collect();

        for (crate_name, rel_path, dir_path) in &scanned_crates {
            graph.add_crate(crate_name, rel_path);

            // Extract dependencies from Cargo.toml
            let cargo_toml = dir_path.join("Cargo.toml");
            if let Ok(content) = fs::read_to_string(&cargo_toml) {
                for known in &known_crate_names {
                    if known != crate_name && cargo_toml_depends_on(&content, known) {
                        graph.add_dependency(crate_name, known);
                    }
                }
            }

            // Extract intra-crate src/**/*.rs files
            let src_dir = dir_path.join("src");
            if src_dir.is_dir() {
                let mut rs_files = Vec::new();
                collect_rs_files(&src_dir, &mut rs_files);
                rs_files.sort();

                for file_path in rs_files {
                    let rel_file_path = file_path
                        .strip_prefix(repo_root)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| file_path.to_string_lossy().replace('\\', "/"));

                    let mod_name = file_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let mut imports = Vec::new();
                    if let Ok(code) = fs::read_to_string(&file_path) {
                        for line in code.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("mod ")
                                || trimmed.starts_with("use crate::")
                                || trimmed.starts_with("use super::")
                                || trimmed.starts_with("use hadron_")
                            {
                                imports.push(trimmed.to_string());
                            }
                        }
                    }

                    graph.crate_modules.push(CrateModuleNode {
                        name: mod_name,
                        rel_path: rel_file_path,
                        imports,
                    });
                }
            }
        }

        graph
    }

    /// Computes topological layer index for each crate name.
    pub fn compute_crate_layers(&self) -> BTreeMap<String, usize> {
        let mut crates: Vec<String> = Vec::new();
        for node in &self.nodes {
            if let TopologyNode::Crate { name, .. } = node {
                crates.push(name.clone());
            }
        }

        let mut dep_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for c in &crates {
            dep_map.insert(c.clone(), Vec::new());
        }

        for edge in &self.edges {
            if edge.relation == "depends_on" {
                if dep_map.contains_key(&edge.source_id) && crates.contains(&edge.target_id) {
                    dep_map.entry(edge.source_id.clone()).or_default().push(edge.target_id.clone());
                }
            }
        }

        let mut layers: BTreeMap<String, usize> = BTreeMap::new();
        for c in &crates {
            layers.insert(c.clone(), 0);
        }

        // Iterative relaxation up to crates.len() passes
        let num_crates = crates.len();
        for _ in 0..num_crates {
            let mut changed = false;
            for c in &crates {
                if let Some(deps) = dep_map.get(c) {
                    let mut max_dep_layer = 0;
                    let mut has_dep = false;
                    for dep in deps {
                        if dep != c {
                            if let Some(&dep_layer) = layers.get(dep) {
                                has_dep = true;
                                if dep_layer >= max_dep_layer {
                                    max_dep_layer = dep_layer;
                                }
                            }
                        }
                    }
                    let target_layer = if has_dep { max_dep_layer + 1 } else { 0 };
                    if layers.get(c) != Some(&target_layer) {
                        layers.insert(c.clone(), target_layer);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }

        layers
    }
}

fn parse_workspace_members(root_cargo_toml: &Path) -> Vec<String> {
    let Ok(content) = fs::read_to_string(root_cargo_toml) else {
        return Vec::new();
    };
    let mut in_workspace = false;
    let mut in_members_array = false;
    let mut members_raw = String::new();

    for line in content.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if code.starts_with('[') && code.ends_with(']') {
            in_workspace = code == "[workspace]";
            if !in_workspace && in_members_array {
                break;
            }
            continue;
        }
        if !in_workspace {
            continue;
        }
        if !in_members_array {
            if let Some(rest) = code.strip_prefix("members") {
                let rest = rest.trim_start();
                if let Some(after_eq) = rest.strip_prefix('=') {
                    in_members_array = true;
                    members_raw.push_str(after_eq);
                    if after_eq.contains(']') {
                        break;
                    }
                }
            }
        } else {
            members_raw.push(' ');
            members_raw.push_str(code);
            if code.contains(']') {
                break;
            }
        }
    }

    let mut members = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in members_raw.chars() {
        if ch == '"' || ch == '\'' {
            if in_quotes {
                let trimmed = current.trim().trim_end_matches('/');
                if !trimmed.is_empty() && !trimmed.contains('*') {
                    members.push(trimmed.to_string());
                }
                current.clear();
            }
            in_quotes = !in_quotes;
        } else if in_quotes {
            current.push(ch);
        }
    }
    members
}

fn parse_package_name(cargo_toml_content: &str) -> Option<String> {
    let mut in_package = false;
    for line in cargo_toml_content.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if code.starts_with('[') && code.ends_with(']') {
            in_package = code == "[package]";
            continue;
        }
        if in_package && code.starts_with("name") {
            if let Some((key, val)) = code.split_once('=') {
                if key.trim() == "name" {
                    let cleaned = val.trim().trim_matches('"').trim_matches('\'');
                    if !cleaned.is_empty() {
                        return Some(cleaned.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Checks whether a `Cargo.toml` manifest declares `dep_name` as a dependency
/// (`<dep> = ...`, `<dep>.workspace = true`, `[dependencies.<dep>]`, or `package = "<dep>"`),
/// avoiding substring false positives (such as `"hadron"` matching `"hadron-lattice"`).
pub fn cargo_toml_depends_on(cargo_toml_content: &str, dep_name: &str) -> bool {
    let mut in_dep_section = false;
    let dep_table_suffix = format!(".{}]", dep_name);

    for line in cargo_toml_content.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        if code.starts_with('[') && code.ends_with(']') {
            if code.contains("dependencies") && code.ends_with(&dep_table_suffix) {
                return true;
            }
            in_dep_section = code.contains("dependencies") && !code.starts_with("[workspace.");
            continue;
        }
        if !in_dep_section {
            continue;
        }
        if let Some((lhs, rhs)) = code.split_once('=') {
            let key = lhs.trim();
            let base_key = key.strip_suffix(".workspace").map(str::trim).unwrap_or(key);
            if base_key == dep_name {
                return true;
            }
            // Check inline table rename `alias = { package = "dep_name", ... }`
            if rhs.contains("package") {
                for part in rhs.split(',') {
                    if let Some((k, v)) = part.split_once('=') {
                        let k_clean = k.trim().trim_start_matches('{').trim();
                        let v_clean = v.trim().trim_end_matches('}').trim().trim_matches('"').trim_matches('\'');
                        if k_clean == "package" && v_clean == dep_name {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn collect_rs_files(dir: &Path, acc: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, acc);
            } else if path.extension().map_or(false, |ext| ext == "rs") {
                acc.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_graph_building() {
        let mut graph = WorkspaceTopologyGraph::new();
        graph.add_crate("hadron-chamber", "crates/hadron-chamber");
        graph.add_crate("hadron-gluon", "crates/hadron-gluon");
        graph.add_dependency("hadron-chamber", "hadron-gluon");

        graph.add_quark("agy", "orchestrator", "excited");
        graph.add_worktree(".hadron/trees/cli-agy", "quark/cli-agy/01M01");
        graph.add_quark_worktree_link("agy", "quark/cli-agy/01M01");

        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.edges.len(), 2);
    }

    #[test]
    fn test_workspace_topology_from_workspace_and_layers() {
        let dir = tempfile::tempdir().unwrap();
        let graph = WorkspaceTopologyGraph::from_workspace(dir.path());
        assert!(!graph.nodes.is_empty());
        let layers = graph.compute_crate_layers();
        assert!(layers.contains_key("hadron-chamber"));
        assert!(layers.contains_key("hadron-forge"));
        assert!(*layers.get("hadron-chamber").unwrap() > *layers.get("hadron-gluon").unwrap());
    }

    #[test]
    fn test_cargo_toml_depends_on_avoids_prefix_false_positive_and_skips_vendor() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("Cargo.toml"),
            r#"[workspace]
members = ["crates/hadron-lattice", "crates/hadron-forge", "crates/hadron-chamber"]
"#,
        )
        .unwrap();

        let lattice_dir = root.join("crates/hadron-lattice");
        let forge_dir = root.join("crates/hadron-forge");
        let chamber_dir = root.join("crates/hadron-chamber");
        let vendor_dir = root.join("crates/gpui-component");

        fs::create_dir_all(&lattice_dir).unwrap();
        fs::create_dir_all(&forge_dir).unwrap();
        fs::create_dir_all(&chamber_dir).unwrap();
        fs::create_dir_all(&vendor_dir).unwrap();

        fs::write(
            lattice_dir.join("Cargo.toml"),
            r#"[package]
name = "hadron-lattice"
description = "Core lattice for hadron swarm"
[dependencies]
serde = "1"
"#,
        )
        .unwrap();

        fs::write(
            forge_dir.join("Cargo.toml"),
            r#"[package]
name = "hadron-forge"
[dependencies]
hadron-lattice = { path = "../hadron-lattice" }
"#,
        )
        .unwrap();

        fs::write(
            chamber_dir.join("Cargo.toml"),
            r#"[package]
name = "hadron"
[[bin]]
name = "hadron-gluon"
[dependencies]
hadron-lattice = { path = "../hadron-lattice" }
hadron-forge.workspace = true
"#,
        )
        .unwrap();

        fs::write(
            vendor_dir.join("Cargo.toml"),
            r#"[package]
name = "gpui-component"
"#,
        )
        .unwrap();

        let graph = WorkspaceTopologyGraph::from_workspace(root);
        let crate_names: Vec<String> = graph
            .nodes
            .iter()
            .filter_map(|n| match n {
                TopologyNode::Crate { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(crate_names, vec!["hadron", "hadron-forge", "hadron-lattice"]);
        assert!(!crate_names.contains(&"gpui-component".to_string()));

        // Verify hadron-lattice and hadron-forge do NOT falsely depend on "hadron"
        assert!(!graph.edges.iter().any(|e| e.target_id == "hadron"));

        let layers = graph.compute_crate_layers();
        assert_eq!(layers.get("hadron-lattice"), Some(&0));
        assert_eq!(layers.get("hadron-forge"), Some(&1));
        assert_eq!(layers.get("hadron"), Some(&2));
    }
}

