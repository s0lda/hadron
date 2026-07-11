//! GPUI window for the chamber. Behind the `gui` feature.
//!
//! Built on the git gpui stack + gpui-component: a frameless, dark, client-decorated
//! window with a custom `TitleBar`, and a 3-pane body built from gpui-component's
//! `Resizable` panels — a Quarks rail, the field chat (grows), and an Inspector rail.
//! Both rails drag-resize and collapse; a single shared [`ResizableState`] drives
//! both gestures, and widths + collapse state persist via [`crate::config`].

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, App, Context, Entity, Render, Subscription, Window, WindowBounds,
    WindowDecorations, WindowOptions,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::resizable::{h_resizable, resizable_panel, ResizableState};
use gpui_component::{h_flex, v_flex, Root, Theme, ThemeMode, TitleBar};
use hadron_lattice::{io, Actor, Event, Kind, QuarkState};

use crate::config::{self, ChamberPrefs};
use crate::model::{self, ChamberView, MessageRow, RosterRow};
use crate::theme;

/// Panel indices within the body's [`ResizableState`] (roster · chat · inspector).
const ROSTER_IX: usize = 0;
const INSPECTOR_IX: usize = 2;
/// Width a rail shrinks to when collapsed — just enough for the toggle.
const RAIL_COLLAPSED: f32 = 36.0;

struct Chamber {
    view: ChamberView,
    prefs: ChamberPrefs,
    /// The field file this chamber reads from and steers into.
    path: PathBuf,
    /// Shared sizing state for the 3-pane body. Drag handles mutate it, and the
    /// collapse toggles drive it programmatically, so both gestures agree.
    resize_state: Entity<ResizableState>,
    /// The human's message box at the foot of the chat column.
    input: Entity<InputState>,
    /// Keeps the input-submit subscription alive for the window's lifetime.
    _input_sub: Subscription,
}

impl Chamber {
    fn new(
        view: ChamberView,
        prefs: ChamberPrefs,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let resize_state = cx.new(|_| ResizableState::default());
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 4)
                .submit_on_enter(true)
                .placeholder("Type @quark a message…  (Enter to send · Shift+Enter for newline)")
        });
        let _input_sub = cx.subscribe_in(&input, window, Self::on_input_submit);

        // Live tail: re-read the field on an interval so quark turns appended by
        // the gluon (a separate process) appear without interaction. Dumb full
        // re-read — the field is small and this matches the engine's own posture.
        // The loop ends when the entity is dropped (`update` returns `Err`).
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(400)).await;
                if this.update(cx, |chamber, cx| chamber.reload_if_changed(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        Chamber { view, prefs, path, resize_state, input, _input_sub }
    }

    /// Re-read the field; if it grew, re-project and repaint. Comparing event
    /// count to the current row count is a cheap change check (projection emits
    /// exactly one row per event), so an unchanged field costs only a read.
    fn reload_if_changed(&mut self, cx: &mut Context<Self>) {
        // Only reproject on a successful read — a transient read error must not
        // blank the current view (which would flash to empty, then repopulate).
        if let Ok(events) = io::read_events(&self.path) {
            if events.len() != self.view.messages.len() {
                self.view = model::project(&events);
                cx.notify();
            }
        }
    }

    /// Submit the human's message on Enter (Shift+Enter inserts a newline).
    /// Appends an `Actor::Human` event to the field — the same bus the quarks
    /// use — then re-reads and re-projects so the new row appears immediately.
    fn on_input_submit(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let InputEvent::PressEnter { shift, .. } = event else {
            return;
        };
        if *shift {
            return;
        }
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }

        let (to, body) = model::parse_mention(&text);
        let ev = Event::new(Actor::Human, to, Kind::Message { body });
        if let Err(e) = io::append_event(&self.path, &ev) {
            eprintln!("chamber: failed to append steering message: {e}");
            return;
        }

        input.update(cx, |state, cx| state.set_value("", window, cx));
        let events = io::read_events(&self.path).unwrap_or_default();
        self.view = model::project(&events);
        cx.notify();
    }
}

impl Render for Chamber {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .child(self.titlebar(cx))
            .child(self.body(cx))
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

    /// The 3-pane body: two collapsible/resizable rails around the field chat.
    fn body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        // Captured for the drag-persist callback below (see `on_resize`).
        let chamber = cx.entity();

        let roster = resizable_panel()
            .size(px(if self.prefs.roster_collapsed {
                RAIL_COLLAPSED
            } else {
                self.prefs.roster_width
            }))
            .size_range(px(RAIL_COLLAPSED)..px(480.0))
            .child(self.roster_pane(cx));

        let chat = resizable_panel().child(self.chat_pane());

        let inspector = resizable_panel()
            .size(px(if self.prefs.inspector_collapsed {
                RAIL_COLLAPSED
            } else {
                self.prefs.inspector_width
            }))
            .size_range(px(RAIL_COLLAPSED)..px(560.0))
            .child(self.inspector_pane(cx));

        h_resizable("chamber-body")
            .with_state(&self.resize_state)
            // Fires when the user finishes dragging a handle. Persist the new
            // rail widths — but not while a rail is collapsed, so its remembered
            // expanded width survives a drag of the *other* handle.
            .on_resize(move |state, _window, app| {
                let sizes = state.read(app).sizes().clone();
                chamber.update(app, |this, _cx| {
                    if !this.prefs.roster_collapsed {
                        if let Some(w) = sizes.first() {
                            this.prefs.roster_width = w.as_f32();
                        }
                    }
                    if !this.prefs.inspector_collapsed {
                        if let Some(w) = sizes.last() {
                            this.prefs.inspector_width = w.as_f32();
                        }
                    }
                    let _ = config::save(&this.prefs);
                });
            })
            .child(roster)
            .child(chat)
            .child(inspector)
    }

    fn roster_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.prefs.roster_collapsed;
        let toggle = div()
            .id("roster-toggle")
            .text_sm()
            .text_color(theme::text_muted())
            .child(if collapsed { ">".to_string() } else { "Quarks   <".to_string() })
            .active(|s| s.opacity(0.6))
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_rail(ROSTER_IX, window, cx);
            }));

        let mut col = v_flex()
            .w_full()
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::sidebar())
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
            .m_4()
            .px_1()
            .rounded_lg()
            .bg(theme::input_bg())
            .child(Input::new(&self.input));

        v_flex().w_full().h_full().bg(theme::bg()).child(messages).child(input)
    }

    fn inspector_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.prefs.inspector_collapsed;
        let toggle = div()
            .id("inspector-toggle")
            .text_sm()
            .text_color(theme::text_muted())
            .child(if collapsed { "<".to_string() } else { ">   Inspector".to_string() })
            .active(|s| s.opacity(0.6))
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_rail(INSPECTOR_IX, window, cx);
            }));

        let mut col = v_flex()
            .w_full()
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::sidebar())
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

    /// Collapse or expand a rail. Flips the persisted flag and drives the shared
    /// `ResizableState` to the target width — collapsing to a sliver, or restoring
    /// the remembered expanded width. Programmatic resizes don't fire `on_resize`,
    /// so the remembered width in `prefs` is never clobbered by the collapse itself.
    fn toggle_rail(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (collapsed_flag, remembered) = match ix {
            ROSTER_IX => {
                self.prefs.roster_collapsed = !self.prefs.roster_collapsed;
                (self.prefs.roster_collapsed, self.prefs.roster_width)
            }
            _ => {
                self.prefs.inspector_collapsed = !self.prefs.inspector_collapsed;
                (self.prefs.inspector_collapsed, self.prefs.inspector_width)
            }
        };
        let target = px(if collapsed_flag { RAIL_COLLAPSED } else { remembered });
        self.resize_state
            .update(cx, |state, cx| state.resize_panel(ix, target, window, cx));
        let _ = config::save(&self.prefs);
        cx.notify();
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
    let field_path = PathBuf::from(&path);
    let events = io::read_events(&field_path).unwrap_or_default();
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
                let chamber = cx.new(|cx| {
                    Chamber::new(view.clone(), prefs.clone(), field_path.clone(), window, cx)
                });
                cx.new(|cx| Root::new(chamber, window, cx).bordered(false))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
