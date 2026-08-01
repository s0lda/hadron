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
