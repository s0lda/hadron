use super::*;
use crate::model::SessionStats;

/// Segment of a horizontal flamechart bar.
#[derive(Debug, Clone, PartialEq)]
pub struct FlameSegment {
    pub label: &'static str,
    pub count: u64,
    pub ratio: f32,
    pub color_rgb: u32,
}

/// Computes proportional segments for token flamechart visualization.
pub fn compute_token_flame_segments(stats: &SessionStats) -> Vec<FlameSegment> {
    let total = stats.total_input + stats.total_output + stats.total_cache_read + stats.total_cache_write;
    if total == 0 {
        return Vec::new();
    }

    let mut segments = Vec::new();
    if stats.total_input > 0 {
        segments.push(FlameSegment {
            label: "Input",
            count: stats.total_input,
            ratio: (stats.total_input as f64 / total as f64) as f32,
            color_rgb: 0x60a5fa, // Blue
        });
    }
    if stats.total_output > 0 {
        segments.push(FlameSegment {
            label: "Output",
            count: stats.total_output,
            ratio: (stats.total_output as f64 / total as f64) as f32,
            color_rgb: 0xc084fc, // Purple
        });
    }
    if stats.total_cache_read > 0 {
        segments.push(FlameSegment {
            label: "Cache Read",
            count: stats.total_cache_read,
            ratio: (stats.total_cache_read as f64 / total as f64) as f32,
            color_rgb: 0x34d399, // Emerald
        });
    }
    if stats.total_cache_write > 0 {
        segments.push(FlameSegment {
            label: "Cache Write",
            count: stats.total_cache_write,
            ratio: (stats.total_cache_write as f64 / total as f64) as f32,
            color_rgb: 0xfbbf24, // Amber
        });
    }

    segments
}

impl Chamber {
    /// Renders the Token Spend & Latency Flamechart (Capability #18).
    pub(super) fn token_flamechart(&self, stats: &SessionStats, _cx: &mut Context<Self>) -> impl IntoElement {
        let segments = compute_token_flame_segments(stats);
        if segments.is_empty() {
            return div().into_any_element();
        }

        let mut bar = h_flex().h(px(18.0)).rounded_md().overflow_hidden().w_full().bg(theme::term_bg());

        for seg in &segments {
            let col = gpui::rgb(seg.color_rgb);
            bar = bar.child(
                div()
                    .flex_grow(seg.ratio)
                    .h_full()
                    .bg(col)
                    .hover(|s| s.opacity(0.85)),
            );
        }

        let mut legend = h_flex().gap_4().items_center().flex_wrap();
        for seg in &segments {
            legend = legend.child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(
                        div()
                            .size(px(8.0))
                            .rounded_full()
                            .bg(gpui::rgb(seg.color_rgb)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text())
                            .child(format!("{}: {} ({:.1}%)", seg.label, format_num(seg.count), seg.ratio * 100.0)),
                    ),
            );
        }

        v_flex()
            .gap_2()
            .p_3()
            .rounded_lg()
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .w_full()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text())
                            .child("Token Distribution & Flame Profile"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(format!("Total: {} tokens", format_num(stats.total_input + stats.total_output + stats.total_cache_read + stats.total_cache_write))),
                    ),
            )
            .child(bar)
            .child(legend)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_token_flame_segments() {
        let stats = SessionStats {
            total_input: 1000,
            total_output: 500,
            total_cache_read: 2000,
            total_cache_write: 500,
            ..Default::default()
        };

        let segments = compute_token_flame_segments(&stats);
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].label, "Input");
        assert!((segments[0].ratio - 0.25).abs() < 0.01);
        assert_eq!(segments[2].label, "Cache Read");
        assert!((segments[2].ratio - 0.50).abs() < 0.01);
    }
}
