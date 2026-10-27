//! Live Interactive GPUI Canvas Preview.
//!
//! Provides coordinate state management and vector primitives for rendering interactive
//! agent-generated wireframes, mermaid graph layouts, and live UI prototypes.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasElement {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label: String,
    pub fill_color: String,
}

impl CanvasElement {
    pub fn new(
        id: impl Into<String>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        label: impl Into<String>,
        fill_color: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            x,
            y,
            width,
            height,
            label: label.into(),
            fill_color: fill_color.into(),
        }
    }

    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct InteractiveCanvasState {
    pub elements: Vec<CanvasElement>,
}

impl InteractiveCanvasState {
    pub fn new() -> Self {
        Self {
            elements: Vec::new(),
        }
    }

    pub fn add_element(&mut self, element: CanvasElement) {
        self.elements.push(element);
    }

    pub fn remove_element(&mut self, id: &str) -> Option<CanvasElement> {
        if let Some(pos) = self.elements.iter().position(|e| e.id == id) {
            Some(self.elements.remove(pos))
        } else {
            None
        }
    }

    pub fn hit_test(&self, px: f32, py: f32) -> Option<&CanvasElement> {
        self.elements.iter().rev().find(|e| e.contains_point(px, py))
    }

    pub fn clear(&mut self) {
        self.elements.clear();
    }
}

impl Chamber {
    /// Renders the Interactive Canvas Wireframe Preview strip inside the Visualizer rail.
    pub(super) fn render_interactive_canvas_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.interactive_canvas.elements.is_empty() {
            return div().into_any_element();
        }

        let center_hit = self
            .interactive_canvas
            .hit_test(400.0, 300.0)
            .map(|e| e.label.clone())
            .unwrap_or_else(|| "None".to_string());

        let mut pills = h_flex().gap_1p5().flex_wrap().w_full();
        for elem in &self.interactive_canvas.elements {
            let elem_id = elem.id.clone();
            pills = pills.child(
                h_flex()
                    .id(SharedString::from(format!("canvas-elem-pill-{elem_id}")))
                    .items_center()
                    .gap_1p5()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .bg(theme::term_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::accent())
                            .child(elem.label.clone()),
                    )
                    .child(
                        div()
                            .text_color(theme::text_muted())
                            .child(format!("{:.0}x{:.0}", elem.width, elem.height)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("canvas-remove-{elem_id}")))
                            .cursor_pointer()
                            .px_1()
                            .text_color(theme::text_muted())
                            .hover(|s| s.text_color(theme::error()))
                            .child("×")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.interactive_canvas.remove_element(&elem_id);
                                cx.notify();
                            })),
                    ),
            );
        }

        v_flex()
            .w_full()
            .p_2()
            .rounded_md()
            .bg(theme::bg_base())
            .border_1()
            .border_color(theme::glass_highlight())
            .gap_1p5()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .text_xs()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text_secondary())
                            .child(format!(
                                "Interactive Wireframe Elements ({}) · Center Hit: {center_hit}",
                                self.interactive_canvas.elements.len()
                            )),
                    )
                    .child(
                        div()
                            .id("canvas-clear-all-btn")
                            .cursor_pointer()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(theme::bg_surface())
                            .text_color(theme::text_muted())
                            .hover(|s| s.text_color(theme::text()))
                            .child("Clear Canvas")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.interactive_canvas.clear();
                                cx.notify();
                            })),
                    ),
            )
            .child(pills)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interactive_canvas_state() {
        let mut canvas = InteractiveCanvasState::new();
        let box1 = CanvasElement::new("box1", 10.0, 10.0, 100.0, 50.0, "Button", "#4285F4");
        canvas.add_element(box1);

        assert_eq!(canvas.elements.len(), 1);
        assert!(canvas.hit_test(15.0, 15.0).is_some());
        assert!(canvas.hit_test(200.0, 200.0).is_none());

        let removed = canvas.remove_element("box1").unwrap();
        assert_eq!(removed.id, "box1");
        assert!(canvas.elements.is_empty());

        canvas.add_element(CanvasElement::new("b2", 0.0, 0.0, 10.0, 10.0, "B2", "#fff"));
        assert_eq!(canvas.elements.len(), 1);
        canvas.clear();
        assert!(canvas.elements.is_empty());
    }
}
