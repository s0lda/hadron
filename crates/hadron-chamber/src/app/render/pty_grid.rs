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
        let num_rows = (num_terms + cols - 1) / cols;

        let mut grid_items = v_flex().gap_2().w_full().min_w_0();
        if num_rows <= 2 {
            grid_items = grid_items.flex_1().min_h_0().size_full();
        }

        for (row_ix, chunk) in self.terminals.chunks(cols).enumerate() {
            let mut row = h_flex().gap_2().w_full().min_w_0();
            if num_rows <= 2 {
                row = row.flex_1().min_h_0();
            } else {
                row = row.flex_shrink_0().min_h(px(160.0)).h(px(180.0));
            }

            for (col_ix, tab) in chunk.iter().enumerate() {
                let tab_ix = row_ix * cols + col_ix;
                let is_active = tab_ix == self.active_terminal_index;
                let (border, bg): (gpui::Hsla, gpui::Hsla) = if is_active {
                    (theme::accent().into(), theme::bg_elevated().into())
                } else {
                    (theme::glass_highlight(), theme::bg_surface().into())
                };

                let pty_content = if let Some(term) = &tab.term {
                    let snap = term.snapshot();
                    let mut lines_div = v_flex()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .p_2()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(11.0))
                        .line_height(px(14.0))
                        .overflow_hidden();

                    let mut has_text = false;
                    for line in &snap.lines {
                        let mut line_row = h_flex()
                            .h(px(14.0))
                            .whitespace_nowrap()
                            .min_w_0();
                        let mut line_empty = true;
                        for run in &line.runs {
                            if !run.text.is_empty() {
                                line_empty = false;
                                has_text = true;
                                let mut run_div = div()
                                    .text_color(gpui::rgb(pack_rgb(run.fg)))
                                    .bg(gpui::rgb(pack_rgb(run.bg)));
                                if run.has_cursor {
                                    run_div = run_div
                                        .border_l(px(2.0))
                                        .border_color(gpui::rgb(pack_rgb(run.fg)));
                                }
                                line_row = line_row.child(run_div.child(run.text.clone()));
                            }
                        }
                        if line_empty {
                            line_row = line_row.child(div().child(" "));
                        }
                        lines_div = lines_div.child(line_row);
                    }

                    if has_text {
                        lines_div.into_any_element()
                    } else {
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .p_2()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child("Terminal ready (idle)")
                            .into_any_element()
                    }
                } else {
                    div()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .p_2()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child("Starting shell / PTY process...")
                        .into_any_element()
                };

                let tab_title = tab.title.clone();
                let card = v_flex()
                    .id(SharedString::from(format!("pty-grid-card-{tab_ix}")))
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_terminal(tab_ix, cx);
                        window.focus(&this.terminal_focus, cx);
                    }))
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
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("pty-grid-close-{tab_ix}")))
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_full()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::glass_surface()).text_color(theme::text()))
                                    .text_color(theme::text_muted())
                                    .text_xs()
                                    .child("×")
                                    .on_click(cx.listener(move |this, _, _window, cx| {
                                        this.close_terminal(tab_ix, cx);
                                    })),
                            ),
                    )
                    .child(pty_content);

                row = row.child(card);
            }

            // Fill empty columns in the last row to keep aligned grid columns
            if chunk.len() < cols {
                for _ in 0..(cols - chunk.len()) {
                    row = row.child(div().flex_1().min_w_0().min_h_0());
                }
            }

            grid_items = grid_items.child(row);
        }

        let scroll_container = div()
            .id("multi-pty-grid-scroll")
            .size_full()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_x_hidden()
            .overflow_y_scroll()
            .track_scroll(&self.terminal_grid_scroll)
            .track_focus(&self.terminal_focus)
            .on_key_down(cx.listener(Self::on_terminal_key))
            .child(grid_items);

        div()
            .relative()
            .flex_1()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(scroll_container)
            .child(
                div().absolute().top_0().bottom_0().right_0().child(
                    Scrollbar::vertical(&self.terminal_grid_scroll)
                        .scrollbar_show(ScrollbarShow::Always),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_multi_terminal_grid_chunking() {
        let titles = vec!["term-1", "term-2", "term-3"];
        let cols = if titles.len() <= 1 { 1 } else { 2 };
        let chunks: Vec<&[&str]> = titles.chunks(cols).collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1].len(), 1);

        let num_rows = (titles.len() + cols - 1) / cols;
        assert_eq!(num_rows, 2);

        // 4 terminals: 2 rows of 2
        let titles_4 = vec!["t1", "t2", "t3", "t4"];
        let chunks_4: Vec<&[&str]> = titles_4.chunks(2).collect();
        assert_eq!(chunks_4.len(), 2);
        assert_eq!(chunks_4[0].len(), 2);
        assert_eq!(chunks_4[1].len(), 2);

        // 5 terminals: 3 rows
        let titles_5 = vec!["t1", "t2", "t3", "t4", "t5"];
        let chunks_5: Vec<&[&str]> = titles_5.chunks(2).collect();
        assert_eq!(chunks_5.len(), 3);
        assert_eq!(chunks_5[2].len(), 1);
    }
}
