//! GPUI window for the chamber. Behind the `gui` feature.
//!
//! Built on the git gpui stack + gpui-component: a frameless, dark, client-decorated
//! window with a custom `TitleBar`, and a 3-pane body — collapsible Quarks rail,
//! field chat, collapsible Inspector rail. Rail state persists via [`crate::config`].

use gpui::{
    div, prelude::*, px, App, Context, Render, Window, WindowBounds, WindowDecorations,
    WindowOptions,
};
use gpui_component::{h_flex, v_flex, Root, Theme, ThemeMode, TitleBar};
use hadron_lattice::{io, QuarkState};

use crate::config::{self, ChamberPrefs};
use crate::model::{self, ChamberView, MessageRow, RosterRow};
use crate::theme;

struct Chamber {
    view: ChamberView,
    prefs: ChamberPrefs,
}

impl Render for Chamber {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .child(self.titlebar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .w_full()
                    .child(self.roster_pane(cx))
                    .child(self.chat_pane())
                    .child(self.inspector_pane(cx)),
            )
    }
}

impl Chamber {
    fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // A modern, centered command bar (Ctrl+Shift+P). TitleBar renders the
        // window controls on the right and handles dragging.
        let command_bar = h_flex()
            .id("command-bar")
            .items_center()
            .gap_3()
            .px_3()
            .py_1()
            .w(px(380.0))
            .rounded_md()
            .bg(theme::surface())
            .text_sm()
            .text_color(theme::text_muted())
            .hover(|s| s.bg(theme::surface_raised()))
            .active(|s| s.opacity(0.85))
            .child(div().child("Run a command…"))
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("Ctrl ⇧ P"),
            )
            .on_click(cx.listener(|_this, _, _window, _cx| {
                // TODO: open the command palette (commands TBD with Jake).
            }));

        TitleBar::new().child(
            h_flex()
                .w_full()
                .items_center()
                .child(div().flex_1())
                .child(command_bar)
                .child(div().flex_1()),
        )
    }

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

        let mut col = v_flex()
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
        let mut messages = v_flex().flex_1().gap_3().p_4().child(
            div().text_sm().text_color(theme::text_muted()).child("FIELD"),
        );
        for m in &self.view.messages {
            messages = messages.child(message_row(m));
        }

        let input = h_flex()
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

        v_flex().flex_1().h_full().bg(theme::bg()).child(messages).child(input)
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

        let mut col = v_flex()
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::sidebar())
            .w(if collapsed { px(36.0) } else { px(self.prefs.inspector_width) })
            .child(toggle);

        if !collapsed {
            let activity: Vec<&MessageRow> = self
                .view
                .messages
                .iter()
                .filter(|m| matches!(m.kind_label, "snapshot" | "edit" | "command"))
                .collect();
            col = col
                .child(div().text_sm().text_color(theme::text_muted()).child("Activity"));
            if activity.is_empty() {
                col = col.child(
                    div().text_sm().text_color(theme::text_muted()).child("no changes yet"),
                );
            } else {
                for m in activity {
                    col = col.child(
                        v_flex()
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
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_secondary())
                                    .child(m.body.clone()),
                            ),
                    );
                }
            }
        }
        col
    }
}

fn roster_row(r: &RosterRow) -> impl IntoElement {
    h_flex()
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
    v_flex()
        .gap_1()
        .child(div().text_sm().text_color(theme::actor_hue(&m.from)).child(header))
        .child(div().text_color(theme::text_secondary()).child(m.body.clone()))
}

fn state_label(state: QuarkState) -> String {
    format!("{state:?}").to_lowercase()
}

/// Launch the chamber window against a field file path.
pub fn run(field_path: Option<String>) {
    let Some(path) = field_path else {
        eprintln!("usage: hadron-chamber <field.jsonl>");
        return;
    };
    let events = io::read_events(std::path::Path::new(&path)).unwrap_or_default();
    let view = model::project(&events);
    let prefs = config::load();

    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        // Build window options here (needs `&App`, not the async cx below).
        let window_options = WindowOptions {
            titlebar: Some(TitleBar::title_bar_options()),
            window_decorations: Some(WindowDecorations::Client),
            window_bounds: Some(WindowBounds::centered(
                gpui::size(px(1000.0), px(640.0)),
                cx,
            )),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, move |window, cx| {
                let chamber = cx.new(|_| Chamber { view: view.clone(), prefs: prefs.clone() });
                cx.new(|cx| Root::new(chamber, window, cx).bordered(false))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
