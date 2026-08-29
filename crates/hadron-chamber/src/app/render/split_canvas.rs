#[allow(dead_code)]
pub struct SplitCanvasState {
    active_quarks: Vec<String>,
    focused_idx: usize,
}

#[allow(dead_code)]
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
