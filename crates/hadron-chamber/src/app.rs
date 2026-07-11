//! GPUI window for the chamber. Behind the `gui` feature.
//!
//! FOUNDATION-VALIDATION STUB: minimal window on the git gpui + gpui-component
//! stack. Once this builds and runs frameless in the target environment, the full
//! roster/chat/inspector UI (see git history at the pre-migration commit) is
//! ported onto gpui-component's TitleBar / Dock / Input.

use gpui::{div, prelude::*, rgb, App, Context, Render, Window, WindowOptions};
use gpui_component::Root;

struct Chamber;

impl Render for Chamber {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x0d0e11))
            .text_color(rgb(0xf5f5f6))
            .p_8()
            .child("Hadron · Chamber — gpui-component foundation OK")
    }
}

/// Launch the chamber window against a field file path.
pub fn run(_field_path: Option<String>) {
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| Chamber);
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
