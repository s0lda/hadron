use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ulid::Ulid;

/// Breakpoint condition matching criteria.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakpointCondition {
    pub tool_name_pattern: String,
    pub argument_substring: Option<String>,
    pub hit_count_threshold: usize,
}

/// Action decided when execution hits an active breakpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterceptionResolution {
    /// Resume execution unmodified.
    Resume,
    /// Resume execution with modified JSON arguments.
    ModifyArguments { new_arguments_json: String },
    /// Cancel the tool invocation and return an error to the calling quark.
    Cancel { reason: String },
    /// Return a mock response directly without executing the tool.
    MockResponse { response_json: String },
}

/// State of an intercepted tool execution awaiting decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterceptedInvocation {
    pub interception_id: String,
    pub turn_ulid: Option<Ulid>,
    pub quark_id: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub timestamp_ms: u64,
}

/// Registry of tool execution breakpoints and pending interceptions.
#[derive(Debug, Clone, Default)]
pub struct BreakpointsRegistry {
    breakpoints: HashMap<String, BreakpointCondition>,
    hit_counts: HashMap<String, usize>,
    pending_interceptions: HashMap<String, InterceptedInvocation>,
}

impl BreakpointsRegistry {
    pub fn new() -> Self {
        Self {
            breakpoints: HashMap::new(),
            hit_counts: HashMap::new(),
            pending_interceptions: HashMap::new(),
        }
    }

    pub fn set_breakpoint(&mut self, id: String, condition: BreakpointCondition) {
        self.breakpoints.insert(id, condition);
    }

    pub fn remove_breakpoint(&mut self, id: &str) -> Option<BreakpointCondition> {
        self.hit_counts.remove(id);
        self.breakpoints.remove(id)
    }

    pub fn list_breakpoints(&self) -> HashMap<String, BreakpointCondition> {
        self.breakpoints.clone()
    }

    /// Check if a proposed tool invocation matches any active breakpoint.
    pub fn should_intercept(&mut self, tool_name: &str, arguments_json: &str) -> Option<String> {
        for (id, cond) in &self.breakpoints {
            let name_match = cond.tool_name_pattern == "*" || cond.tool_name_pattern == tool_name;
            let arg_match = match &cond.argument_substring {
                Some(sub) => arguments_json.contains(sub),
                None => true,
            };

            if name_match && arg_match {
                let count = self.hit_counts.entry(id.clone()).or_insert(0);
                *count += 1;
                if *count >= cond.hit_count_threshold {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    pub fn register_interception(&mut self, invocation: InterceptedInvocation) {
        self.pending_interceptions
            .insert(invocation.interception_id.clone(), invocation);
    }

    pub fn get_pending_interception(&self, id: &str) -> Option<&InterceptedInvocation> {
        self.pending_interceptions.get(id)
    }

    pub fn list_pending(&self) -> Vec<InterceptedInvocation> {
        self.pending_interceptions.values().cloned().collect()
    }

    pub fn resolve_interception(
        &mut self,
        id: &str,
    ) -> Option<InterceptedInvocation> {
        self.pending_interceptions.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_breakpoints_trigger_and_interception() {
        let mut registry = BreakpointsRegistry::new();
        registry.set_breakpoint(
            "bp-edit-cargo".into(),
            BreakpointCondition {
                tool_name_pattern: "edit".into(),
                argument_substring: Some("Cargo.toml".into()),
                hit_count_threshold: 1,
            },
        );

        // Does not match tool name
        assert!(registry.should_intercept("exec", "cargo check").is_none());

        // Does not match argument substring
        assert!(registry
            .should_intercept("edit", r#"{"file":"src/main.rs"}"#)
            .is_none());

        // Matches both
        let matched = registry.should_intercept("edit", r#"{"file":"Cargo.toml"}"#);
        assert_eq!(matched.as_deref(), Some("bp-edit-cargo"));

        let inv = InterceptedInvocation {
            interception_id: "int-1".into(),
            turn_ulid: None,
            quark_id: "agy".into(),
            tool_name: "edit".into(),
            arguments_json: r#"{"file":"Cargo.toml"}"#.into(),
            timestamp_ms: 1000,
        };

        registry.register_interception(inv.clone());
        assert_eq!(registry.list_pending().len(), 1);

        let resolved = registry.resolve_interception("int-1");
        assert_eq!(resolved.unwrap().interception_id, "int-1");
        assert_eq!(registry.list_pending().len(), 0);
    }
}
