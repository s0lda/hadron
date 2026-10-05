use std::path::PathBuf;
use gpui::*;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewPanelState {
    pub current_url: String,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub active_port: Option<u16>,
    pub last_capture_path: Option<PathBuf>,
}

impl Default for PreviewPanelState {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl PreviewPanelState {
    pub fn new() -> Self {
        Self {
            current_url: "http://127.0.0.1:3000".to_string(),
            viewport_width: 1280.0,
            viewport_height: 800.0,
            active_port: None,
            last_capture_path: None,
        }
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport_width = width;
        self.viewport_height = height;
    }

    pub fn attach_port(&mut self, port: u16) {
        self.active_port = Some(port);
        self.current_url = format!("http://127.0.0.1:{port}");
    }
}

pub fn render_preview_panel(state: &PreviewPanelState) -> impl IntoElement {
    div()
        .id("preview-panel")
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .text_sm()
                .child(format!("Preview: {} ({}x{})", state.current_url, state.viewport_width, state.viewport_height))
        )
}

impl super::Chamber {
    pub(super) fn preview_view(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let state = PreviewPanelState::new();
        render_preview_panel(&state)
    }
}

#[cfg(test)]
mod tests {
    use super::PreviewPanelState;

    #[test]
    fn test_preview_panel_viewport_and_url_state() {
        let mut state = PreviewPanelState::new();
        assert_eq!(state.current_url, "http://127.0.0.1:3000");

        state.set_viewport(375.0, 667.0);
        assert_eq!(state.viewport_width, 375.0);
        assert_eq!(state.viewport_height, 667.0);

        state.attach_port(8080);
        assert_eq!(state.current_url, "http://127.0.0.1:8080");
    }
}
