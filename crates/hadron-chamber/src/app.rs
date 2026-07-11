//! GPUI window for the chamber. Behind the `gui` feature.
//!
//! Renders a `ChamberView` (the pure model projection) as two columns: the quark
//! roster on the left, the field transcript (chat) on the right. v1 is a static
//! snapshot loaded once — live tail + input are later tasks.

use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Window, WindowBounds,
    WindowOptions,
};
use hadron_lattice::{io, QuarkState};

use crate::model::{self, ChamberView, MessageRow, RosterRow};

struct Chamber {
    view: ChamberView,
}

impl Render for Chamber {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .child(roster_column(&self.view.roster))
            .child(chat_column(&self.view.messages))
    }
}

/// Left column: the roster, one row per quark with a state chip.
fn roster_column(roster: &[RosterRow]) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(220.0))
        .h_full()
        .p_3()
        .gap_2()
        .bg(rgb(0x181825))
        .child(div().text_sm().text_color(rgb(0x89b4fa)).child("Quarks"))
        .children(roster.iter().map(|r| {
            div()
                .flex()
                .flex_row()
                .justify_between()
                .child(r.id.clone())
                .child(
                    div()
                        .text_sm()
                        .text_color(state_color(r.state))
                        .child(state_label(r.state)),
                )
        }))
}

/// Right column: the field transcript.
fn chat_column(messages: &[MessageRow]) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .p_3()
        .gap_2()
        .child(div().text_sm().text_color(rgb(0x89b4fa)).child("Field"))
        .children(messages.iter().map(message_row))
}

fn message_row(m: &MessageRow) -> impl IntoElement {
    let header = match &m.to {
        Some(to) => format!("{} → {}  ·  {}", m.from, to, m.kind_label),
        None => format!("{}  ·  {}", m.from, m.kind_label),
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_sm().text_color(rgb(0x6c7086)).child(header))
        .child(div().child(m.body.clone()))
}

fn state_label(state: QuarkState) -> String {
    format!("{state:?}").to_lowercase()
}

fn state_color(state: QuarkState) -> gpui::Rgba {
    match state {
        QuarkState::Ground => rgb(0x6c7086),
        QuarkState::Excited | QuarkState::Thinking => rgb(0xa6e3a1),
        QuarkState::Waiting => rgb(0xf9e2af),
        QuarkState::Blocked | QuarkState::Error => rgb(0xf38ba8),
    }
}

/// True when running under WSL (WSLg). Used to work around WSLg's Wayland.
fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| {
            let s = s.to_lowercase();
            s.contains("microsoft") || s.contains("wsl")
        })
        .unwrap_or(false)
}

/// Launch the chamber window against a field file path.
pub fn run(field_path: Option<String>) {
    // WSLg's Wayland compositor version is not supported by gpui's Wayland
    // backend (it panics with `UnsupportedVersion`). On WSL, force the X11
    // backend (XWayland serves `DISPLAY`), which works. No effect elsewhere.
    if is_wsl() && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    let Some(path) = field_path else {
        eprintln!("usage: hadron-chamber <field.jsonl>");
        return;
    };
    let events = io::read_events(std::path::Path::new(&path)).unwrap_or_default();
    let view = model::project(&events);

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Chamber { view: view.clone() }),
        )
        .unwrap();
        cx.activate(true);
    });
}
