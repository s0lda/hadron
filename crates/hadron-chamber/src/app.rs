//! GPUI window for the chamber. Behind the `gui` feature.
//!
//! A 3-pane layout — collapsible Quarks rail (left), the field transcript
//! (center), a collapsible Inspector rail (right). Rail collapse state persists
//! across sessions via [`crate::config`]. Themed with [`crate::theme`].

use gpui::{
    div, prelude::*, px, App, Application, Bounds, Context, Decorations, MouseButton,
    TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowDecorations,
    WindowOptions,
};
use hadron_lattice::{io, QuarkState};

use crate::config::{self, ChamberPrefs};
use crate::model::{self, ChamberView, MessageRow, RosterRow};
use crate::theme;

struct Chamber {
    view: ChamberView,
    prefs: ChamberPrefs,
}

impl Render for Chamber {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Client-side decorations (borderless + rounded + our own dark titlebar)
        // only take effect where the platform supports them (Wayland/macOS). On
        // server-decorated platforms (incl. WSLg X11) the WM draws the frame, so
        // we skip our custom titlebar to avoid a double bar.
        let client = matches!(window.window_decorations(), Decorations::Client { .. });

        // A dark, draggable custom titlebar (only when we own the frame).
        let titlebar = div()
            .id("titlebar")
            .flex()
            .flex_row()
            .items_center()
            .h(px(38.0))
            .w_full()
            .px_4()
            .bg(theme::sidebar())
            .child(
                div()
                    .text_sm()
                    .text_color(theme::text_secondary())
                    .child("Hadron · Chamber"),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, window, _| window.start_window_move()),
            );

        let body = div()
            .flex()
            .flex_row()
            .flex_1()
            .child(self.roster_pane(cx))
            .child(self.chat_pane())
            .child(self.inspector_pane(cx));

        div()
            .id("root")
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .when(client, |r| r.rounded_lg().overflow_hidden())
            .when(client, |r| r.child(titlebar))
            .child(body)
    }
}

impl Chamber {
    fn roster_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.prefs.roster_collapsed;
        let toggle = div()
            .id("roster-toggle")
            .text_sm()
            .text_color(theme::text_muted())
            .child(if collapsed { ">".to_string() } else { "Quarks   <".to_string() })
            .active(|s| s.opacity(0.6))
            .on_click(cx.listener(|this, _, _, cx| {
                this.prefs.roster_collapsed = !this.prefs.roster_collapsed;
                let _ = config::save(&this.prefs);
                cx.notify();
            }));

        let mut col = div()
            .flex()
            .flex_col()
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::sidebar())
            .w(if collapsed { px(36.0) } else { px(self.prefs.roster_width) })
            .child(toggle);

        if !collapsed {
            for r in &self.view.roster {
                col = col.child(roster_row(r));
            }
            if self.view.roster.is_empty() {
                col = col.child(
                    div().text_sm().text_color(theme::text_muted()).child("no quarks yet"),
                );
            }
        }
        col
    }

    fn chat_pane(&self) -> impl IntoElement {
        let mut messages = div().flex().flex_col().flex_1().gap_3().p_4();
        messages = messages.child(
            div().text_sm().text_color(theme::text_muted()).child("FIELD"),
        );
        for m in &self.view.messages {
            messages = messages.child(message_row(m));
        }

        // Placeholder input bar (real typing is a follow-up; see Plan 4 Task 4).
        let input = div()
            .flex()
            .flex_row()
            .items_center()
            .m_4()
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(theme::input_bg())
            .child(
                div()
                    .text_color(theme::text_muted())
                    .child("type @quark a message…  (input coming soon)"),
            );

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(theme::bg())
            .child(messages)
            .child(input)
    }

    fn inspector_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.prefs.inspector_collapsed;
        let toggle = div()
            .id("inspector-toggle")
            .text_sm()
            .text_color(theme::text_muted())
            .child(if collapsed { "<".to_string() } else { ">   Inspector".to_string() })
            .active(|s| s.opacity(0.6))
            .on_click(cx.listener(|this, _, _, cx| {
                this.prefs.inspector_collapsed = !this.prefs.inspector_collapsed;
                let _ = config::save(&this.prefs);
                cx.notify();
            }));

        let mut col = div()
            .flex()
            .flex_col()
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::sidebar())
            .w(if collapsed { px(36.0) } else { px(self.prefs.inspector_width) })
            .child(toggle);

        if !collapsed {
            // v1 inspector: surface snapshot/edit/command activity from the field.
            let activity: Vec<&MessageRow> = self
                .view
                .messages
                .iter()
                .filter(|m| matches!(m.kind_label, "snapshot" | "edit" | "command"))
                .collect();
            col = col.child(
                div().text_sm().text_color(theme::text_muted()).child("Activity"),
            );
            if activity.is_empty() {
                col = col.child(
                    div().text_sm().text_color(theme::text_muted()).child("no changes yet"),
                );
            } else {
                for m in activity {
                    col = col.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme::surface())
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::accent_secondary())
                                    .child(m.kind_label.to_string()),
                            )
                            .child(div().text_sm().text_color(theme::text_secondary()).child(m.body.clone())),
                    );
                }
            }
        }
        col
    }
}

fn roster_row(r: &RosterRow) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .items_center()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(theme::surface())
        .child(div().child(r.id.clone()))
        .child(
            div()
                .text_sm()
                .text_color(theme::quark_state(r.state))
                .child(state_label(r.state)),
        )
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
        .child(div().text_sm().text_color(theme::actor_hue(&m.from)).child(header))
        .child(div().text_color(theme::text_secondary()).child(m.body.clone()))
}

fn state_label(state: QuarkState) -> String {
    format!("{state:?}").to_lowercase()
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
    let prefs = config::load();

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, gpui::size(px(1000.0), px(640.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // Ask for a frameless, transparent, client-decorated window so we
                // can draw our own dark titlebar with rounded corners. Falls back
                // to server decorations where unsupported (e.g. WSLg X11).
                titlebar: Some(TitlebarOptions {
                    title: Some("Hadron · Chamber".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                window_background: WindowBackgroundAppearance::Transparent,
                window_decorations: Some(WindowDecorations::Client),
                app_id: Some("dev.hadron.chamber".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Chamber { view: view.clone(), prefs: prefs.clone() }),
        )
        .unwrap();
        cx.activate(true);
    });
}
