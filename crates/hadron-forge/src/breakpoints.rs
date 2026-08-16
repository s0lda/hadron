use serde::{Deserialize, Serialize};

/// Breakpoint entry representation in hadron-forge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakpointEntry {
    pub id: String,
    pub tool_name: String,
    pub argument_filter: Option<String>,
    pub enabled: bool,
}

pub struct BreakpointsForge;

impl BreakpointsForge {
    pub fn matches_breakpoint(entry: &BreakpointEntry, tool_name: &str, args_json: &str) -> bool {
        if !entry.enabled {
            return false;
        }
        if entry.tool_name != "*" && entry.tool_name != tool_name {
            return false;
        }
        if let Some(ref filter) = entry.argument_filter {
            if !args_json.contains(filter) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_breakpoints_forge_matching() {
        let bp = BreakpointEntry {
            id: "bp-1".into(),
            tool_name: "edit".into(),
            argument_filter: Some("Cargo.toml".into()),
            enabled: true,
        };

        assert!(BreakpointsForge::matches_breakpoint(&bp, "edit", r#"{"file":"Cargo.toml"}"#));
        assert!(!BreakpointsForge::matches_breakpoint(&bp, "edit", r#"{"file":"main.rs"}"#));
        assert!(!BreakpointsForge::matches_breakpoint(&bp, "exec", r#"{"file":"Cargo.toml"}"#));
    }
}
