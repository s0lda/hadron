//! Dynamic Capability Preons & Seat Bindings.
//!
//! Provides in-memory registry, seat bindings, and dynamic prompt synthesis
//! for runtime-injectable capability preons.

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

/// Metadata associated with a capability preon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreonMetadata {
    pub name: String,
    pub preferred_role: Option<String>,
    pub capabilities: Vec<String>,
    pub description: Option<String>,
}

/// A fully defined capability preon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreonDefinition {
    pub name: String,
    pub preferred_role: Option<String>,
    pub body: String,
    pub metadata: PreonMetadata,
}

/// Registry of capability preons with dynamic per-seat bindings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreonRegistry {
    preons: HashMap<String, PreonDefinition>,
    seat_bindings: HashMap<String, HashSet<String>>,
}

impl PreonRegistry {
    pub fn new() -> Self {
        Self {
            preons: HashMap::new(),
            seat_bindings: HashMap::new(),
        }
    }

    /// Register or update a capability preon in the registry.
    pub fn register_preon(&mut self, name: &str, content: &str, preferred_role: Option<&str>) {
        let meta = PreonMetadata {
            name: name.to_string(),
            preferred_role: preferred_role.map(str::to_string),
            capabilities: Vec::new(),
            description: None,
        };

        self.preons.insert(
            name.to_string(),
            PreonDefinition {
                name: name.to_string(),
                preferred_role: preferred_role.map(str::to_string),
                body: content.to_string(),
                metadata: meta,
            },
        );
    }

    /// Remove a preon from the registry and unbind from all seats.
    pub fn unregister_preon(&mut self, name: &str) -> bool {
        let removed = self.preons.remove(name).is_some();
        if removed {
            for bindings in self.seat_bindings.values_mut() {
                bindings.remove(name);
            }
        }
        removed
    }

    /// Dynamically attach a preon to a specific quark seat at runtime.
    pub fn attach_to_seat(&mut self, quark_id: &str, preon_name: &str) -> Result<(), String> {
        if !self.preons.contains_key(preon_name) {
            return Err(format!("Preon '{}' is not registered", preon_name));
        }

        self.seat_bindings
            .entry(quark_id.to_string())
            .or_default()
            .insert(preon_name.to_string());

        Ok(())
    }

    /// Detach a preon from a specific quark seat.
    pub fn detach_from_seat(&mut self, quark_id: &str, preon_name: &str) -> bool {
        if let Some(bindings) = self.seat_bindings.get_mut(quark_id) {
            bindings.remove(preon_name)
        } else {
            false
        }
    }

    /// List all preons currently attached to a quark seat.
    pub fn get_seat_preons(&self, quark_id: &str) -> Vec<&PreonDefinition> {
        if let Some(names) = self.seat_bindings.get(quark_id) {
            let mut list: Vec<&PreonDefinition> = names
                .iter()
                .filter_map(|name| self.preons.get(name))
                .collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            list
        } else {
            Vec::new()
        }
    }

    /// Render a single preon's markdown body.
    pub fn render_preon(&self, name: &str) -> Option<String> {
        self.preons
            .get(name)
            .map(|p| format!("# Capability Preon: {}\n\n{}", p.name, p.body))
    }

    /// Compose the full runtime prompt for a quark seat, injecting all attached preons.
    pub fn render_seat_prompt(&self, quark_id: &str, base_prompt: &str) -> String {
        let attached = self.get_seat_preons(quark_id);
        if attached.is_empty() {
            return base_prompt.to_string();
        }

        let mut out = String::new();
        out.push_str(base_prompt);
        out.push_str("\n\n# Dynamically Injected Capability Preons\n\n");

        for preon in attached {
            out.push_str(&format!("## Preon: `{}`\n", preon.name));
            if let Some(ref role) = preon.preferred_role {
                out.push_str(&format!("**Specialized Role**: `{}`\n", role));
            }
            out.push_str(&preon.body);
            out.push_str("\n\n");
        }

        out
    }

    /// List all registered preons.
    pub fn list_preons(&self) -> Vec<&PreonDefinition> {
        let mut list: Vec<&PreonDefinition> = self.preons.values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preon_registration_attachment_and_prompt_synthesis() {
        let mut reg = PreonRegistry::new();
        reg.register_preon(
            "security-guard",
            "Enforce zero-trust checks on all IPC and network boundaries.",
            Some("SecurityAuditor"),
        );
        reg.register_preon(
            "ast-specialist",
            "Use syn structural AST matching and verify character boundaries.",
            Some("AstRefactorer"),
        );

        // Attach to worker-1
        reg.attach_to_seat("worker-1", "security-guard").unwrap();
        reg.attach_to_seat("worker-1", "ast-specialist").unwrap();

        let bound = reg.get_seat_preons("worker-1");
        assert_eq!(bound.len(), 2);
        assert_eq!(bound[0].name, "ast-specialist");
        assert_eq!(bound[1].name, "security-guard");

        let base_prompt = "You are worker-1 solving ticket #42.";
        let compiled = reg.render_seat_prompt("worker-1", base_prompt);
        assert!(compiled.contains("Dynamically Injected Capability Preons"));
        assert!(compiled.contains("zero-trust"));
        assert!(compiled.contains("syn structural AST matching"));

        // Detach
        assert!(reg.detach_from_seat("worker-1", "security-guard"));
        let bound_after = reg.get_seat_preons("worker-1");
        assert_eq!(bound_after.len(), 1);
        assert_eq!(bound_after[0].name, "ast-specialist");
    }

    #[test]
    fn test_unregister_cleans_up_seat_bindings() {
        let mut reg = PreonRegistry::new();
        reg.register_preon("temp-preon", "Temporary instructions.", None);
        reg.attach_to_seat("worker-2", "temp-preon").unwrap();

        assert_eq!(reg.get_seat_preons("worker-2").len(), 1);

        assert!(reg.unregister_preon("temp-preon"));
        assert_eq!(reg.get_seat_preons("worker-2").len(), 0);
    }
}
