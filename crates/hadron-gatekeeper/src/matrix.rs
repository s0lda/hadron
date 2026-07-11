use serde::{Deserialize, Serialize};

/// The category of a proposed operation. An *input* to the matrix — Hadron does
/// not derive this from command text in the CLI-adapter architecture (a quark's
/// turn surfaces only as a message, not structured tool calls). A later slice
/// decides who supplies the `Risk`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Writing, editing, or deleting files inside the workspace.
    WorkspaceEdit,
    /// Executing a shell command (includes publish-class ops like `cargo publish`).
    BashExec,
}

/// The human's god-mode configuration: two independent bypass toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Level 1: auto-approve workspace edits without asking.
    pub auto_approve_edits: bool,
    /// Level 2: bypass all bash-execution prompts.
    pub bypass_bash: bool,
}

impl Policy {
    /// The safe default: nothing is bypassed; every risky op asks the human.
    pub fn locked_down() -> Self {
        Policy { auto_approve_edits: false, bypass_bash: false }
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy::locked_down()
    }
}

/// The matrix's verdict for a single proposed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// The policy pre-authorizes this class of op; proceed without a prompt.
    AutoApprove,
    /// Pause and surface a permission request to the human.
    AskHuman,
}

/// The bypass matrix: does `policy` pre-authorize an op of this `risk`?
pub fn decide(risk: Risk, policy: Policy) -> Decision {
    let bypassed = match risk {
        Risk::WorkspaceEdit => policy.auto_approve_edits,
        Risk::BashExec => policy.bypass_bash,
    };
    if bypassed {
        Decision::AutoApprove
    } else {
        Decision::AskHuman
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_down_asks_for_everything() {
        let p = Policy::locked_down();
        assert_eq!(decide(Risk::WorkspaceEdit, p), Decision::AskHuman);
        assert_eq!(decide(Risk::BashExec, p), Decision::AskHuman);
    }

    #[test]
    fn edit_toggle_only_bypasses_edits() {
        let p = Policy { auto_approve_edits: true, bypass_bash: false };
        assert_eq!(decide(Risk::WorkspaceEdit, p), Decision::AutoApprove);
        // Independent: bypassing edits must NOT bypass bash.
        assert_eq!(decide(Risk::BashExec, p), Decision::AskHuman);
    }

    #[test]
    fn bash_toggle_only_bypasses_bash() {
        let p = Policy { auto_approve_edits: false, bypass_bash: true };
        assert_eq!(decide(Risk::BashExec, p), Decision::AutoApprove);
        // Independent: bypassing bash must NOT bypass edits.
        assert_eq!(decide(Risk::WorkspaceEdit, p), Decision::AskHuman);
    }

    #[test]
    fn both_toggles_bypass_both() {
        let p = Policy { auto_approve_edits: true, bypass_bash: true };
        assert_eq!(decide(Risk::WorkspaceEdit, p), Decision::AutoApprove);
        assert_eq!(decide(Risk::BashExec, p), Decision::AutoApprove);
    }

    #[test]
    fn default_policy_is_locked_down() {
        assert_eq!(Policy::default(), Policy::locked_down());
    }
}
