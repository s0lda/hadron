use super::*;
use crate::model::breadcrumb::{BreadcrumbKind, BreadcrumbSummary};

impl Chamber {
    /// Render the Nucleus Invariants & Context Breadcrumb Bar (Capability #15).
    /// Displays active feature map rows, invariants, and active plan context.
    pub(super) fn breadcrumb_bar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
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
            .children(summary.items.into_iter().map(|item| {
                let kind_color: gpui::Hsla = match item.kind {
                    BreadcrumbKind::Plan => theme::accent().into(),
                    BreadcrumbKind::Feature => theme::halo_active(),
                    BreadcrumbKind::Invariant => theme::halo_reasoning(),
                    BreadcrumbKind::Lesson => theme::text_secondary().into(),
                };
                let icon = item.kind.icon_char();

                h_flex()
                    .gap_1p5()
                    .items_center()
                    .flex_shrink_0()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
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
