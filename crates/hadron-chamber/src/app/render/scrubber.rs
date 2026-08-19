use super::*;
use chrono::{DateTime, Utc};
use gpui_component::ActiveTheme;

/// Computes the current scrub index, total events, and percentage progress.
pub fn compute_scrub_progress(
    scrub_ts: Option<DateTime<Utc>>,
    messages: &[crate::model::MessageRow],
) -> (usize, usize, f32) {
    let total = messages.len();
    if total == 0 {
        return (0, 0, 100.0);
    }

    let cursor = match scrub_ts {
        Some(ts) => messages.iter().take_while(|m| m.ts <= ts).count(),
        None => total,
    };

    let ratio = if total > 0 {
        (cursor as f32 / total as f32) * 100.0
    } else {
        100.0
    };

    (cursor, total, ratio)
}

impl Chamber {
    /// Renders the Vertical Session Time-Travel Scrubber & Event Inspector in the Right Rail (Capability #19).
    #[allow(dead_code)]
    pub(super) fn time_travel_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (cursor, total, ratio) = compute_scrub_progress(self.task_scrub, &self.view.messages);
        let is_live = self.task_scrub.is_none() || cursor == total;

        let status_pill = if is_live {
            h_flex()
                .gap_1p5()
                .items_center()
                .px_2p5()
                .py_1()
                .rounded_full()
                .bg(theme::bg_elevated())
                .border_1()
                .border_color(theme::halo_active())
                .child(
                    div()
                        .size(px(7.0))
                        .rounded_full()
                        .bg(theme::halo_active()),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme::text())
                        .child("LIVE STREAM"),
                )
        } else {
            h_flex()
                .gap_1p5()
                .items_center()
                .px_2p5()
                .py_1()
                .rounded_full()
                .bg(theme::bg_elevated())
                .border_1()
                .border_color(theme::halo_reasoning())
                .child(
                    div()
                        .size(px(7.0))
                        .rounded_full()
                        .bg(theme::halo_reasoning()),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme::halo_reasoning())
                        .child(format!("REWOUND ({cursor}/{total})")),
                )
        };

        let live_jump_btn = if !is_live {
            Some(
                div()
                    .id("tt-jump-live")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::accent())
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::field_base())
                    .hover(|s| s.opacity(0.9))
                    .cursor_pointer()
                    .child("Jump to Live ▶▶")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.task_scrub = None;
                        cx.notify();
                    })),
            )
        } else {
            None
        };

        // Navigation toolbar
        let nav_controls = h_flex()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .id("tt-nav-start")
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::text_muted())
                    .hover(|s| s.text_color(theme::text()))
                    .cursor_pointer()
                    .child("⏮ First")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(first) = this.view.messages.first() {
                            this.task_scrub = Some(first.ts);
                            cx.notify();
                        }
                    })),
            )
            .child(
                div()
                    .id("tt-nav-prev")
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::text_muted())
                    .hover(|s| s.text_color(theme::text()))
                    .cursor_pointer()
                    .child("◀ Step")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let msgs = &this.view.messages;
                        if msgs.is_empty() {
                            return;
                        }
                        let (cur, _, _) = compute_scrub_progress(this.task_scrub, msgs);
                        if cur > 1 {
                            if let Some(target) = msgs.get(cur - 2) {
                                this.task_scrub = Some(target.ts);
                                cx.notify();
                            }
                        } else if cur == 1 {
                            if let Some(first) = msgs.first() {
                                this.task_scrub = Some(first.ts);
                                cx.notify();
                            }
                        }
                    })),
            )
            .child(
                div()
                    .id("tt-nav-next")
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::text_muted())
                    .hover(|s| s.text_color(theme::text()))
                    .cursor_pointer()
                    .child("Step ▶")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let msgs = &this.view.messages;
                        if msgs.is_empty() {
                            return;
                        }
                        let (cur, tot, _) = compute_scrub_progress(this.task_scrub, msgs);
                        if cur < tot {
                            if let Some(target) = msgs.get(cur) {
                                this.task_scrub = Some(target.ts);
                                cx.notify();
                            }
                        } else {
                            this.task_scrub = None;
                            cx.notify();
                        }
                    })),
            );

        let progress_track = v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("Step {cursor} of {total}"))
                    .child(format!("{:.0}% timeline depth", ratio)),
            )
            .child(
                div()
                    .id("tt-progress-bar")
                    .w_full()
                    .h(px(6.0))
                    .rounded_full()
                    .bg(theme::term_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .child(
                        div()
                            .w(gpui::relative(ratio / 100.0))
                            .h_full()
                            .rounded_full()
                            .bg(if is_live {
                                gpui::Hsla::from(theme::accent())
                            } else {
                                theme::halo_reasoning()
                            }),
                    ),
            );

        let header_card = v_flex()
            .w_full()
            .p_3()
            .gap_2p5()
            .bg(theme::bg_surface())
            .border_b_1()
            .border_color(theme::glass_highlight())
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(status_pill)
                    .children(live_jump_btn),
            )
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(nav_controls)
                    .child(
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(theme::text_muted())
                            .child(match self.task_scrub {
                                Some(ts) => ts.format("%H:%M:%S UTC").to_string(),
                                None => "NOW (Live)".to_string(),
                            }),
                    ),
            )
            .child(progress_track);

        if self.view.messages.is_empty() {
            return v_flex()
                .size_full()
                .child(header_card)
                .child(
                    div()
                        .p_4()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child("No session events recorded yet."),
                )
                .into_any_element();
        }

        // Active inspected event index
        let active_ix = if cursor == 0 {
            0
        } else {
            (cursor - 1).min(self.view.messages.len().saturating_sub(1))
        };
        let active_msg = self.view.messages.get(active_ix);

        // Vertical timeline stream of events
        let mut timeline_col = v_flex().gap_2().p_3().w_full();

        let local_offset = *chrono::Local::now().offset();
        for (ix, m) in self.view.messages.iter().enumerate() {
            let is_active_step = ix == active_ix;
            let is_past_or_current = ix < cursor;
            let time_str = m.ts.with_timezone(&local_offset).format("%H:%M:%S").to_string();
            let author_id = self.resolve_identity(&m.from);
            let author_name = if author_id.name.starts_with('@') {
                author_id.name.clone()
            } else {
                format!("@{}", author_id.name)
            };

            let step_target_ts = m.ts;

            let dot_color: gpui::Hsla = if is_active_step {
                theme::accent().into()
            } else if is_past_or_current {
                theme::text_secondary().into()
            } else {
                theme::text_muted().into()
            };

            let card_bg: gpui::Hsla = if is_active_step {
                theme::bg_elevated().into()
            } else {
                theme::glass_card()
            };

            let card_border: gpui::Hsla = if is_active_step {
                theme::accent().into()
            } else {
                theme::glass_highlight()
            };

            let row_element = h_flex()
                .w_full()
                .gap_2p5()
                .items_start()
                // Left vertical timeline indicator
                .child(
                    v_flex()
                        .items_center()
                        .w(px(14.0))
                        .flex_shrink_0()
                        .pt(px(4.0))
                        .child(
                            div()
                                .size(if is_active_step { px(10.0) } else { px(6.0) })
                                .rounded_full()
                                .bg(dot_color)
                                .border_1()
                                .border_color(if is_active_step {
                                    gpui::Hsla::from(theme::accent())
                                } else {
                                    theme::glass_highlight()
                                }),
                        )
                        .child(
                            div()
                                .w(px(1.5))
                                .h(px(32.0))
                                .bg(theme::glass_highlight()),
                        ),
                )
                // Event Card
                .child(
                    v_flex()
                        .id(gpui::SharedString::from(format!("tt-event-step-{}", ix)))
                        .flex_1()
                        .min_w_0()
                        .p_2p5()
                        .rounded_lg()
                        .bg(card_bg)
                        .border_1()
                        .border_color(card_border)
                        .gap_1p5()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_elevated()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.task_scrub = Some(step_target_ts);
                            cx.notify();
                        }))
                        .child(
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_family(cx.theme().mono_font_family.clone())
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(theme::text_muted())
                                                .child(format!("#{:02}", ix + 1)),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(author_id.color)
                                                .child(author_name),
                                        )
                                        .child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_md()
                                                .bg(theme::bg_base())
                                                .text_xs()
                                                .text_color(theme::text_muted())
                                                .child(m.kind_label),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(theme::text_muted())
                                        .child(time_str),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(if is_active_step {
                                    theme::text()
                                } else {
                                    theme::text_secondary()
                                })
                                .truncate()
                                .child(m.body.clone()),
                        ),
                );

            timeline_col = timeline_col.child(row_element);
        }

        // Selected Inspector Detail Pane
        let inspector_detail = if let Some(m) = active_msg {
            let time_full = m.ts.with_timezone(&local_offset).format("%Y-%m-%d %H:%M:%S UTC").to_string();

            let token_stats = if let Some(ref u) = m.usage {
                let in_tok = u.spend.input.unwrap_or(0);
                let out_tok = u.spend.output.unwrap_or(0);
                let cache_tok = u.spend.cache_read.unwrap_or(0);
                format!("Usage: In: {in_tok} · Out: {out_tok} · Cache: {cache_tok}")
            } else if let Some(tok) = m.legacy_used_tokens {
                format!("Used: {tok} tokens")
            } else {
                "No token telemetry".to_string()
            };

            v_flex()
                .w_full()
                .p_3()
                .bg(theme::bg_elevated())
                .border_t_1()
                .border_color(theme::glass_highlight())
                .gap_2()
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_xs()
                                        .text_color(theme::text())
                                        .child(format!("Inspected Event #{:02}", active_ix + 1)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(time_full),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_color(theme::accent())
                                .child(token_stats),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .max_h(px(140.0))
                        .overflow_y_scrollbar()
                        .p_2()
                        .rounded_md()
                        .bg(theme::term_bg())
                        .border_1()
                        .border_color(theme::glass_highlight())
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(theme::text())
                        .child(m.body.clone()),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        v_flex()
            .size_full()
            .child(self.breadcrumb_bar(cx))
            .child(header_card)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("time-travel-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.time_travel_scroll)
                            .child(timeline_col),
                    )
                    .child(
                        div().absolute().top_0().bottom_0().right_0().child(
                            Scrollbar::vertical(&self.time_travel_scroll)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                    ),
            )
            .child(inspector_detail)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_scrub_progress() {
        let t0 = Utc::now();
        let t1 = t0 + chrono::Duration::seconds(10);
        let t2 = t0 + chrono::Duration::seconds(20);

        let messages = vec![
            crate::model::MessageRow {
                from: "human".into(),
                to: None,
                body: "1".into(),
                ts: t0,
                kind_label: "message",
                turn: None,
                severity: None,
                usage: None,
                legacy_used_tokens: None,
            },
            crate::model::MessageRow {
                from: "agy".into(),
                to: None,
                body: "2".into(),
                ts: t1,
                kind_label: "message",
                turn: None,
                severity: None,
                usage: None,
                legacy_used_tokens: None,
            },
            crate::model::MessageRow {
                from: "human".into(),
                to: None,
                body: "3".into(),
                ts: t2,
                kind_label: "message",
                turn: None,
                severity: None,
                usage: None,
                legacy_used_tokens: None,
            },
        ];

        // Live
        let (cur, total, ratio) = compute_scrub_progress(None, &messages);
        assert_eq!(cur, 3);
        assert_eq!(total, 3);
        assert_eq!(ratio, 100.0);

        // Rewound to t1
        let (cur, total, ratio) = compute_scrub_progress(Some(t1), &messages);
        assert_eq!(cur, 2);
        assert_eq!(total, 3);
        assert!((ratio - 66.66).abs() < 0.1);
    }
}
