use super::*;
use crate::model::breadcrumb::{BreadcrumbKind, BreadcrumbSummary};

impl Chamber {
    /// Render the general Nucleus Invariants & Context Breadcrumb Bar.
    /// Displays active feature map rows, invariants, lessons, and active plan context.
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

        self.render_breadcrumb_summary(summary, "Nucleus & Swarm Context", cx)
    }

    /// Render dynamic breadcrumb pills specifically contextualized to the currently viewed plan.
    pub(super) fn breadcrumb_bar_for_plan(
        &self,
        rel_path: &str,
        content: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let repo = crate::vcs::repo_root_of(&self.path);
        let nucleus_dir = repo.join(".hadron").join("nucleus");

        let feat_file = nucleus_dir.join("features.md");
        let feat_content = std::fs::read_to_string(&feat_file).ok();

        let inv_file = nucleus_dir.join("invariants").join("always.md");
        let inv_content = std::fs::read_to_string(&inv_file).ok();

        let index_file = nucleus_dir.join("index.md");
        let index_content = std::fs::read_to_string(&index_file).ok();

        let summary = BreadcrumbSummary::from_plan(
            rel_path,
            content,
            feat_content.as_deref(),
            inv_content.as_deref(),
            index_content.as_deref(),
        );

        self.render_breadcrumb_summary(summary, "Plan & Nucleus Context", cx)
    }

    fn render_breadcrumb_summary(
        &self,
        summary: BreadcrumbSummary,
        header_title: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if summary.is_empty() {
            return div().into_any_element();
        }

        let item_count = summary.items.len();
        let is_collapsed = self.plan_context_collapsed;

        let header_row = h_flex()
            .id("nucleus-breadcrumb-toggle")
            .w_full()
            .min_w_0()
            .items_center()
            .justify_between()
            .p_2p5()
            .rounded_lg()
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .cursor_pointer()
            .hover(|s| s.bg(theme::bg_elevated()))
            .on_click(cx.listener(|this, _, _window, cx| {
                this.plan_context_collapsed = !this.plan_context_collapsed;
                cx.notify();
            }))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        Icon::new(if is_collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .small()
                        .text_color(theme::text_muted()),
                    )
                    .child(
                        Icon::new(IconName::Info)
                            .small()
                            .text_color(theme::accent()),
                    )
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_sm()
                            .text_color(theme::text())
                            .truncate()
                            .child(header_title),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .bg(theme::bg_base())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme::text_muted())
                    .child(format!("{item_count} active")),
            );

        let mut card = v_flex()
            .id("nucleus-breadcrumb-hud")
            .w_full()
            .min_w_0()
            .gap_2()
            .child(header_row);

        if !is_collapsed {
            let body = v_flex()
                .w_full()
                .min_w_0()
                .gap_1p5()
                .p_2p5()
                .rounded_lg()
                .bg(theme::glass_card())
                .border_1()
                .border_color(theme::glass_highlight())
                .children(summary.items.into_iter().enumerate().map(|(ix, item)| {
                        let kind_color: gpui::Hsla = match item.kind {
                            BreadcrumbKind::Plan => theme::accent().into(),
                            BreadcrumbKind::File => gpui::rgb(0x38bdf8).into(),
                            BreadcrumbKind::Feature => theme::halo_active(),
                            BreadcrumbKind::Invariant => theme::halo_reasoning(),
                            BreadcrumbKind::Lesson => theme::text_secondary().into(),
                        };
                        let icon = item.kind.icon_char();
                        let target_path = item.target_path.clone();

                        let mut row = h_flex()
                            .id(gpui::SharedString::from(format!("breadcrumb-pill-{ix}")))
                            .w_full()
                            .min_w_0()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .px_2p5()
                            .py_1p5()
                            .rounded_md()
                            .bg(theme::bg_base())
                            .border_1()
                            .border_color(theme::glass_highlight());

                        if let Some(target) = target_path {
                            let target_click = target.clone();
                            row = row
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::bg_elevated()).border_color(theme::accent()))
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.handle_context_menu_action(
                                        ContextMenuAction::OpenFile(target_click.clone()),
                                        cx,
                                    );
                                }));
                        }

                        row.child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .child(icon),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(theme::bg_surface())
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(kind_color)
                                        .child(item.kind.label()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(theme::text())
                                        .truncate()
                                        .child(item.label),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1p5()
                                .flex_shrink_0()
                                .when_some(item.detail, |this, detail| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(theme::bg_surface())
                                            .text_color(theme::text_muted())
                                            .child(detail),
                                    )
                                })
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child("↗"),
                                ),
                        )
                    }));
            card = card.child(body);
        }

        card.into_any_element()
    }
}
