//! Bespoke Quark Persona & Theme Customizer.
//!
//! Provides visual ownership over individual quark presence in Chamber:
//! custom accent colors, avatar glyphs, badge labels, and sound theme overrides.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarkPersonaTheme {
    pub quark_id: String,
    pub accent_hex: String,
    pub avatar_glyph: String,
    pub sound_theme_override: Option<String>,
    pub badge_label: Option<String>,
}

#[allow(dead_code)]
impl QuarkPersonaTheme {
    pub fn new(quark_id: impl Into<String>, accent_hex: impl Into<String>, avatar_glyph: impl Into<String>) -> Self {
        Self {
            quark_id: quark_id.into(),
            accent_hex: accent_hex.into(),
            avatar_glyph: avatar_glyph.into(),
            sound_theme_override: None,
            badge_label: None,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct QuarkThemeRegistry {
    themes: HashMap<String, QuarkPersonaTheme>,
}

#[allow(dead_code)]
impl QuarkThemeRegistry {
    pub fn new() -> Self {
        Self {
            themes: HashMap::new(),
        }
    }

    pub fn get(&self, quark_id: &str) -> Option<&QuarkPersonaTheme> {
        self.themes.get(quark_id)
    }

    pub fn set(&mut self, theme: QuarkPersonaTheme) {
        self.themes.insert(theme.quark_id.clone(), theme);
    }

    pub fn remove(&mut self, quark_id: &str) -> Option<QuarkPersonaTheme> {
        self.themes.remove(quark_id)
    }

    pub fn all(&self) -> Vec<&QuarkPersonaTheme> {
        let mut list: Vec<&QuarkPersonaTheme> = self.themes.values().collect();
        list.sort_by_key(|t| &t.quark_id);
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quark_theme_registry() {
        let mut registry = QuarkThemeRegistry::new();
        let mut agy_theme = QuarkPersonaTheme::new("agy", "#4285F4", "✦");
        agy_theme.badge_label = Some("Orchestrator".into());
        agy_theme.sound_theme_override = Some("synth".into());

        registry.set(agy_theme);

        let retrieved = registry.get("agy").unwrap();
        assert_eq!(retrieved.accent_hex, "#4285F4");
        assert_eq!(retrieved.avatar_glyph, "✦");
        assert_eq!(retrieved.badge_label.as_deref(), Some("Orchestrator"));

        assert_eq!(registry.all().len(), 1);

        let removed = registry.remove("agy");
        assert!(removed.is_some());
        assert!(registry.get("agy").is_none());
    }
}
