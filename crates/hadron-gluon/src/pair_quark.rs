//! Dual-Cursor Pair-Quarking Mode.
//!
//! Synchronizes state between a Driver quark (authoring implementation) and a Navigator
//! quark (authoring tests & edge-case review) concurrently collaborating inside a shared worktree.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairRole {
    Driver,
    Navigator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairQuarkSession {
    pub session_id: String,
    pub driver_quark: String,
    pub navigator_quark: String,
    pub shared_file_focus: Option<PathBuf>,
    pub focus_line: usize,
}

impl PairQuarkSession {
    pub fn new(session_id: impl Into<String>, driver: impl Into<String>, navigator: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            driver_quark: driver.into(),
            navigator_quark: navigator.into(),
            shared_file_focus: None,
            focus_line: 0,
        }
    }

    /// Sets the collaborative editor focus and cursor line.
    pub fn set_focus(&mut self, file_path: impl AsRef<Path>, line: usize) {
        self.shared_file_focus = Some(file_path.as_ref().to_path_buf());
        self.focus_line = line;
    }

    /// Swaps the Driver and Navigator roles between the two collaborating quarks.
    pub fn swap_roles(&mut self) {
        std::mem::swap(&mut self.driver_quark, &mut self.navigator_quark);
    }

    /// Checks the role of a given quark in this session.
    pub fn role_of(&self, quark_name: &str) -> Option<PairRole> {
        if self.driver_quark == quark_name {
            Some(PairRole::Driver)
        } else if self.navigator_quark == quark_name {
            Some(PairRole::Navigator)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pair_quark_session_roles_and_swap() {
        let mut session = PairQuarkSession::new("pair-42", "agy", "reviewer");
        assert_eq!(session.role_of("agy"), Some(PairRole::Driver));
        assert_eq!(session.role_of("reviewer"), Some(PairRole::Navigator));
        assert_eq!(session.role_of("random"), None);

        session.set_focus("crates/hadron-gluon/src/engine.rs", 120);
        assert_eq!(
            session.shared_file_focus.as_ref().unwrap(),
            Path::new("crates/hadron-gluon/src/engine.rs")
        );
        assert_eq!(session.focus_line, 120);

        session.swap_roles();
        assert_eq!(session.role_of("reviewer"), Some(PairRole::Driver));
        assert_eq!(session.role_of("agy"), Some(PairRole::Navigator));
    }
}
