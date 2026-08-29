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
}
