use super::*;
use crate::model::attention::{compute_file_attention, AttentionLevel};

impl Chamber {
    /// Renders the Swarm Attention & File Heatmap HUD (Capability #13).
    pub(super) fn attention_hud(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut edits = Vec::new();
        for m in self.view.messages.iter().rev().take(25) {
            if m.kind_label == "edit" {
                if let Some(path) = m.body.split_whitespace().find(|w| w.contains('/') || w.contains('.')) {
                    edits.push((m.from.clone(), path.trim_matches(':').to_string()));
                }
            }
        }

        let file_attentions = compute_file_attention(&self.last_live_activities, &edits);
        if file_attentions.is_empty() {
            return div().into_any_element();
        }

        let mut row = h_flex().gap_2().items_center().overflow_x_scrollbar().w_full().py_1();

        row = row.child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme::accent())
                .child("Swarm Focus:"),
        );

        for att in file_attentions.iter().take(6) {
            let (bg, border, text_col, icon): (gpui::Hsla, gpui::Hsla, gpui::Hsla, &str) = match att.level {
                AttentionLevel::ActiveEditing => (
                    theme::halo_active(),
                    theme::halo_active(),
                    theme::text().into(),
                    "⚡",
                ),
                AttentionLevel::Hot => (
                    theme::bg_elevated().into(),
                    theme::accent().into(),
                    theme::accent().into(),
                    "🔥",
                ),
                AttentionLevel::Warm => (
                    theme::bg_surface().into(),
                    theme::glass_highlight(),
                    theme::text_secondary().into(),
                    "📖",
                ),
                AttentionLevel::Cold => (
                    theme::bg_surface().into(),
                    theme::glass_highlight(),
                    theme::text_muted().into(),
                    "·",
                ),
            };

            let file_name = att.path.rsplit('/').next().unwrap_or(&att.path).to_string();
            let full_path = att.path.clone();

            row = row.child(
                h_flex()
                    .cursor_pointer()
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .hover(|s| s.border_color(theme::accent()))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.handle_context_menu_action(
                                ContextMenuAction::OpenFile(full_path.clone()),
                                cx,
                            );
                        }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .child(icon),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child(file_name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(format!("({})", att.active_quarks.join(", "))),
                    ),
            );
        }

        h_flex()
            .px_3()
            .py_1()
            .bg(theme::term_bg())
            .border_b_1()
            .border_color(theme::glass_highlight())
            .w_full()
            .child(row)
            .into_any_element()
    }
}
