use super::*;

pub struct SplitCanvasState {
    active_quarks: Vec<String>,
    focused_idx: usize,
}

impl SplitCanvasState {
    pub fn new() -> Self {
        Self {
            active_quarks: Vec::new(),
            focused_idx: 0,
        }
    }

    pub fn add_quark(&mut self, quark_id: &str) {
        if !self.active_quarks.iter().any(|q| q == quark_id) {
            self.active_quarks.push(quark_id.to_string());
        }
    }

    pub fn active_tiles(&self) -> &[String] {
        &self.active_quarks
    }

    pub fn focused_index(&self) -> usize {
        self.focused_idx
    }

    pub fn calculate_columns(&self) -> usize {
        match self.active_quarks.len() {
            0 | 1 => 1,
            2..=4 => 2,
            _ => 3,
        }
    }

    pub fn cycle_focus(&mut self) {
        if !self.active_quarks.is_empty() {
            self.focused_idx = (self.focused_idx + 1) % self.active_quarks.len();
        }
    }
}

impl Chamber {
    /// Renders the Multi-Quark Live Split-Canvas Grid & Interactive Wireframe Preview.
    pub(super) fn render_split_canvas_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.split_canvas.active_tiles();
        let cols = self.split_canvas.calculate_columns();
        let focused_idx = self.split_canvas.focused_index();

        let mut grid_rows = v_flex().gap_2().w_full();
        if tiles.is_empty() {
            grid_rows = grid_rows.child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("No active split-canvas tiles yet. Run `/canvas` or click Sync Roster below."),
            );
        } else {
            for (chunk_ix, chunk) in tiles.chunks(cols.max(1)).enumerate() {
                let mut row = h_flex().gap_2().w_full();
                for (offset, qid) in chunk.iter().enumerate() {
                    let tile_ix = chunk_ix * cols.max(1) + offset;
                    let is_focused = tile_ix == focused_idx;
                    let identity = self.resolve_identity(qid);
                    let activity_detail = self
                        .last_live_activities
                        .get(qid)
                        .and_then(|a| a.as_ref())
                        .map(|a| format!("{}: {}", a.doing.label(), a.detail))
                        .unwrap_or_else(|| "Ready · Idle".to_string());

                    row = row.child(
                        v_flex()
                            .id(SharedString::from(format!("split-canvas-tile-{qid}")))
                            .flex_1()
                            .min_w_0()
                            .p_2()
                            .rounded_md()
                            .bg(if is_focused {
                                theme::bg_elevated()
                            } else {
                                theme::bg_surface()
                            })
                            .border_1()
                            .border_color(if is_focused {
                                theme::accent().into()
                            } else {
                                theme::glass_highlight()
                            })
                            .gap_1()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        h_flex()
                                            .gap_1p5()
                                            .items_center()
                                            .child(
                                                div()
                                                    .size(px(6.0))
                                                    .rounded_full()
                                                    .bg(identity.color),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(identity.color)
                                                    .truncate()
                                                    .child(identity.name),
                                            ),
                                    )
                                    .when(is_focused, |el| {
                                        el.child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(theme::accent().opacity(0.18))
                                                .text_xs()
                                                .text_color(theme::accent())
                                                .child("FOCUSED"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_secondary())
                                    .truncate()
                                    .child(activity_detail),
                            ),
                    );
                }
                grid_rows = grid_rows.child(row);
            }
        }

        v_flex()
            .w_full()
            .p_3()
            .gap_2()
            .bg(theme::bg_surface())
            .border_t_1()
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
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text())
                                    .child(format!(
                                        "Live Split-Canvas ({} tiles · {} cols)",
                                        tiles.len(),
                                        cols
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                div()
                                    .id("split-canvas-sync-btn")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(theme::tab_bar_bg())
                                    .border_1()
                                    .border_color(theme::glass_highlight())
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .hover(|s| s.text_color(theme::text()))
                                    .child("Sync Roster")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let ids: Vec<String> = this
                                            .view
                                            .roster
                                            .iter()
                                            .filter(|r| r.enabled && r.adopted)
                                            .map(|r| r.id.clone())
                                            .collect();
                                        for id in ids {
                                            this.split_canvas.add_quark(&id);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("split-canvas-cycle-focus")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(theme::tab_bar_bg())
                                    .border_1()
                                    .border_color(theme::glass_highlight())
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .hover(|s| s.text_color(theme::text()))
                                    .child("Cycle Focus")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.split_canvas.cycle_focus();
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(grid_rows)
            .child(self.render_interactive_canvas_preview(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_canvas_grid_geometry() {
        let mut canvas = SplitCanvasState::new();
        canvas.add_quark("quark-alpha");
        canvas.add_quark("quark-beta");
        canvas.add_quark("quark-gamma");

        assert_eq!(canvas.active_tiles().len(), 3);
        assert_eq!(canvas.calculate_columns(), 2);
    }
}
