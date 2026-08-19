use super::*;

impl Chamber {
    /// Render the Quick REPL & Tool Scratchpad Overlay (Capability #16).
    pub(super) fn repl_overlay(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let output = self.repl_output.as_ref().map(|out| {
            v_flex()
                .w_full()
                .max_h(px(240.0))
                .p_3()
                .rounded_lg()
                .bg(theme::term_bg())
                .border_1()
                .border_color(theme::glass_highlight())
                .text_xs()
                .text_color(theme::text())
                .child(out.clone())
        });

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(60.0))
            .bg(rgba(0x00000099))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.repl_overlay_open = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .occlude()
                    .w(px(580.0))
                    .max_w_full()
                    .rounded_xl()
                    .bg(theme::bg_elevated())
                    .border_1()
                    .border_color(theme::accent())
                    .shadow_lg()
                    .p_4()
                    .gap_3()
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
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(theme::accent())
                                            .child("Quick REPL & Tool Scratchpad"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child("(/command · ?note · tool:name)"),
                                    ),
                            )
                            .child(
                                div()
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .hover(|s| s.text_color(theme::text()))
                                    .child("Esc to close")
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.repl_overlay_open = false;
                                            cx.notify();
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .child(self.repl_input.clone())
                    )
                    .children(output)
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("repl-eval")
                                    .label("Evaluate")
                                    .primary()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.execute_repl_query(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
