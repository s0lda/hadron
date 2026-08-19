use super::*;
use chrono::{DateTime, Utc};

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
    /// Renders the Session Time-Travel Scrub Bar (Capability #19).
    pub(super) fn time_travel_scrubber(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (cursor, total, ratio) = compute_scrub_progress(self.task_scrub, &self.view.messages);
        let is_live = self.task_scrub.is_none() || cursor == total;

        let status_label = if is_live {
            "LIVE".to_string()
        } else {
            format!("REWOUND ({cursor}/{total})")
        };

        let status_color = if is_live {
            theme::halo_active()
        } else {
            theme::halo_reasoning()
        };

        let fill_color: gpui::Hsla = if is_live {
            theme::accent().into()
        } else {
            theme::halo_reasoning()
        };

        let bar = h_flex()
            .id("time-travel-scrubber-bar")
            .h(px(6.0))
            .w_full()
            .rounded_full()
            .bg(theme::term_bg())
            .border_1()
            .border_color(theme::glass_highlight())
            .child(
                div()
                    .w(gpui::relative(ratio / 100.0))
                    .h_full()
                    .rounded_full()
                    .bg(fill_color),
            );

        v_flex()
            .w_full()
            .gap_1p5()
            .px_3()
            .py_1p5()
            .bg(theme::bg_surface())
            .border_b_1()
            .border_color(theme::glass_highlight())
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
                                    .w_2()
                                    .h_2()
                                    .rounded_full()
                                    .bg(status_color),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text())
                                    .child(format!("Time-Travel: {status_label}")),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .id("scrub-step-back")
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
                                        if let Some(prev) = this.view.messages.iter().rev().find(|m| {
                                            this.task_scrub.map_or(true, |ts| m.ts < ts)
                                        }) {
                                            this.task_scrub = Some(prev.ts);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("scrub-step-live")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(theme::glass_card())
                                    .border_1()
                                    .border_color(theme::glass_highlight())
                                    .text_xs()
                                    .text_color(theme::accent())
                                    .hover(|s| s.opacity(0.85))
                                    .cursor_pointer()
                                    .child("Jump to Live ▶▶")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.task_scrub = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(bar)
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
