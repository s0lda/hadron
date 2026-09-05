//! Live Interactive GPUI Canvas Preview.
//!
//! Provides coordinate state management and vector primitives for rendering interactive
//! agent-generated wireframes, mermaid graph layouts, and live UI prototypes.

use serde::{Deserialize, Serialize};

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct InteractiveCanvasState {
    pub elements: Vec<CanvasElement>,
}

#[allow(dead_code)]
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
