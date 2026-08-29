#[derive(Debug, Clone)]
pub struct BakeOffCandidateResult {
    pub quark_id: String,
    pub lines_changed: usize,
    pub tests_added: usize,
    pub gate_passed: bool,
}

pub struct BakeOffManager {
    pub spec_id: String,
    candidates: Vec<BakeOffCandidateResult>,
}

impl BakeOffManager {
    pub fn new(spec_id: &str) -> Self {
        Self {
            spec_id: spec_id.to_string(),
            candidates: Vec::new(),
        }
    }

    pub fn record_result(&mut self, quark: &str, lines: usize, tests: usize, passed: bool) {
        self.candidates.push(BakeOffCandidateResult {
            quark_id: quark.to_string(),
            lines_changed: lines,
            tests_added: tests,
            gate_passed: passed,
        });
    }

    pub fn select_winner(&self) -> Option<BakeOffCandidateResult> {
        self.candidates
            .iter()
            .filter(|c| c.gate_passed)
            .min_by_key(|c| c.lines_changed)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bakeoff_candidate_ranking() {
        let mut manager = BakeOffManager::new("spec-123");
        manager.record_result("quark-alpha", 120, 10, true);
        manager.record_result("quark-beta", 250, 8, true);

        let winner = manager.select_winner().unwrap();
        assert_eq!(winner.quark_id, "quark-alpha");
    }
}
