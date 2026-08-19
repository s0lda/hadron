use super::*;
use gpui_component::ActiveTheme;

impl Chamber {
    /// Renders the Multi-Quark PTY Grid (Capability #17).
    pub(super) fn multi_pty_grid(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.terminals.is_empty() {
            return div()
                .p_4()
                .text_xs()
                .text_color(theme::text_muted())
                .child("No active PTY terminals")
                .into_any_element();
        }

        let num_terms = self.terminals.len();
        let cols = if num_terms <= 1 { 1 } else { 2 };

        let mut grid_container = v_flex().gap_2().size_full().p_2();

        for chunk in self.terminals.chunks(cols) {
            let mut row = h_flex().gap_2().flex_1().w_full();
            for tab in chunk {
                let is_active = self.terminals.iter().position(|t| t.title == tab.title) == Some(self.active_terminal_index);
                let (border, bg): (gpui::Hsla, gpui::Hsla) = if is_active {
                    (theme::accent().into(), theme::bg_elevated().into())
                } else {
                    (theme::glass_highlight(), theme::bg_surface().into())
                };

                let pty_content = if let Some(term) = &tab.term {
                    let snap = term.snapshot();
                    let text = snap.plain_text();
                    div()
                        .flex_1()
                        .p_2()
                        .overflow_hidden()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .text_color(theme::text())
                        .child(if text.trim().is_empty() { "Terminal ready (idle)".to_string() } else { text })
                } else {
                    div()
                        .flex_1()
                        .p_2()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child("Starting shell / PTY process...")
                };

                let tab_title = tab.title.clone();
                let card = v_flex()
                    .flex_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .px_2()
                            .py_1()
                            .bg(theme::term_bg())
                            .border_b_1()
                            .border_color(theme::glass_highlight())
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(
                                        div()
                                            .size(px(6.0))
                                            .rounded_full()
                                            .bg(if is_active { theme::halo_active() } else { theme::halo_idle() }),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(if is_active { theme::accent() } else { theme::text() })
                                            .child(tab_title),
                                    ),
                            ),
                    )
                    .child(pty_content);

                row = row.child(card);
            }
            grid_container = grid_container.child(row);
        }

        grid_container.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_terminal_grid_chunking() {
        let titles = vec!["term-1", "term-2", "term-3"];
        let cols = if titles.len() <= 1 { 1 } else { 2 };
        let chunks: Vec<&[&str]> = titles.chunks(cols).collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1].len(), 1);
    }
}
