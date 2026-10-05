use std::collections::{HashMap, HashSet};

pub struct AstBlastRadiusAnalyzer {
    symbol_to_tests: HashMap<String, HashSet<String>>,
}

impl AstBlastRadiusAnalyzer {
    pub fn new() -> Self {
        Self {
            symbol_to_tests: HashMap::new(),
        }
    }

    pub fn register_caller(&mut self, test_name: &str, symbol_name: &str) {
        self.symbol_to_tests
            .entry(symbol_name.to_string())
            .or_default()
            .insert(test_name.to_string());
    }

    pub fn find_impacted_tests(&self, changed_symbols: &[&str]) -> Vec<String> {
        let mut res = HashSet::new();
        for sym in changed_symbols {
            if let Some(tests) = self.symbol_to_tests.get(*sym) {
                res.extend(tests.clone());
            }
        }
        let mut list: Vec<String> = res.into_iter().collect();
        list.sort();
        list
    }

    pub fn find_affected_crates(changed_files: &[&str]) -> AffectedCratesResult {
        let mut crates = HashSet::new();
        for file in changed_files {
            let normalized = file.trim_start_matches("./");
            if normalized == "Cargo.toml"
                || normalized == "Cargo.lock"
                || normalized.starts_with(".cargo")
            {
                return AffectedCratesResult::WorkspaceWide(format!(
                    "Root build file changed: {normalized}"
                ));
            }
            if let Some(rest) = normalized.strip_prefix("crates/") {
                if let Some(crate_name) = rest.split('/').next() {
                    // crates/hadron-chamber declares package "hadron" in Cargo.toml
                    let pkg_name = if crate_name == "hadron-chamber" {
                        "hadron"
                    } else {
                        crate_name
                    };
                    crates.insert(pkg_name.to_string());
                    continue;
                }
            }
            // Any change outside crates/ (or top-level file) triggers full workspace test
            return AffectedCratesResult::WorkspaceWide(format!(
                "Non-crate file changed: {normalized}"
            ));
        }
        if crates.is_empty() {
            AffectedCratesResult::WorkspaceWide("No specific crate changes detected".to_string())
        } else {
            let mut list: Vec<String> = crates.into_iter().collect();
            list.sort();
            AffectedCratesResult::Specific(list)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AffectedCratesResult {
    Specific(Vec<String>),
    WorkspaceWide(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blast_radius_slicing() {
        let mut analyzer = AstBlastRadiusAnalyzer::new();
        analyzer.register_caller("test_port_mesh", "PortMesh::allocate");
        analyzer.register_caller("test_vcr_record", "VcrTape::record");

        let impacted = analyzer.find_impacted_tests(&["PortMesh::allocate"]);
        assert_eq!(impacted, vec!["test_port_mesh".to_string()]);
    }

    #[test]
    fn test_affected_crates_detection() {
        let changed_single = vec!["crates/hadron-chamber/src/main.rs"];
        let res = AstBlastRadiusAnalyzer::find_affected_crates(&changed_single);
        assert_eq!(
            res,
            AffectedCratesResult::Specific(vec!["hadron".to_string()])
        );

        let changed_forge = vec!["crates/hadron-forge/src/lib.rs"];
        let res_forge = AstBlastRadiusAnalyzer::find_affected_crates(&changed_forge);
        assert_eq!(
            res_forge,
            AffectedCratesResult::Specific(vec!["hadron-forge".to_string()])
        );

        let changed_root = vec!["Cargo.toml", "crates/hadron-chamber/src/main.rs"];
        let res_root = AstBlastRadiusAnalyzer::find_affected_crates(&changed_root);
        assert!(matches!(res_root, AffectedCratesResult::WorkspaceWide(_)));
    }
}
