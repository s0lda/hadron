//! Ephemeral Preon synthesis for specialized ad-hoc subtasks.
//!
//! Generates targeted preon specifications on the fly when the orchestrator
//! or router needs a specialist role (e.g. Vulkan shader debugger, eBPF profiler,
//! AST refactor specialist) without requiring pre-authored static markdown files.

use crate::preons::Preon;

/// Synthesizes an ephemeral [`Preon`] tailored to `role` and `task`.
pub fn synthesize_ephemeral_preon(role: &str, task: &str) -> Preon {
    let mut body = String::new();
    body.push_str(&format!("# Specialist Role: {}\n\n", role));
    body.push_str(&format!("You are operating as the specialist `{}` for the following targeted assignment:\n\n", role));
    body.push_str(&format!("**Goal**: {}\n\n", task));
    body.push_str("## Invariants & Focus Areas\n");

    let task_lower = task.to_ascii_lowercase();
    let role_lower = role.to_ascii_lowercase();

    if role_lower.contains("vulkan") || task_lower.contains("vulkan") || task_lower.contains("lavapipe") || task_lower.contains("gpu") {
        body.push_str("- Enforce Vulkan / Lavapipe software rasterization constraints.\n");
        body.push_str("- Check memory barriers and pipeline execution synchronization.\n");
    }
    if role_lower.contains("security") || task_lower.contains("security") || task_lower.contains("auth") {
        body.push_str("- Strict threat boundary analysis and input validation.\n");
        body.push_str("- Enforce least privilege and defense in depth.\n");
    }
    if role_lower.contains("ast") || task_lower.contains("ast") || task_lower.contains("syn") || task_lower.contains("merge") {
        body.push_str("- Structural syntax tree validation and item-level reconciliation.\n");
        body.push_str("- Guarantee no character boundary slicing or macro expansion corruption.\n");
    }
    if role_lower.contains("perf") || task_lower.contains("perf") || task_lower.contains("profile") || task_lower.contains("ebpf") {
        body.push_str("- Zero unnecessary heap allocations and lock contention analysis.\n");
        body.push_str("- Evidence-based CPU and memory profiling.\n");
    }

    body.push_str("- Follow Standard Model Invariants: Evidence before adjectives, SSOT, and verify with actual execution.\n");

    Preon {
        name: role.to_string(),
        preferred_role: Some(role.to_string()),
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesizes_ephemeral_preon_with_targeted_rules() {
        let preon = synthesize_ephemeral_preon("VulkanDebugger", "Trace lavapipe pipeline barrier failure");
        assert_eq!(preon.name, "VulkanDebugger");
        assert_eq!(preon.preferred_role.as_deref(), Some("VulkanDebugger"));
        assert!(preon.body.contains("Vulkan"));
        assert!(preon.body.contains("Lavapipe"));
    }

    #[test]
    fn synthesizes_security_specialist_preon() {
        let preon = synthesize_ephemeral_preon("SecurityAuditor", "Audit authentication endpoint permissions");
        assert!(preon.body.contains("SecurityAuditor"));
        assert!(preon.body.contains("least privilege"));
    }

    #[test]
    fn synthesizes_ast_specialist_preon() {
        let preon = synthesize_ephemeral_preon("AstMergeResolver", "Reconcile syn AST conflicts");
        assert!(preon.body.contains("syntax tree"));
    }
}
