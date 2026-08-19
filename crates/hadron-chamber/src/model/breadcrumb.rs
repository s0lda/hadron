//! Breadcrumb context model: parses features, invariants, and notes into HUD breadcrumbs.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreadcrumbKind {
    Feature,
    Invariant,
    Lesson,
    Plan,
}

impl BreadcrumbKind {
    pub fn label(self) -> &'static str {
        match self {
            BreadcrumbKind::Feature => "Feature",
            BreadcrumbKind::Invariant => "Invariant",
            BreadcrumbKind::Lesson => "Lesson",
            BreadcrumbKind::Plan => "Plan",
        }
    }

    pub fn icon_char(self) -> &'static str {
        match self {
            BreadcrumbKind::Feature => "⚡",
            BreadcrumbKind::Invariant => "🛡️",
            BreadcrumbKind::Lesson => "💡",
            BreadcrumbKind::Plan => "📋",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreadcrumbItem {
    pub kind: BreadcrumbKind,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BreadcrumbSummary {
    pub items: Vec<BreadcrumbItem>,
}

impl BreadcrumbSummary {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add_item(&mut self, kind: BreadcrumbKind, label: impl Into<String>, detail: Option<String>) {
        self.items.push(BreadcrumbItem {
            kind,
            label: label.into(),
            detail,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn format_hud(&self) -> String {
        if self.items.is_empty() {
            return String::new();
        }
        self.items
            .iter()
            .map(|item| {
                if let Some(detail) = &item.detail {
                    format!("{}: {} ({})", item.kind.label(), item.label, detail)
                } else {
                    format!("{}: {}", item.kind.label(), item.label)
                }
            })
            .collect::<Vec<_>>()
            .join(" › ")
    }

    /// Extract breadcrumb items from nucleus features, invariants, and active plan
    pub fn from_nucleus(
        features_table: Option<&str>,
        invariants_text: Option<&str>,
        active_plan: Option<&str>,
    ) -> Self {
        let mut summary = Self::new();

        if let Some(plan) = active_plan {
            let name = plan.rsplit('/').next().unwrap_or(plan);
            summary.add_item(BreadcrumbKind::Plan, name, None);
        }

        if let Some(feat_md) = features_table {
            for line in feat_md.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('|') && !trimmed.contains("---|---") && !trimmed.contains("Feature") {
                    let cols: Vec<&str> = trimmed.split('|').map(str::trim).filter(|s| !s.is_empty()).collect();
                    if cols.len() >= 3 {
                        let name = cols[0].trim_matches('*');
                        let status = cols[2];
                        if status.contains("Active") || status.contains("Refining") {
                            summary.add_item(BreadcrumbKind::Feature, name, Some(status.to_string()));
                            if summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Feature).count() >= 2 {
                                break;
                            }
                        }
                    }
                }
            }
        }

        if let Some(inv_md) = invariants_text {
            for line in inv_md.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("## ") {
                    summary.add_item(BreadcrumbKind::Invariant, rest, None);
                    if summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Invariant).count() >= 2 {
                        break;
                    }
                }
            }
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_breadcrumb_format() {
        let mut summary = BreadcrumbSummary::new();
        summary.add_item(BreadcrumbKind::Plan, "01-phase-1-nucleus-quick-dx.md", None);
        summary.add_item(BreadcrumbKind::Feature, "Chamber GUI", Some("Active".to_string()));
        summary.add_item(BreadcrumbKind::Invariant, "Vulkan / Lavapipe Software Fallback", None);

        assert_eq!(summary.len(), 3);
        assert_eq!(
            summary.format_hud(),
            "Plan: 01-phase-1-nucleus-quick-dx.md › Feature: Chamber GUI (Active) › Invariant: Vulkan / Lavapipe Software Fallback"
        );
    }

    #[test]
    fn test_breadcrumb_from_nucleus_parsing() {
        let features = r#"
| Feature | Description | Status | Entrypoint Files |
|---|---|---|---|
| **Chamber GUI** | Native GPUI desktop workspace | Active | `crates/hadron-chamber/src/app/mod.rs` |
| **Gluon Engine** | Swarm daemon | Active | `crates/hadron-gluon/src/engine/mod.rs` |
| **Old Feature** | Deprecated thing | Deprecated | `old.rs` |
"#;

        let invariants = r#"
# Invariants Registry

## GUI & Rendering Constraints
- Some detail

## IPC & Swarm Protocol
- Some protocol rule
"#;

        let summary = BreadcrumbSummary::from_nucleus(
            Some(features),
            Some(invariants),
            Some(".hadron/docs/plans/2026-08-19-twenty-capabilities/master.md"),
        );

        assert_eq!(summary.items[0].kind, BreadcrumbKind::Plan);
        assert_eq!(summary.items[0].label, "master.md");

        let feature_items: Vec<_> = summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Feature).collect();
        assert_eq!(feature_items.len(), 2);
        assert_eq!(feature_items[0].label, "Chamber GUI");
        assert_eq!(feature_items[1].label, "Gluon Engine");

        let invariant_items: Vec<_> = summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Invariant).collect();
        assert_eq!(invariant_items.len(), 2);
        assert_eq!(invariant_items[0].label, "GUI & Rendering Constraints");
        assert_eq!(invariant_items[1].label, "IPC & Swarm Protocol");
    }
}
