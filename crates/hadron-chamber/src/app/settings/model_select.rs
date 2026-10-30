//! Searchable model selector delegate backing the native `gpui_component::select::SelectState`
//! dropdown component across Settings and the Add-Quark wizard.

use gpui::SharedString;
use gpui_component::select::{SelectItem, SearchableVec};

#[derive(Clone, Debug, PartialEq)]
pub struct ModelItem {
    pub label: SharedString,
    pub value: SharedString,
}

impl SelectItem for ModelItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }

    fn matches(&self, query: &str) -> bool {
        self.label.to_lowercase().contains(&query.to_lowercase())
            || self.value.to_lowercase().contains(&query.to_lowercase())
    }
}

pub type ModelSelectDelegate = SearchableVec<ModelItem>;

pub fn create_model_delegate(default_label: &str, models: &[String], selected: Option<&str>) -> ModelSelectDelegate {
    let mut items = Vec::new();
    items.push(ModelItem {
        label: format!("{default_label} (inherit)").into(),
        value: "".into(),
    });
    for m in models {
        let m_str = m.trim();
        if m_str.is_empty() {
            continue;
        }
        if !items.iter().any(|i| i.value == m_str) {
            items.push(ModelItem {
                label: m_str.to_string().into(),
                value: m_str.to_string().into(),
            });
        }
    }
    if let Some(sel) = selected {
        let sel_str = sel.trim();
        if !sel_str.is_empty() && !items.iter().any(|i| i.value == sel_str) {
            items.push(ModelItem {
                label: sel_str.to_string().into(),
                value: sel_str.to_string().into(),
            });
        }
    }
    SearchableVec::new(items)
}

/// Curated fallback models for known vendors so any seat (including CLI seats without
/// a live model probe or ACP seats whose background probe is still in flight) always
/// offers a rich, searchable list of models in Settings and the Quark Info panel.
pub fn vendor_fallback_models(vendor: &str) -> Vec<String> {
    let v = vendor.trim().to_ascii_lowercase();
    if v.contains("agy") || v.contains("gemini") || v.contains("google") {
        vec![
            "gemini-4-argon-medium".to_string(),
            "gemini-4-argon-high".to_string(),
            "gemini-4-argon-low".to_string(),
            "gemini-3.8-flash-high".to_string(),
            "gemini-3.8-flash-medium".to_string(),
            "gemini-3.8-flash-low".to_string(),
            "gemini-3.8-flash".to_string(),
            "gemini-3.6-flash".to_string(),
            "gemini-3.5-flash".to_string(),
            "gemini-3.1-pro".to_string(),
            "gemini-2.5-pro".to_string(),
            "gemini-2.5-flash".to_string(),
        ]
    } else if v.contains("claude") || v.contains("anthropic") {
        vec![
            "claude-fable-5".to_string(),
            "claude-opus-4-8".to_string(),
            "claude-opus-4-7".to_string(),
            "claude-sonnet-5".to_string(),
            "claude-sonnet-4-5".to_string(),
            "claude-haiku-4-5".to_string(),
            "opus".to_string(),
            "sonnet".to_string(),
            "haiku".to_string(),
        ]
    } else if v.contains("codex") || v.contains("openai") {
        vec![
            "gpt-5.6-sol".to_string(),
            "gpt-5.6-terra".to_string(),
            "gpt-5.6-luna".to_string(),
            "gpt-5.5".to_string(),
            "gpt-5.4-mini".to_string(),
            "gpt-5.2".to_string(),
            "gpt-5".to_string(),
            "o3".to_string(),
            "o4-mini".to_string(),
            "gpt-4.1".to_string(),
            "gpt-4o".to_string(),
            "gpt-4o-mini".to_string(),
        ]
    } else if v.contains("grok") || v.contains("xai") {
        vec![
            "grok-4".to_string(),
            "grok-3".to_string(),
            "grok-3-mini".to_string(),
        ]
    } else if v.contains("copilot") || v.contains("github") {
        vec![
            "claude-sonnet-5".to_string(),
            "claude-opus-4.8".to_string(),
            "claude-fable-5".to_string(),
            "gpt-5.6-sol".to_string(),
            "gpt-5.6-terra".to_string(),
            "gpt-5.5".to_string(),
            "gpt-5.4".to_string(),
            "gpt-5.3-codex".to_string(),
            "claude-sonnet-4.5".to_string(),
            "claude-haiku-4.5".to_string(),
            "gpt-5".to_string(),
            "gpt-4o".to_string(),
            "o3-mini".to_string(),
        ]
    } else if v.contains("mistral") {
        vec![
            "mistral-large-latest".to_string(),
            "codestral-latest".to_string(),
            "ministral-8b-latest".to_string(),
        ]
    } else if v.contains("deepseek") {
        vec![
            "deepseek-chat".to_string(),
            "deepseek-reasoner".to_string(),
        ]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::select::SelectDelegate;

    #[test]
    fn vendor_fallback_models_covers_core_vendors() {
        assert!(vendor_fallback_models("agy").contains(&"gemini-4-argon-medium".to_string()));
        assert!(vendor_fallback_models("agy").contains(&"gemini-3.8-flash".to_string()));
        assert!(!vendor_fallback_models("agy").contains(&"gemini-3.8-pro".to_string()));
        assert!(vendor_fallback_models("claude").contains(&"claude-opus-4-8".to_string()));
        assert!(vendor_fallback_models("claude").contains(&"claude-sonnet-5".to_string()));
        assert!(vendor_fallback_models("claude").contains(&"claude-fable-5".to_string()));
        assert!(vendor_fallback_models("codex").contains(&"gpt-5.6-terra".to_string()));
        assert!(vendor_fallback_models("codex").contains(&"gpt-5.6-sol".to_string()));
        assert!(vendor_fallback_models("grok").contains(&"grok-4".to_string()));
        assert!(vendor_fallback_models("copilot").contains(&"gpt-5.6-sol".to_string()));
        assert!(vendor_fallback_models("copilot").contains(&"claude-sonnet-5".to_string()));
        assert!(vendor_fallback_models("unknown-vendor").is_empty());
    }

    #[test]
    fn create_model_delegate_includes_inherit_and_deduplicates_selected() {
        let models = vec!["gpt-5".to_string(), "o3".to_string()];
        let delegate = create_model_delegate("Inherit", &models, Some("custom-model"));
        assert_eq!(delegate.items_count(0), 4);
    }
}

