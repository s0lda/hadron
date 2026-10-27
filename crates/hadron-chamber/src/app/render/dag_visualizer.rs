use super::*;
use gpui::{Point, Pixels};
use gpui_component::ActiveTheme;
use hadron_lattice::task_graph::TaskGraph;
use std::hash::{Hash, Hasher};

pub(crate) fn compute_plan_content_hash(content: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

pub(crate) const DAG_HEADER_STATS_MIN_WIDTH: f32 = 580.0;

pub(crate) fn show_dag_inline_stats(inspector_width: f32) -> bool {
    inspector_width >= DAG_HEADER_STATS_MIN_WIDTH
}

pub trait ScrollbarExt {
    fn both<H: gpui_component::scroll::ScrollbarHandle + Clone>(scroll_handle: &H) -> Scrollbar;
}

impl ScrollbarExt for Scrollbar {
    fn both<H: gpui_component::scroll::ScrollbarHandle + Clone>(scroll_handle: &H) -> Scrollbar {
        Scrollbar::new(scroll_handle)
    }
}

/// Convert a gpui ScrollDelta to a pixel point.
pub fn scroll_delta_to_pixels(delta: &gpui::ScrollDelta) -> Point<Pixels> {
    match delta {
        gpui::ScrollDelta::Lines(d) => gpui::point(px(d.x * 32.0), px(d.y * 32.0)),
        gpui::ScrollDelta::Pixels(d) => gpui::point(d.x, d.y),
    }
}

/// Compute updated scroll offset for the DAG waves container from a scroll wheel event.
/// If `shift` is held and vertical delta dominates, maps vertical scroll to horizontal scroll.
/// Clamps the resulting offset within `[-max_offset.x, 0]` and `[-max_offset.y, 0]`.
pub fn compute_dag_scroll_offset(
    current_offset: Point<Pixels>,
    max_offset: Point<Pixels>,
    delta: Point<Pixels>,
    shift_pressed: bool,
) -> Point<Pixels> {
    let (dx, dy) = if shift_pressed && delta.x.abs() < delta.y.abs() {
        (delta.y, px(0.0))
    } else {
        (delta.x, delta.y)
    };

    let min_x = -max_offset.x.abs();
    let min_y = -max_offset.y.abs();

    let new_x = (current_offset.x + dx).clamp(min_x, px(0.0));
    let new_y = (current_offset.y + dy).clamp(min_y, px(0.0));

    gpui::point(new_x, new_y)
}

impl Chamber {
    pub(super) fn resolve_cached_plan_dag(
        &self,
        repo: &std::path::Path,
        content: &str,
    ) -> CachedPlanDag {
        let content_hash = compute_plan_content_hash(content);
        let head_oid = self.git_branch_fingerprint.clone();

        if let Some(cached) = self.cached_plan_dag.borrow().as_ref() {
            if cached.content_hash == content_hash && cached.head_oid == head_oid {
                return cached.clone();
            }
        }

        let graph = TaskGraph::parse_from_markdown(content);
        let waves = match graph.compute_waves() {
            Ok(w) => w,
            Err(_) => vec![graph.tasks.clone()],
        };
        let ready_ids: std::collections::HashSet<String> =
            graph.ready_tasks().into_iter().map(|t| t.id).collect();
        let mut landed_commits = std::collections::HashSet::new();
        for task in &graph.tasks {
            if let Some(ref hash) = task.commit_hash {
                if crate::vcs::is_commit_merged(repo, hash, "main") {
                    landed_commits.insert(hash.clone());
                }
            }
        }
        let progress = crate::app::mentions::parse_plan_progress_with_status(repo, content);
        let task_groups = crate::app::mentions::parse_plan_tasks_with_status(repo, content);

        let built = CachedPlanDag {
            content_hash,
            head_oid,
            graph,
            waves,
            ready_ids,
            landed_commits,
            progress,
            task_groups,
        };
        *self.cached_plan_dag.borrow_mut() = Some(built.clone());
        built
    }

    /// Renders the Interactive Plan DAG & Wave Visualizer (Capability #11).
    #[allow(dead_code)]
    pub(super) fn plan_dag_visualizer(&self, content: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let repo = crate::vcs::repo_root_of(&self.path).to_path_buf();
        let cached = self.resolve_cached_plan_dag(&repo, content);
        let graph = &cached.graph;
        if graph.tasks.is_empty() {
            return div().into_any_element();
        }
        let waves = &cached.waves;

        if waves.is_empty() {
            return div().into_any_element();
        }

        let is_expanded = self.plan_dag_expanded;
        let ready_ids = &cached.ready_ids;
        let unblocked_count = ready_ids.len();
        let show_stats = show_dag_inline_stats(self.prefs.inspector_width);

        let header_bar = h_flex()
            .id("dag-toggle-header")
            .items_center()
            .justify_between()
            .gap_2()
            .w_full()
            .min_w_0()
            .overflow_hidden()
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
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
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
                            .truncate()
                            .child("Execution Topology & Wave DAG"),
                    )
                    .when(show_stats, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_color(theme::text_muted())
                                .truncate()
                                .child(format!(
                                    "({} waves · {} nodes · {} unblocked)",
                                    waves.len(),
                                    graph.tasks.len(),
                                    unblocked_count
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
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

        let mut waves_row = h_flex()
            .id("dag-waves-row")
            .gap_2()
            .items_start()
            .track_scroll(&self.dag_waves_scroll)
            .max_h(px(320.0))
            .overflow_x_scroll()
            .overflow_y_scroll()
            .pb_1()
            .w_full()
            .min_w_0()
            .on_scroll_wheel(cx.listener(|this, ev: &gpui::ScrollWheelEvent, _window, cx| {
                cx.stop_propagation();
                let delta_px = scroll_delta_to_pixels(&ev.delta);
                let current_offset = this.dag_waves_scroll.offset();
                let max_offset = this.dag_waves_scroll.max_offset();
                let new_offset = compute_dag_scroll_offset(
                    current_offset,
                    max_offset,
                    delta_px,
                    ev.modifiers.shift,
                );
                this.dag_waves_scroll.set_offset(new_offset);
                cx.notify();
            }));

        for (wave_idx, wave) in waves.iter().enumerate() {
            let mut col = v_flex()
                .gap_1p5()
                .p_2()
                .rounded_lg()
                .bg(theme::bg_surface())
                .border_1()
                .border_color(theme::glass_highlight())
                .w(px(210.0))
                .flex_none()
                .min_w(px(180.0))
                .max_w(px(240.0));

            col = col.child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .pb_1()
                    .border_b_1()
                    .border_color(theme::glass_highlight())
                    .child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(theme::bg_base())
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
                let is_landed = if let Some(ref hash) = task.commit_hash {
                    cached.landed_commits.contains(hash)
                } else {
                    task.completed
                };

                let (border_col, bg_col, status_icon, status_label, text_col) = if is_landed {
                    (
                        theme::halo_idle().opacity(0.45),
                        theme::bg_elevated(),
                        "✓",
                        task.commit_hash.as_deref().unwrap_or("landed"),
                        theme::halo_idle(),
                    )
                } else if task.completed {
                    (
                        theme::info().opacity(0.45).into(),
                        theme::bg_elevated(),
                        "✓",
                        task.commit_hash.as_deref().unwrap_or("in-branch"),
                        theme::info().into(),
                    )
                } else if is_ready {
                    (
                        theme::halo_active().opacity(0.45),
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
                    .p_1p5()
                    .rounded_md()
                    .bg(bg_col)
                    .border_1()
                    .border_color(border_col)
                    .gap_0p5()
                    .w_full()
                    .min_w_0();

                card = card.child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .w_full()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(text_col)
                                .truncate()
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
                        .truncate()
                        .child(task.title.clone()),
                );

                if !task.depends_on.is_empty() && !task.completed {
                    card = card.child(
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(theme::text_muted())
                            .truncate()
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
                    .relative()
                    .p_2()
                    .rounded_lg()
                    .bg(theme::term_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .w_full()
                    .child(waves_row)
                    .child(Scrollbar::both(&self.dag_waves_scroll)),
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

    #[test]
    fn test_plan_content_hash_cache_key_stability() {
        let md = "- [x] Task 1.1: Setup (commit abc1234)\n- [ ] Task 1.2: Next (after: task-1.1)\n";
        let h1 = compute_plan_content_hash(md);
        let h2 = compute_plan_content_hash(md);
        let h3 = compute_plan_content_hash("- [x] Task 1.1: Setup\n- [x] Task 1.2: Next\n");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_show_dag_inline_stats_hides_on_narrow_inspector() {
        assert!(!show_dag_inline_stats(300.0));
        assert!(!show_dag_inline_stats(480.0));
        assert!(!show_dag_inline_stats(579.0));
        assert!(show_dag_inline_stats(580.0));
        assert!(show_dag_inline_stats(640.0));
    }

    #[test]
    fn test_compute_dag_scroll_offset_regular_vertical() {
        let current = gpui::point(px(0.0), px(0.0));
        let max_offset = gpui::point(px(500.0), px(200.0));
        let delta = gpui::point(px(0.0), px(-50.0));
        let new_offset = compute_dag_scroll_offset(current, max_offset, delta, false);
        assert_eq!(new_offset.x, px(0.0));
        assert_eq!(new_offset.y, px(-50.0));
    }

    #[test]
    fn test_compute_dag_scroll_offset_shift_maps_vertical_to_horizontal() {
        let current = gpui::point(px(0.0), px(0.0));
        let max_offset = gpui::point(px(500.0), px(200.0));
        let delta = gpui::point(px(0.0), px(-80.0));
        let new_offset = compute_dag_scroll_offset(current, max_offset, delta, true);
        assert_eq!(new_offset.x, px(-80.0));
        assert_eq!(new_offset.y, px(0.0));
    }

    #[test]
    fn test_compute_dag_scroll_offset_clamping() {
        let current = gpui::point(px(-450.0), px(-180.0));
        let max_offset = gpui::point(px(500.0), px(200.0));

        // Overscroll down/right
        let delta_exceed = gpui::point(px(-100.0), px(-50.0));
        let clamped = compute_dag_scroll_offset(current, max_offset, delta_exceed, false);
        assert_eq!(clamped.x, px(-500.0));
        assert_eq!(clamped.y, px(-200.0));

        // Overscroll top/left
        let delta_pos = gpui::point(px(1000.0), px(1000.0));
        let clamped_zero = compute_dag_scroll_offset(current, max_offset, delta_pos, false);
        assert_eq!(clamped_zero.x, px(0.0));
        assert_eq!(clamped_zero.y, px(0.0));
    }
}
