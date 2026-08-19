use super::*;
use crate::model::breadcrumb::{BreadcrumbKind, BreadcrumbSummary};

impl Chamber {
    /// Render the Nucleus Invariants & Context Breadcrumb Bar (Capability #15).
    /// Displays active feature map rows, invariants, lessons, and active plan context.
    /// Each pill is interactive and opens its corresponding nucleus or source file.
    pub(super) fn breadcrumb_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repo = crate::vcs::repo_root_of(&self.path);
        let nucleus_dir = repo.join(".hadron").join("nucleus");

        let feat_file = nucleus_dir.join("features.md");
        let feat_content = std::fs::read_to_string(&feat_file).ok();

        let inv_file = nucleus_dir.join("invariants").join("always.md");
        let inv_content = std::fs::read_to_string(&inv_file).ok();

        let index_file = nucleus_dir.join("index.md");
        let index_content = std::fs::read_to_string(&index_file).ok();

        let summary = BreadcrumbSummary::from_nucleus(
            feat_content.as_deref(),
            inv_content.as_deref(),
            index_content.as_deref(),
            self.last_plan_path.as_deref(),
        );

        if summary.is_empty() {
            return div().into_any_element();
        }

        h_flex()
            .id("nucleus-breadcrumb-hud")
            .w_full()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .bg(theme::tab_bar_bg())
            .border_b_1()
            .border_color(theme::glass_highlight())
            .text_xs()
            .overflow_x_scroll()
            .children(summary.items.into_iter().enumerate().map(|(ix, item)| {
                let kind_color: gpui::Hsla = match item.kind {
                    BreadcrumbKind::Plan => theme::accent().into(),
                    BreadcrumbKind::Feature => theme::halo_active(),
                    BreadcrumbKind::Invariant => theme::halo_reasoning(),
                    BreadcrumbKind::Lesson => theme::text_secondary().into(),
                };
                let icon = item.kind.icon_char();

                let action_target: Option<String> = match item.kind {
                    BreadcrumbKind::Plan => item.detail.clone(),
                    BreadcrumbKind::Feature => {
                        item.detail.clone().or_else(|| Some(".hadron/nucleus/features.md".to_string()))
                    }
                    BreadcrumbKind::Invariant => Some(".hadron/nucleus/invariants/always.md".to_string()),
                    BreadcrumbKind::Lesson => {
                        let slug = item.label.trim();
                        let note_path = format!(".hadron/nucleus/notes/{slug}.md");
                        if repo.join(&note_path).exists() {
                            Some(note_path)
                        } else {
                            Some(".hadron/nucleus/index.md".to_string())
                        }
                    }
                };

                let target_path = action_target.clone();

                h_flex()
                    .id(gpui::SharedString::from(format!("breadcrumb-pill-{ix}")))
                    .gap_1p5()
                    .items_center()
                    .flex_shrink_0()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::bg_elevated()).border_color(theme::accent()))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if let Some(ref path) = target_path {
                                this.handle_context_menu_action(
                                    ContextMenuAction::OpenFile(path.clone()),
                                    cx,
                                );
                            }
                        }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .child(icon),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(kind_color)
                            .child(format!("{}:", item.kind.label())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text())
                            .child(item.label),
                    )
                    .when_some(item.detail, |this, detail| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(format!("({detail})")),
                        )
                    })
            }))
            .into_any_element()
    }
}
