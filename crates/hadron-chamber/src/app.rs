//! GPUI window for the chamber. Behind the `gui` feature.
//!
//! Built on the git gpui stack + gpui-component: a transparent, dark,
//! client-decorated window inside our own rounded, shadowed [`crate::window_frame`],
//! with a custom `TitleBar` and a 3-pane body — a Quarks rail, the field chat
//! (grows), and an Inspector rail. Expanded rails are gpui-component `Resizable`
//! panels (drag-resize); a collapsed rail is a fixed strip outside the group.
//! Widths + collapse state persist via [`crate::config`].

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    actions, div, prelude::*, px, rgb, rgba, App, Context, Entity, FocusHandle, KeyBinding,
    MouseButton, Render, SharedString, Subscription, Window, WindowBackgroundAppearance,
    WindowBounds, WindowControlArea, WindowDecorations, WindowOptions,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::avatar::Avatar;
use gpui_component::badge::Badge;
use gpui_component::tooltip::Tooltip;
use gpui_component::{
    h_flex, v_flex, Icon, IconName, Root, Sizable, Size, Theme, ThemeMode, TitleBar,
};
use hadron_lattice::{io, Actor, Event, Kind};

use crate::config::{self, ChamberPrefs};
use crate::model::{self, ChamberView, MessageRow, RosterRow};
use crate::theme;

actions!(chamber, [TogglePalette]);

/// Key-dispatch context for the chamber's window-level actions.
const KEY_CONTEXT: &str = "Chamber";

/// Width of a collapsed rail's strip (just the expand affordance).
const RAIL_STRIP: f32 = 44.0;
/// Drag-resize bounds for an expanded rail.
const RAIL_MIN: f32 = 160.0;
const RAIL_MAX: f32 = 440.0;

/// The two collapsible side rails.
#[derive(Clone, Copy)]
enum Rail {
    Roster,
    Inspector,
}

/// Commands offered by the Ctrl+Shift+P palette. v1: the two rail toggles.
#[derive(Clone, Copy)]
enum PaletteCmd {
    ToggleRoster,
    ToggleInspector,
}

impl PaletteCmd {
    const ALL: [PaletteCmd; 2] = [PaletteCmd::ToggleRoster, PaletteCmd::ToggleInspector];

    /// Label reflects the current rail state, so the verb matches what will happen.
    fn label(self, prefs: &ChamberPrefs) -> &'static str {
        match self {
            PaletteCmd::ToggleRoster => {
                if prefs.roster_collapsed {
                    "Show Quarks rail"
                } else {
                    "Hide Quarks rail"
                }
            }
            PaletteCmd::ToggleInspector => {
                if prefs.inspector_collapsed {
                    "Show Terminal"
                } else {
                    "Hide Terminal"
                }
            }
        }
    }
}

struct Chamber {
    view: ChamberView,
    prefs: ChamberPrefs,
    /// The field file this chamber reads from and steers into.
    path: PathBuf,
    /// The human's message box at the foot of the chat column.
    input: Entity<InputState>,
    /// Root focus target, so Ctrl+Shift+P dispatches regardless of what's focused.
    focus_handle: FocusHandle,
    /// Whether the Ctrl+Shift+P command palette overlay is showing.
    palette_open: bool,
    /// The palette's filter box.
    palette_input: Entity<InputState>,
    /// Keep the two input subscriptions alive for the window's lifetime.
    _input_sub: Subscription,
    _palette_sub: Subscription,
}

impl Chamber {
    fn new(
        view: ChamberView,
        prefs: ChamberPrefs,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 4)
                .submit_on_enter(true)
                .placeholder("Type @quark a message…  (Enter to send · Shift+Enter for newline)")
        });
        let _input_sub = cx.subscribe_in(&input, window, Self::on_input_submit);

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let palette_input = cx.new(|cx| {
            InputState::new(window, cx)
                .submit_on_enter(true)
                .placeholder("Run a command…")
        });
        let _palette_sub = cx.subscribe_in(&palette_input, window, Self::on_palette_submit);

        // Live tail: re-read the field on an interval so quark turns appended by
        // the gluon (a separate process) appear without interaction. Dumb full
        // re-read — the field is small and this matches the engine's own posture.
        // The loop ends when the entity is dropped (`update` returns `Err`).
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            if this
                .update(cx, |chamber, cx| chamber.reload_if_changed(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();

        Chamber {
            view,
            prefs,
            path,
            input,
            focus_handle,
            palette_open: false,
            palette_input,
            _input_sub,
            _palette_sub,
        }
    }

    /// Toggle the command palette (Ctrl+Shift+P, or the titlebar bar). Opening
    /// clears + focuses the filter box; closing returns focus to the root.
    fn on_toggle_palette(
        &mut self,
        _: &TogglePalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = !self.palette_open;
        if self.palette_open {
            self.palette_input.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    /// Run the top match when the human presses Enter in the palette filter.
    fn on_palette_submit(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::PressEnter { .. }) {
            return;
        }
        let query = input.read(cx).value().to_lowercase();
        if let Some(&cmd) = self.filtered_commands(&query).first() {
            self.run_command(cmd, window, cx);
        }
    }

    /// Commands whose label contains the (lowercased) query.
    fn filtered_commands(&self, query: &str) -> Vec<PaletteCmd> {
        PaletteCmd::ALL
            .into_iter()
            .filter(|c| c.label(&self.prefs).to_lowercase().contains(query))
            .collect()
    }

    /// Execute a palette command and dismiss the overlay.
    fn run_command(&mut self, cmd: PaletteCmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            PaletteCmd::ToggleRoster => self.toggle_rail(Rail::Roster, window, cx),
            PaletteCmd::ToggleInspector => self.toggle_rail(Rail::Inspector, window, cx),
        }
        self.palette_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The command-palette overlay: a dim backdrop (click to dismiss) behind a
    /// centered box with the filter input and the matching commands.
    fn palette_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.palette_input.read(cx).value().to_lowercase();
        let cmds = self.filtered_commands(&query);

        let mut list = v_flex().gap_1().mt_2();
        if cmds.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .child("no matching command"),
            );
        } else {
            for (i, cmd) in cmds.into_iter().enumerate() {
                let top = i == 0; // the Enter target
                list = list.child(
                    div()
                        .id(("palette-cmd", i))
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .bg(if top {
                            theme::surface_raised()
                        } else {
                            theme::surface()
                        })
                        .hover(|s| s.bg(theme::surface_raised()))
                        .active(|s| s.opacity(0.8))
                        .child(cmd.label(&self.prefs).to_string())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_command(cmd, window, cx)
                        })),
                );
            }
        }

        div()
            .id("palette-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .bg(rgba(0x00000066))
            .on_click(cx.listener(|this, _, window, cx| {
                this.palette_open = false;
                window.focus(&this.focus_handle, cx);
                cx.notify();
            }))
            .child(
                v_flex()
                    .occlude()
                    .mt(px(96.0))
                    .w(px(480.0))
                    .p_2()
                    .rounded_lg()
                    .bg(theme::sidebar())
                    .border_1()
                    .border_color(theme::border())
                    .child(Input::new(&self.palette_input))
                    .child(list),
            )
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let titlebar = self.titlebar(window, cx);
        let body = self.body(cx);
        let status = self.status_bar();
        let overlay = self.palette_open.then(|| self.palette_overlay(cx));

        let content = v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_toggle_palette))
            .relative()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .child(titlebar)
            .child(body)
            .child(status)
            .children(overlay);

        // Wrap in our transparent, rounded, shadowed client-side frame.
        crate::window_frame::window_frame(window, cx, content)
    }
}

impl Chamber {
    /// Our own titlebar: a centered command bar (Ctrl+Shift+P), draggable side
    /// regions, and custom min/max/close controls with *circular* hover — Zed-like,
    /// and a circle can't poke a square corner past the rounded frame.
    fn titlebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let command_bar = h_flex()
            .id("command-bar")
            .items_center()
            .gap_3()
            .my_2()
            .px_3()
            .py_1p5()
            .w(px(380.0))
            .rounded_md()
            .bg(theme::bg()) // darker than the titlebar → a recessed search field
            .text_sm()
            .text_color(theme::text_muted())
            .hover(|s| s.bg(theme::surface()))
            .active(|s| s.opacity(0.85))
            // Don't let a press on the bar start a window move.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().child("Run a command…"))
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("Ctrl ⇧ P"),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.on_toggle_palette(&TogglePalette, window, cx);
            }));

        let controls = h_flex()
            .items_center()
            .gap_1()
            .pr(px(8.0))
            .flex_shrink_0()
            // App/options menu (placeholder — the 3-line menu; options land later).
            .child(menu_button())
            .child(div().w(px(6.0)))
            .child(control_button("min", IconName::WindowMinimize, false))
            .child(control_button(
                "max",
                if window.is_maximized() {
                    IconName::WindowRestore
                } else {
                    IconName::WindowMaximize
                },
                false,
            ))
            .child(control_button("close", IconName::WindowClose, true));

        h_flex()
            .id("titlebar")
            .h(px(40.0))
            .w_full()
            .flex_none()
            .items_center()
            .bg(theme::sidebar())
            .border_b_1()
            .border_color(theme::border())
            .child(drag_region("drag-l"))
            .child(command_bar)
            .child(
                h_flex()
                    .flex_1()
                    .h_full()
                    .items_center()
                    .justify_end()
                    .child(drag_region("drag-r"))
                    .child(controls),
            )
    }

    /// The status bar along the foot of the window (same tone as the titlebar).
    /// Placeholder content for now.
    fn status_bar(&self) -> impl IntoElement {
        h_flex()
            .w_full()
            .h(px(24.0))
            .flex_none()
            .items_center()
            .justify_between()
            .px_3()
            .bg(theme::sidebar())
            .border_t_1()
            .border_color(theme::border())
            .text_xs()
            .text_color(theme::text_muted())
            .child(div().child("ready"))
            .child(div().child(format!("{} quark(s)", self.view.roster.len())))
    }

    /// The body: two collapsible rails around the field chat. Only *expanded*
    /// rails live in the resizable group (so a collapsed rail can't be dragged);
    /// a collapsed rail is a fixed strip outside the group. The group is re-keyed
    /// per layout so a fresh sizing state seeds panel widths from prefs, and
    /// `on_resize` persists new widths back — drag-resize without the fighting.
    fn body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let roster_collapsed = self.prefs.roster_collapsed;
        let inspector_collapsed = self.prefs.inspector_collapsed;
        let chamber = cx.entity();

        let group_id = SharedString::from(format!(
            "chamber-body-{}-{}",
            roster_collapsed as u8, inspector_collapsed as u8
        ));

        let mut group = h_resizable(group_id).on_resize(move |state, _window, app| {
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
        });

        if !roster_collapsed {
            group = group.child(
                resizable_panel()
                    .size(px(self.prefs.roster_width))
                    .size_range(px(RAIL_MIN)..px(RAIL_MAX))
                    .child(self.roster_pane(cx)),
            );
        }
        group = group.child(resizable_panel().child(self.chat_pane()));
        if !inspector_collapsed {
            group = group.child(
                resizable_panel()
                    .size(px(self.prefs.inspector_width))
                    .size_range(px(RAIL_MIN)..px(RAIL_MAX))
                    .child(self.terminal_pane(cx)),
            );
        }

        h_flex()
            .flex_1()
            .w_full()
            .when(roster_collapsed, |this| {
                this.child(self.rail_strip(Rail::Roster, cx))
            })
            .child(group)
            .when(inspector_collapsed, |this| {
                this.child(self.rail_strip(Rail::Inspector, cx))
            })
    }

    /// A collapsed rail: a fixed vertical strip with just the expand affordance
    /// (and, on the Quarks rail, the pinned Settings button).
    fn rail_strip(&self, rail: Rail, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, icon) = match rail {
            Rail::Roster => ("roster-strip", IconName::PanelLeftOpen),
            Rail::Inspector => ("inspector-strip", IconName::PanelRightOpen),
        };
        let mut col = v_flex()
            .id(id)
            .h_full()
            .w(px(RAIL_STRIP))
            .flex_none()
            .py_2()
            .items_center()
            .gap_2()
            .bg(theme::sidebar())
            .child(
                div()
                    .id("expand")
                    .text_color(theme::text_muted())
                    .child(Icon::new(icon).small())
                    .active(|s| s.opacity(0.6))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.toggle_rail(rail, window, cx)),
                    ),
            );
        if let Rail::Roster = rail {
            col = col
                .child(div().flex_1())
                .child(self.settings_button(cx, true));
        }
        col
    }

    /// The Settings entry pinned to the foot of the Quarks rail. Placeholder for
    /// now — content lands here as it's built out.
    fn settings_button(&self, cx: &mut Context<Self>, icon_only: bool) -> impl IntoElement {
        div()
            .id("settings")
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded_md()
            .text_sm()
            .text_color(theme::text_muted())
            .hover(|s| s.bg(theme::surface()))
            .active(|s| s.opacity(0.7))
            .child(Icon::new(IconName::Settings).small())
            .when(!icon_only, |this| this.child("Settings"))
            .on_click(cx.listener(|_this, _, _window, _cx| {
                // TODO: open settings (placeholder — content TBD with Jake).
            }))
    }

    fn roster_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex()
            .id("roster-toggle")
            .w_full()
            .justify_between()
            .items_center()
            .text_sm()
            .text_color(theme::text_muted())
            .child("Quarks")
            .child(Icon::new(IconName::PanelLeftClose).small())
            .active(|s| s.opacity(0.6))
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_rail(Rail::Roster, window, cx)),
            );

        let mut col = v_flex()
            .w_full()
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::sidebar())
            .child(header);

        for r in &self.view.roster {
            col = col.child(roster_row(r));
        }
        if self.view.roster.is_empty() {
            col = col.child(
                div()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .child("no quarks yet"),
            );
        }
        // Settings pinned to the bottom of the rail.
        col.child(div().flex_1())
            .child(self.settings_button(cx, false))
    }

    fn chat_pane(&self) -> impl IntoElement {
        let mut messages = v_flex().flex_1().gap_3().p_4().child(
            div()
                .text_sm()
                .text_color(theme::text_muted())
                .child("FIELD"),
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

        v_flex()
            .w_full()
            .h_full()
            .bg(theme::bg())
            .child(messages)
            .child(input)
    }

    /// The right rail: the Terminal. Placeholder body for now — real terminal
    /// wiring lands later. (Internally still `Rail::Inspector` for collapse/size.)
    fn terminal_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex()
            .id("inspector-toggle")
            .w_full()
            .justify_between()
            .items_center()
            .text_sm()
            .text_color(theme::text_muted())
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(IconName::SquareTerminal).small())
                    .child("Terminal"),
            )
            .child(Icon::new(IconName::PanelRightClose).small())
            .active(|s| s.opacity(0.6))
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_rail(Rail::Inspector, window, cx)),
            );

        v_flex()
            .w_full()
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::sidebar())
            .child(header)
            .child(
                v_flex()
                    .flex_1()
                    .mt_1()
                    .p_3()
                    .rounded_md()
                    .bg(theme::bg())
                    .text_sm()
                    .text_color(theme::text_muted())
                    .child("$ terminal coming soon"),
            )
    }

    /// Collapse or expand a rail. Just flips the persisted flag — the layout
    /// follows (an expanded rail is a resizable panel; a collapsed one is a fixed
    /// strip), so there's no sizing state to drive by hand.
    fn toggle_rail(&mut self, rail: Rail, _window: &mut Window, cx: &mut Context<Self>) {
        match rail {
            Rail::Roster => self.prefs.roster_collapsed = !self.prefs.roster_collapsed,
            Rail::Inspector => self.prefs.inspector_collapsed = !self.prefs.inspector_collapsed,
        }
        let _ = config::save(&self.prefs);
        cx.notify();
    }
}

/// The titlebar's app/options menu — a 3-line "hamburger" with circular hover.
/// Placeholder for now: opening a menu of options lands later. Stops propagation
/// so a press here can't start a window move.
fn menu_button() -> impl IntoElement {
    div()
        .id("app-menu")
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded_full()
        .text_color(theme::text_secondary())
        .hover(|s| s.bg(theme::surface_raised()).text_color(theme::text()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _window, cx| {
            cx.stop_propagation();
            // TODO: open the options menu (placeholder — items TBD with Jake).
        })
        .child(Icon::new(IconName::Menu).small())
}

/// A circular window-control button (min / max / close) with circular hover.
fn control_button(id: &'static str, icon: IconName, is_close: bool) -> impl IntoElement {
    let hover_bg = if is_close {
        theme::danger()
    } else {
        theme::surface_raised()
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded_full()
        .text_color(theme::text_secondary())
        .hover(|s| s.bg(hover_bg).text_color(theme::text()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            match id {
                "min" => window.minimize_window(),
                "max" => window.zoom_window(),
                _ => window.remove_window(),
            }
        })
        .child(Icon::new(icon).small())
}

/// A draggable titlebar region: marks the area for the compositor and starts a
/// window move on press.
fn drag_region(id: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .h_full()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
}

/// One roster entry, styled as a presence list-item: an initials [`Avatar`] with
/// a status [`Badge`] dot, a display name, and a one-word presence subtitle, with
/// a tooltip on hover. Display name currently derives from the quark id — a
/// Settings-chosen name/avatar lands here later.
fn roster_row(r: &RosterRow) -> impl IntoElement {
    let name = r.id.clone();
    let label = theme::presence_label(r.state);
    let tip: SharedString = format!("{name} — {label}").into();

    h_flex()
        .id(SharedString::from(format!("quark-{}", r.id)))
        .items_center()
        .gap_2p5()
        .px_2()
        .py_1p5()
        .rounded_md()
        .hover(|s| s.bg(theme::surface()))
        .child(
            Badge::new()
                .dot()
                .color(theme::presence(r.state))
                .child(Avatar::new().name(name.clone()).with_size(Size::Small)),
        )
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text())
                        .truncate()
                        .child(name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::presence(r.state))
                        .child(label),
                ),
        )
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
}

fn message_row(m: &MessageRow) -> impl IntoElement {
    let header = match &m.to {
        Some(to) => format!("{} → {}  ·  {}", m.from, to, m.kind_label),
        None => format!("{}  ·  {}", m.from, m.kind_label),
    };
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(theme::actor_hue(&m.from))
                .child(header),
        )
        .child(
            div()
                .text_color(theme::text_secondary())
                .child(m.body.clone()),
        )
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
        // Align gpui-component's own component colors (titlebar, inputs, window
        // controls) to Jake's palette so they blend with our hand-drawn surfaces.
        {
            let t = gpui_component::Theme::global_mut(cx);
            // Titlebar shares the sidebar's colour; the search bar (a darker
            // recessed pill) uses the base bg. `tokens.title_bar` is what the
            // TitleBar actually paints.
            t.title_bar = rgb(0x191a1b).into();
            t.tokens.title_bar = gpui::Hsla::from(rgb(0x191a1b)).into();
            t.title_bar_border = rgb(0x252627).into();
            t.background = rgb(0x121314).into();
            t.input = rgb(0x191a1b).into();
            t.secondary = rgb(0x191a1b).into();
            t.secondary_hover = rgb(0x252627).into();
            t.popover = rgb(0x191a1b).into();
            t.border = rgb(0x303133).into();
            // Close-button hover: red *background*, but keep the X light so it
            // stays legible (was red-on-red).
            t.danger = rgb(0xef4444).into();
            t.danger_foreground = rgb(0xf5f5f6).into();
            // Subtle dark window frame (Zed-style CSD border), matching the UI.
            t.window_border = rgb(0x2a2b2c).into();
            // Root paints this behind everything; transparent so our rounded
            // window frame (crate::window_frame) shows the shadow through the
            // corners instead of a square fill.
            t.tokens.background = gpui::hsla(0.0, 0.0, 0.0, 0.0).into();
        }
        cx.bind_keys([
            KeyBinding::new("ctrl-shift-p", TogglePalette, Some(KEY_CONTEXT)),
            KeyBinding::new("cmd-shift-p", TogglePalette, Some(KEY_CONTEXT)),
        ]);

        // Build window options here (needs `&App`, not the async cx below).
        let window_options = WindowOptions {
            titlebar: Some(TitleBar::title_bar_options()),
            window_decorations: Some(WindowDecorations::Client),
            // Transparent so the client-side shadow + rounded frame composite
            // over the desktop (Zed's approach). On WSLg this is the open
            // question — if the compositor ignores it, the inset shows black.
            window_background: WindowBackgroundAppearance::Transparent,
            window_bounds: Some(WindowBounds::centered(
                gpui::size(px(1440.0), px(900.0)),
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
