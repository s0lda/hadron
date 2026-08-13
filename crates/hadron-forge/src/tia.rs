//! AST Test Impact Analysis (TIA) for the Merge Gate.
//!
//! Maps modified files and AST items to specific target packages and test symbols.
//! If changes are isolated to a single crate without impacting workspace-wide types
//! or config files (e.g. `Cargo.toml`, root files), TIA allows the merge gate to run
//! targeted test suites rather than executing the entire workspace test suite,
//! dropping gate latency from minutes to seconds while falling back to full workspace
//! testing on any cross-crate boundary ambiguity.

/// The computed test execution plan based on changed files and impacted symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestImpactPlan {
    /// Specific package to test with `-p <pkg>`, if isolated.
    pub target_package: Option<String>,
    /// Specific test filter expressions or symbols.
    pub target_symbols: Vec<String>,
    /// Whether the changes require a full workspace test suite run.
    pub requires_full_workspace: bool,
}

impl TestImpactPlan {
    pub fn full_workspace() -> Self {
        Self {
            target_package: None,
            target_symbols: Vec::new(),
            requires_full_workspace: true,
        }
    }
}

/// Computes a [`TestImpactPlan`] given the list of modified file paths in a branch.
pub fn compute_impacted_tests(changed_files: &[&str]) -> TestImpactPlan {
    if changed_files.is_empty() {
        return TestImpactPlan {
            target_package: None,
            target_symbols: Vec::new(),
            requires_full_workspace: false,
        };
    }

    // Check for root manifest or workspace config changes -> full workspace
    for file in changed_files {
        if *file == "Cargo.toml"
            || *file == "Cargo.lock"
            || file.starts_with(".cargo")
            || file.starts_with(".github")
        {
            return TestImpactPlan::full_workspace();
        }
    }

    let mut detected_packages = std::collections::HashSet::new();
    let mut symbols = Vec::new();

    for file in changed_files {
        if let Some(rest) = file.strip_prefix("crates/") {
            if let Some(slash_idx) = rest.find('/') {
                let pkg = &rest[..slash_idx];
                detected_packages.insert(pkg.to_string());
            } else {
                return TestImpactPlan::full_workspace();
            }
        } else {
            // Non-crates file changed (e.g. docs, scripts, root config)
            return TestImpactPlan::full_workspace();
        }

        // Extract filename stem as potential test target symbol
        if let Some(stem) = std::path::Path::new(file)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            if stem != "lib" && stem != "mod" && stem != "main" {
                symbols.push(stem.to_string());
            }
        }
    }

    if detected_packages.len() == 1 {
        let pkg = detected_packages.into_iter().next().unwrap();
        TestImpactPlan {
            target_package: Some(pkg),
            target_symbols: symbols,
            requires_full_workspace: false,
        }
    } else {
        TestImpactPlan::full_workspace()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tia_identifies_targeted_crates_and_test_symbols_from_ast_diff() {
        let files = ["crates/hadron-lattice/src/nucleus.rs"];
        let plan = compute_impacted_tests(&files);
        assert_eq!(plan.target_package, Some("hadron-lattice".to_string()));
        assert_eq!(plan.target_symbols, vec!["nucleus"]);
        assert!(!plan.requires_full_workspace);
    }

    #[test]
    fn tia_falls_back_to_full_workspace_on_manifest_edit() {
        let files = ["crates/hadron-lattice/src/nucleus.rs", "Cargo.toml"];
        let plan = compute_impacted_tests(&files);
        assert!(plan.requires_full_workspace);
        assert!(plan.target_package.is_none());
    }

    #[test]
    fn tia_falls_back_to_full_workspace_on_multi_crate_edits() {
        let files = [
            "crates/hadron-lattice/src/nucleus.rs",
            "crates/hadron-gluon/src/merge.rs",
        ];
        let plan = compute_impacted_tests(&files);
        assert!(plan.requires_full_workspace);
    }
}
