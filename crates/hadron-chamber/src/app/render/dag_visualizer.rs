use super::*;
use gpui_component::ActiveTheme;
use hadron_lattice::task_graph::TaskGraph;

impl Chamber {
    /// Renders the Interactive Plan DAG & Wave Visualizer (Capability #11).
    pub(super) fn plan_dag_visualizer(&self, content: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let graph = TaskGraph::parse_from_markdown(content);
        let waves = match graph.compute_waves() {
            Ok(w) => w,
            Err(_) => return div().child("Circular dependency in task graph").into_any_element(),
        };

        if waves.is_empty() || graph.tasks.is_empty() {
            return div().into_any_element();
        }

        let is_expanded = self.plan_dag_expanded;
        let ready_ids: std::collections::HashSet<String> = graph.ready_tasks().into_iter().map(|t| t.id).collect();

        let header_bar = h_flex()
            .id("dag-toggle-header")
            .items_center()
            .justify_between()
            .w_full()
            .p_2p5()
            .rounded_lg()
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .cursor_pointer()
            .hover(|s| s.bg(theme::bg_elevated()))
            .on_click(cx.listener(|this, _, _, cx| {
                this.plan_dag_expanded = !this.plan_dag_expanded;
                cx.notify();
            }))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(if is_expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .small()
                        .text_color(theme::text_muted()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text())
                            .child("Execution Topology & Wave DAG"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(theme::text_muted())
                            .child(format!("({} waves · {} nodes)", waves.len(), graph.tasks.len())),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(theme::bg_base())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::accent())
                    .child(if is_expanded { "Hide Wave Graph" } else { "Show Wave Graph" }),
            );

        if !is_expanded {
            return header_bar.into_any_element();
        }

        let mut waves_row = h_flex().gap_2p5().items_start().overflow_x_scrollbar().pb_2().w_full().min_w_0();

        for (wave_idx, wave) in waves.iter().enumerate() {
            let mut col = v_flex()
                .gap_2()
                .p_2p5()
                .rounded_lg()
                .bg(theme::bg_surface())
                .border_1()
                .border_color(theme::glass_highlight())
                .flex_1()
                .min_w(px(160.0));

            col = col.child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::accent())
                            .child(format!("Wave {}", wave_idx + 1)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(theme::text_muted())
                            .child(format!("{} tasks", wave.len())),
                    ),
            );

            for task in wave {
                let is_ready = ready_ids.contains(&task.id);
                let (border_col, bg_col, status_icon, status_label, text_col) = if task.completed {
                    (
                        theme::halo_idle(),
                        theme::bg_elevated(),
                        "✓",
                        task.commit_hash.as_deref().unwrap_or("done"),
                        theme::halo_idle(),
                    )
                } else if is_ready {
                    (
                        theme::halo_active(),
                        theme::bg_elevated(),
                        "▶",
                        "ready",
                        theme::halo_active(),
                    )
                } else {
                    (
                        theme::glass_highlight(),
                        theme::bg_surface(),
                        "⏸",
                        "blocked",
                        theme::text_muted().into(),
                    )
                };

                let mut card = v_flex()
                    .p_2()
                    .rounded_md()
                    .bg(bg_col)
                    .border_1()
                    .border_color(border_col)
                    .gap_1();

                card = card.child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(text_col)
                                .child(format!("{status_icon} {}", task.id)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_color(theme::text_muted())
                                .child(status_label.to_string()),
                        ),
                );

                card = card.child(
                    div()
                        .text_xs()
                        .text_color(theme::text())
                        .child(task.title.clone()),
                );

                if !task.depends_on.is_empty() && !task.completed {
                    card = card.child(
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(theme::text_muted())
                            .child(format!("after: {}", task.depends_on.join(", "))),
                    );
                }

                col = col.child(card);
            }

            waves_row = waves_row.child(col);
        }

        v_flex()
            .gap_2()
            .w_full()
            .child(header_bar)
            .child(
                v_flex()
                    .p_2()
                    .rounded_lg()
                    .bg(theme::term_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .w_full()
                    .child(waves_row),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_dag_wave_resolution() {
        let md = r#"
- [x] Task 1.1: Setup (commit abc1234)
- [ ] Task 1.2: Middle (after: task-1.1)
- [ ] Task 1.3: End (after: task-1.2)
"#;
        let graph = TaskGraph::parse_from_markdown(md);
        let waves = graph.compute_waves().unwrap();
        assert_eq!(waves.len(), 3);
        assert!(graph.tasks[0].completed);
        assert_eq!(graph.tasks[0].commit_hash.as_deref(), Some("abc1234"));
    }
}
