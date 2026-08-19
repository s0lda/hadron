//! Breadcrumb context model: parses features, invariants, and notes into HUD breadcrumbs.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreadcrumbKind {
    Plan,
    File,
    Feature,
    Invariant,
    Lesson,
}

impl BreadcrumbKind {
    pub fn label(self) -> &'static str {
        match self {
            BreadcrumbKind::Plan => "Plan",
            BreadcrumbKind::File => "File",
            BreadcrumbKind::Feature => "Feature",
            BreadcrumbKind::Invariant => "Invariant",
            BreadcrumbKind::Lesson => "Lesson",
        }
    }

    pub fn icon_char(self) -> &'static str {
        match self {
            BreadcrumbKind::Plan => "📋",
            BreadcrumbKind::File => "📄",
            BreadcrumbKind::Feature => "⚡",
            BreadcrumbKind::Invariant => "🛡️",
            BreadcrumbKind::Lesson => "💡",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreadcrumbItem {
    pub kind: BreadcrumbKind,
    pub label: String,
    pub detail: Option<String>,
    pub target_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BreadcrumbSummary {
    pub items: Vec<BreadcrumbItem>,
}

impl BreadcrumbSummary {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add_item(
        &mut self,
        kind: BreadcrumbKind,
        label: impl Into<String>,
        detail: Option<String>,
        target_path: Option<String>,
    ) {
        self.items.push(BreadcrumbItem {
            kind,
            label: label.into(),
            detail,
            target_path,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
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

    /// Extract dynamic, contextual breadcrumb items from the currently active plan and its content.
    pub fn from_plan(
        plan_rel_path: &str,
        plan_content: &str,
        features_table: Option<&str>,
        invariants_text: Option<&str>,
        lessons_index: Option<&str>,
    ) -> Self {
        let mut summary = Self::new();

        // 1. Active Plan
        let plan_name = plan_rel_path.rsplit('/').next().unwrap_or(plan_rel_path);
        summary.add_item(
            BreadcrumbKind::Plan,
            plan_name,
            Some("Active Plan".to_string()),
            Some(plan_rel_path.to_string()),
        );

        // 2. Parent Master Plan or Sub-Plans
        if let Some((dir, file)) = plan_rel_path.rsplit_once('/') {
            if file != "master.md" && file != "index.md" {
                let master_path = format!("{dir}/master.md");
                summary.add_item(
                    BreadcrumbKind::Plan,
                    "master.md",
                    Some("Master Plan".to_string()),
                    Some(master_path),
                );
            }
        }

        if plan_name == "master.md" || plan_name == "index.md" {
            let dir = plan_rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            for line in plan_content.lines() {
                if let Some(start) = line.find("[`0") {
                    if let Some(end) = line[start..].find(".md`]") {
                        let sub_name = &line[start + 2..start + end + 3];
                        let sub_path = if dir.is_empty() {
                            sub_name.to_string()
                        } else {
                            format!("{dir}/{sub_name}")
                        };
                        summary.add_item(
                            BreadcrumbKind::Plan,
                            sub_name,
                            Some("Sub-Plan".to_string()),
                            Some(sub_path),
                        );
                        if summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Plan).count() >= 3 {
                            break;
                        }
                    }
                }
            }
        }

        // 3. Key target files mentioned in the plan
        let mut found_files = Vec::new();
        for line in plan_content.lines() {
            let trimmed = line.trim();
            for token in trimmed.split(&[' ', '`', '(', ')', '*', '"', '\'', ','][..]) {
                let clean = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '_' && c != '-' && c != '.');
                if (clean.starts_with("crates/") || clean.starts_with("src/"))
                    && (clean.ends_with(".rs") || clean.ends_with(".toml") || clean.ends_with(".md") || clean.ends_with(".json"))
                {
                    let path_str = clean.to_string();
                    if !found_files.contains(&path_str) {
                        found_files.push(path_str);
                    }
                }
            }
        }

        for path in found_files.into_iter().take(3) {
            let base = path.rsplit('/').next().unwrap_or(&path).to_string();
            summary.add_item(
                BreadcrumbKind::File,
                base,
                Some(path.clone()),
                Some(path),
            );
        }

        // 4. Plan-specific constraints & invariants
        let mut in_constraints = false;
        let mut found_constraints = 0;
        for line in plan_content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("##")
                && (trimmed.to_lowercase().contains("constraint") || trimmed.to_lowercase().contains("invariant"))
            {
                in_constraints = true;
                continue;
            } else if in_constraints && trimmed.starts_with("##") {
                in_constraints = false;
            }
            if in_constraints && (trimmed.starts_with("- ") || trimmed.starts_with("* ")) {
                let text = trimmed[2..].trim();
                let label = if let Some(bold) = text.strip_prefix("**") {
                    bold.split("**").next().unwrap_or(text).trim_end_matches(':').trim()
                } else {
                    text.split(':').next().unwrap_or(text).trim().trim_matches('*').trim()
                };
                if !label.is_empty() && label.len() < 50 {
                    summary.add_item(
                        BreadcrumbKind::Invariant,
                        label,
                        Some("Plan Invariant".to_string()),
                        Some(".hadron/nucleus/invariants/always.md".to_string()),
                    );
                    found_constraints += 1;
                    if found_constraints >= 2 {
                        break;
                    }
                }
            }
        }

        if found_constraints == 0 {
            if let Some(inv_md) = invariants_text {
                for line in inv_md.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("## ") {
                        summary.add_item(
                            BreadcrumbKind::Invariant,
                            rest,
                            Some("Nucleus Invariant".to_string()),
                            Some(".hadron/nucleus/invariants/always.md".to_string()),
                        );
                        break;
                    }
                }
            }
        }

        // 5. Matching Features
        if let Some(feat_md) = features_table {
            let mut found_feat = 0;
            for line in feat_md.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('|') && !trimmed.contains("---|---") && !trimmed.contains("Feature") {
                    let cols: Vec<&str> = trimmed.split('|').map(str::trim).filter(|s| !s.is_empty()).collect();
                    if cols.len() >= 4 {
                        let feat_name = cols[0].trim_matches('*');
                        let status = cols[2];
                        let entrypoint = cols[3].trim_matches('`').trim();
                        let search_term = feat_name.split('(').next().unwrap_or(feat_name).trim();
                        if plan_content.to_lowercase().contains(&search_term.to_lowercase()) {
                            let target = if entrypoint.is_empty() {
                                ".hadron/nucleus/features.md".to_string()
                            } else {
                                entrypoint.to_string()
                            };
                            summary.add_item(
                                BreadcrumbKind::Feature,
                                feat_name,
                                Some(status.to_string()),
                                Some(target),
                            );
                            found_feat += 1;
                            if found_feat >= 2 {
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 6. Matching Lessons
        if let Some(index_md) = lessons_index {
            for line in index_md.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("- [") {
                    if let Some(slug) = trimmed.split(']').next().and_then(|s| s.strip_prefix("- [")) {
                        if plan_content.contains(slug) {
                            summary.add_item(
                                BreadcrumbKind::Lesson,
                                slug,
                                Some("Nucleus Lesson".to_string()),
                                Some(format!(".hadron/nucleus/notes/{slug}.md")),
                            );
                            break;
                        }
                    }
                }
            }
        }

        summary
    }

    /// Extract breadcrumb items from nucleus features, invariants, lessons, and active plan
    pub fn from_nucleus(
        features_table: Option<&str>,
        invariants_text: Option<&str>,
        lessons_index: Option<&str>,
        active_plan: Option<&str>,
    ) -> Self {
        let mut summary = Self::new();

        if let Some(plan) = active_plan {
            let name = plan.rsplit('/').next().unwrap_or(plan);
            summary.add_item(
                BreadcrumbKind::Plan,
                name,
                Some("Active Plan".to_string()),
                Some(plan.to_string()),
            );
        }

        if let Some(feat_md) = features_table {
            for line in feat_md.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('|') && !trimmed.contains("---|---") && !trimmed.contains("Feature") {
                    let cols: Vec<&str> = trimmed.split('|').map(str::trim).filter(|s| !s.is_empty()).collect();
                    if cols.len() >= 4 {
                        let name = cols[0].trim_matches('*');
                        let status = cols[2];
                        let entrypoint = cols[3].trim_matches('`').trim();
                        let target = if entrypoint.is_empty() {
                            ".hadron/nucleus/features.md".to_string()
                        } else {
                            entrypoint.to_string()
                        };
                        if status.contains("Active") || status.contains("Refining") {
                            summary.add_item(
                                BreadcrumbKind::Feature,
                                name,
                                Some(status.to_string()),
                                Some(target),
                            );
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
                    summary.add_item(
                        BreadcrumbKind::Invariant,
                        rest,
                        Some("Nucleus Invariant".to_string()),
                        Some(".hadron/nucleus/invariants/always.md".to_string()),
                    );
                    if summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Invariant).count() >= 2 {
                        break;
                    }
                }
            }
        }

        if let Some(index_md) = lessons_index {
            for line in index_md.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("- [") {
                    if let Some(slug) = trimmed.split(']').next().and_then(|s| s.strip_prefix("- [")) {
                        summary.add_item(
                            BreadcrumbKind::Lesson,
                            slug,
                            Some("Nucleus Lesson".to_string()),
                            Some(format!(".hadron/nucleus/notes/{slug}.md")),
                        );
                        if summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Lesson).count() >= 1 {
                            break;
                        }
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
        summary.add_item(
            BreadcrumbKind::Plan,
            "01-phase-1-nucleus-quick-dx.md",
            None,
            Some(".hadron/docs/plans/01-phase-1-nucleus-quick-dx.md".to_string()),
        );
        summary.add_item(
            BreadcrumbKind::Feature,
            "Chamber GUI",
            Some("Active".to_string()),
            Some("crates/hadron-chamber/src/app/mod.rs".to_string()),
        );
        summary.add_item(
            BreadcrumbKind::Invariant,
            "Vulkan / Lavapipe Software Fallback",
            None,
            Some(".hadron/nucleus/invariants/always.md".to_string()),
        );
        summary.add_item(
            BreadcrumbKind::Lesson,
            "compiled-is-not-running",
            None,
            Some(".hadron/nucleus/notes/compiled-is-not-running.md".to_string()),
        );

        assert_eq!(summary.len(), 4);
        assert_eq!(
            summary.format_hud(),
            "Plan: 01-phase-1-nucleus-quick-dx.md › Feature: Chamber GUI (Active) › Invariant: Vulkan / Lavapipe Software Fallback › Lesson: compiled-is-not-running"
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

        let index = r#"
# Memory index
- [compiled-is-not-running](notes/compiled-is-not-running.md) — A patch that compiles is not a feature that runs
"#;

        let summary = BreadcrumbSummary::from_nucleus(
            Some(features),
            Some(invariants),
            Some(index),
            Some(".hadron/docs/plans/2026-08-19-twenty-capabilities/master.md"),
        );

        assert_eq!(summary.items[0].kind, BreadcrumbKind::Plan);
        assert_eq!(summary.items[0].label, "master.md");
        assert_eq!(summary.items[0].target_path.as_deref(), Some(".hadron/docs/plans/2026-08-19-twenty-capabilities/master.md"));

        let feature_items: Vec<_> = summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Feature).collect();
        assert_eq!(feature_items.len(), 2);
        assert_eq!(feature_items[0].label, "Chamber GUI");
        assert_eq!(feature_items[0].target_path.as_deref(), Some("crates/hadron-chamber/src/app/mod.rs"));
        assert_eq!(feature_items[1].label, "Gluon Engine");
        assert_eq!(feature_items[1].target_path.as_deref(), Some("crates/hadron-gluon/src/engine/mod.rs"));

        let invariant_items: Vec<_> = summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Invariant).collect();
        assert_eq!(invariant_items.len(), 2);
        assert_eq!(invariant_items[0].label, "GUI & Rendering Constraints");
        assert_eq!(invariant_items[1].label, "IPC & Swarm Protocol");

        let lesson_items: Vec<_> = summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Lesson).collect();
        assert_eq!(lesson_items.len(), 1);
        assert_eq!(lesson_items[0].label, "compiled-is-not-running");
        assert_eq!(lesson_items[0].target_path.as_deref(), Some(".hadron/nucleus/notes/compiled-is-not-running.md"));
    }

    #[test]
    fn test_breadcrumb_from_plan_parsing() {
        let plan_text = r#"
# Phase 1: Nucleus & Quick DX Plan

## Global Constraints
- **Lavapipe Software Fallback**: GPUI rasterizes in CPU software.
- **NDJSON Message Framing**: IPC uses newline-delimited JSON.

### Task 1.1: Context Breadcrumb Bar
**Files:**
- Modify: `crates/hadron-chamber/src/app/render/breadcrumb.rs`
- Create: `crates/hadron-lattice/src/promoter.rs`
"#;

        let features = r#"
| Feature | Description | Status | Entrypoint Files |
|---|---|---|---|
| **Chamber GUI** | Native GPUI desktop workspace | Active | `crates/hadron-chamber/src/app/mod.rs` |
"#;

        let summary = BreadcrumbSummary::from_plan(
            ".hadron/docs/plans/2026-08-19-twenty-capabilities/01-phase-1-nucleus-quick-dx.md",
            plan_text,
            Some(features),
            None,
            None,
        );

        assert_eq!(summary.items[0].kind, BreadcrumbKind::Plan);
        assert_eq!(summary.items[0].label, "01-phase-1-nucleus-quick-dx.md");
        assert_eq!(summary.items[1].kind, BreadcrumbKind::Plan);
        assert_eq!(summary.items[1].label, "master.md");

        let file_items: Vec<_> = summary.items.iter().filter(|i| i.kind == BreadcrumbKind::File).collect();
        assert_eq!(file_items.len(), 2);
        assert_eq!(file_items[0].label, "breadcrumb.rs");
        assert_eq!(file_items[0].target_path.as_deref(), Some("crates/hadron-chamber/src/app/render/breadcrumb.rs"));
        assert_eq!(file_items[1].label, "promoter.rs");
        assert_eq!(file_items[1].target_path.as_deref(), Some("crates/hadron-lattice/src/promoter.rs"));

        let inv_items: Vec<_> = summary.items.iter().filter(|i| i.kind == BreadcrumbKind::Invariant).collect();
        assert_eq!(inv_items.len(), 2);
        assert_eq!(inv_items[0].label, "Lavapipe Software Fallback");
        assert_eq!(inv_items[1].label, "NDJSON Message Framing");
    }
}
