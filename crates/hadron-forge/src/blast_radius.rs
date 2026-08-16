//! Pure logic for the `blast_radius` tool family.
//! Analyzes modified files and workspace dependency graphs to calculate
//! directly modified crates, downstream dependents, affected symbols, and impacted test suites.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use serde::{Deserialize, Serialize};

use crate::cargo_tree::{get_cargo_tree, CargoPackageInfo};
use crate::exec::{exec, Program, EXEC_DEADLINE};
use crate::file::{ForgeError, Root};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlastRadiusReport {
    pub changed_files: Vec<String>,
    pub direct_crates: Vec<String>,
    pub downstream_crates: Vec<String>,
    pub impacted_test_targets: Vec<String>,
    pub summary: String,
}

/// Compute affected crates and their reverse dependencies.
pub fn compute_impact(
    packages: &[CargoPackageInfo],
    changed_files: &[String],
    _root_path: &Path,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut direct_crates = BTreeSet::new();

    // Map changed files to crates based on path heuristic
    for file in changed_files {
        for pkg in packages {
            if !pkg.is_workspace_member {
                continue;
            }
            // Check if file starts with crate name or crates/<name>
            let crate_dir = format!("crates/{}", pkg.name);
            if file.starts_with(&crate_dir) || file.starts_with(&pkg.name) {
                direct_crates.insert(pkg.name.clone());
            } else if file.starts_with("src/") && packages.len() == 1 {
                direct_crates.insert(pkg.name.clone());
            }
        }
    }

    // If direct_crates is empty but files are in root, assume all workspace members or primary
    if direct_crates.is_empty() && !changed_files.is_empty() {
        for file in changed_files {
            if file.ends_with("Cargo.toml") || file.ends_with("Cargo.lock") {
                for pkg in packages {
                    if pkg.is_workspace_member {
                        direct_crates.insert(pkg.name.clone());
                    }
                }
                break;
            }
        }
    }

    // Build reverse dependency graph
    // pkg_name -> list of packages that depend on pkg_name
    let mut reverse_deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pkg in packages {
        for dep in &pkg.dependencies {
            reverse_deps
                .entry(dep.name.clone())
                .or_default()
                .push(pkg.name.clone());
        }
    }

    // BFS to find all downstream crates
    let mut downstream = BTreeSet::new();
    let mut queue: Vec<String> = direct_crates.iter().cloned().collect();

    while let Some(current) = queue.pop() {
        if let Some(dependents) = reverse_deps.get(&current) {
            for dep in dependents {
                if !direct_crates.contains(dep) && downstream.insert(dep.clone()) {
                    queue.push(dep.clone());
                }
            }
        }
    }

    (direct_crates, downstream)
}

/// Query git for modified files since a given ref (defaults to HEAD~1 or HEAD).
pub fn get_changed_files(
    root: &Root,
    since_ref: Option<&str>,
) -> Result<Vec<String>, ForgeError> {
    let base_ref = since_ref.unwrap_or("HEAD~1");
    let args = vec![
        "diff".to_string(),
        "--name-only".to_string(),
        base_ref.to_string(),
    ];
    let exec_res = exec(root, Program::Git, &args, EXEC_DEADLINE);
    match exec_res {
        Ok(out) => {
            let files: Vec<String> = out
                .stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            Ok(files)
        }
        Err(_) => {
            // Fallback to git status --porcelain
            let status_args = vec!["status".to_string(), "--porcelain".to_string()];
            let status_res = exec(root, Program::Git, &status_args, EXEC_DEADLINE)?;
            let mut files = Vec::new();
            for line in status_res.stdout.lines() {
                let trimmed = line.trim();
                if trimmed.len() > 3 {
                    files.push(trimmed[3..].trim().to_string());
                }
            }
            Ok(files)
        }
    }
}

/// Analyze blast radius for given files or git diff.
pub fn analyze_blast_radius(
    root: &Root,
    since_ref: Option<&str>,
    explicit_files: Option<Vec<String>>,
) -> Result<BlastRadiusReport, ForgeError> {
    let changed_files = match explicit_files {
        Some(files) if !files.is_empty() => files,
        _ => get_changed_files(root, since_ref)?,
    };

    let packages = get_cargo_tree(root, None).unwrap_or_default();
    let (direct_set, downstream_set) = compute_impact(&packages, &changed_files, root.path());

    let direct_crates: Vec<String> = direct_set.into_iter().collect();
    let downstream_crates: Vec<String> = downstream_set.into_iter().collect();

    let mut impacted_test_targets = Vec::new();
    for c in &direct_crates {
        impacted_test_targets.push(format!("cargo test -p {}", c));
    }
    for c in &downstream_crates {
        impacted_test_targets.push(format!("cargo test -p {}", c));
    }

    let summary = format!(
        "Blast Radius: {} modified files impacting {} direct crate(s) and {} downstream dependent(s). Recommended gate checks: {}",
        changed_files.len(),
        direct_crates.len(),
        downstream_crates.len(),
        if impacted_test_targets.is_empty() {
            "cargo test --workspace".to_string()
        } else {
            impacted_test_targets.join(", ")
        }
    );

    Ok(BlastRadiusReport {
        changed_files,
        direct_crates,
        downstream_crates,
        impacted_test_targets,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cargo_tree::CargoDependencyInfo;

    #[test]
    fn compute_impact_identifies_direct_and_downstream() {
        let pkgs = vec![
            CargoPackageInfo {
                name: "core".to_string(),
                version: "0.1.0".to_string(),
                is_workspace_member: true,
                dependencies: vec![],
                features: vec![],
            },
            CargoPackageInfo {
                name: "service".to_string(),
                version: "0.1.0".to_string(),
                is_workspace_member: true,
                dependencies: vec![CargoDependencyInfo {
                    name: "core".to_string(),
                    req: "0.1.0".to_string(),
                    kind: None,
                    optional: false,
                }],
                features: vec![],
            },
            CargoPackageInfo {
                name: "app".to_string(),
                version: "0.1.0".to_string(),
                is_workspace_member: true,
                dependencies: vec![CargoDependencyInfo {
                    name: "service".to_string(),
                    req: "0.1.0".to_string(),
                    kind: None,
                    optional: false,
                }],
                features: vec![],
            },
        ];

        let changed = vec!["crates/core/src/lib.rs".to_string()];
        let (direct, downstream) = compute_impact(&pkgs, &changed, Path::new("/dummy"));

        assert_eq!(direct.into_iter().collect::<Vec<_>>(), vec!["core"]);
        let down_vec: Vec<String> = downstream.into_iter().collect();
        assert!(down_vec.contains(&"service".to_string()));
        assert!(down_vec.contains(&"app".to_string()));
    }
}
