use std::collections::HashMap;

pub struct PreonRegistry {
    preons: HashMap<String, String>,
}

impl PreonRegistry {
    pub fn new() -> Self {
        Self {
            preons: HashMap::new(),
        }
    }

    pub fn register_preon(&mut self, name: &str, content: &str) {
        self.preons.insert(name.to_string(), content.to_string());
    }

    pub fn render_preon(&self, name: &str) -> Option<String> {
        self.preons
            .get(name)
            .map(|c| format!("# Capability Preon: {}\n\n{}", name, c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preon_loading_and_attachment() {
        let mut reg = PreonRegistry::new();
        reg.register_preon("security", "Enforce strict zero-trust checks on all IPC packets.");

        let prompt_chunk = reg.render_preon("security").unwrap();
        assert!(prompt_chunk.contains("zero-trust"));
    }
}
