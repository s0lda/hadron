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
    actions, div, prelude::*, px, rgb, rgba, App, Context, Decorations, Entity, FocusHandle, Hsla,
    KeyBinding, MouseButton, Pixels, Render, Rgba, SharedString, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowDecorations, WindowOptions,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::avatar::Avatar;
use gpui_component::badge::Badge;
use gpui_component::stepper::{Stepper, StepperItem};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::tooltip::Tooltip;
use gpui_component::{
    h_flex, v_flex, Icon, IconName, Root, Sizable, Size, Theme, ThemeMode, TitleBar,
};
use hadron_lattice::{io, Actor, Event, Kind};

use crate::config::{self, ChamberPrefs, Identity};
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

/// Corner radius for floating panels/containers on the unified canvas.
const INNER_RADIUS: Pixels = px(12.0);

/// The two collapsible side rails.
#[derive(Clone, Copy)]
enum Rail {
    Roster,
    Inspector,
}

/// A fully-resolved display identity — what actually renders after applying the
/// user's [`Identity`] overrides over code defaults (id-derived name, hue color,
/// initials avatar).
struct ResolvedIdentity {
    name: String,
    color: Hsla,
    image: Option<String>,
}

/// The palette a user picks an identity color from (Settings). Kept small and
/// legible on the dark surfaces.
const IDENTITY_SWATCHES: [u32; 8] = [
    0xf5f5f6, // near-white (the human's default)
    0xec4899, // pink
    0xa855f7, // purple
    0x60a5fa, // blue
    0x34d399, // green
    0xfbbf24, // amber
    0xfb7185, // rose
    0x94a3b8, // slate
];

/// Parse a `#rrggbb` string into a color.
fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    u32::from_str_radix(s, 16).ok().map(rgb)
}

/// Up to two uppercase initials from a display name, for the fallback avatar.
fn initials(name: &str) -> String {
    let mut words = name.split_whitespace().filter_map(|w| w.chars().next());
    match (words.next(), words.next()) {
        (Some(a), Some(b)) => format!("{a}{b}").to_uppercase(),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => "?".to_string(),
    }
}

/// Render an identity's avatar: the chosen image if set, else a colored circle
/// with the name's initials.
fn identity_avatar(id: &ResolvedIdentity, diameter: f32) -> gpui::AnyElement {
    match &id.image {
        Some(path) => Avatar::new()
            .src(path.clone())
            .with_size(Size::Size(px(diameter)))
            .into_any_element(),
        None => div()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .size(px(diameter))
            .rounded_full()
            .bg(id.color.opacity(0.2))
            .text_color(id.color)
            .text_size(px(diameter * 0.4))
            .child(initials(&id.name))
            .into_any_element(),
    }
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

/// The three views over the field, selected by the chat column's segmented tabs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChatTab {
    /// The conversation — human/quark messages, styled like a chat.
    Chat,
    /// Every event on the field, compact — the raw activity log.
    Log,
    /// A vertical stepper over the run's milestones (non-message activity).
    Timeline,
}

impl ChatTab {
    const ALL: [ChatTab; 3] = [ChatTab::Chat, ChatTab::Log, ChatTab::Timeline];

    fn index(self) -> usize {
        match self {
            ChatTab::Chat => 0,
            ChatTab::Log => 1,
            ChatTab::Timeline => 2,
        }
    }

    fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(ChatTab::Chat)
    }

    fn label(self) -> &'static str {
        match self {
            ChatTab::Chat => "Chat",
            ChatTab::Log => "Log",
            ChatTab::Timeline => "Timeline",
        }
    }
}

/// Which identity the Settings overlay is currently editing.
#[derive(Clone, PartialEq, Eq)]
enum SettingsTarget {
    Human,
    Quark(String),
}

impl SettingsTarget {
    /// The actor key used for identity resolution / prefs lookup.
    fn key(&self) -> &str {
        match self {
            SettingsTarget::Human => "human",
            SettingsTarget::Quark(id) => id,
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
    /// Which view the chat column's segmented tabs are showing.
    chat_tab: ChatTab,
    /// Whether the Ctrl+Shift+P command palette overlay is showing.
    palette_open: bool,
    /// The palette's filter box.
    palette_input: Entity<InputState>,
    /// Whether the Settings overlay is showing, and which identity it edits.
    settings_open: bool,
    settings_target: SettingsTarget,
    /// Settings editor fields (display name + image path for the current target).
    settings_name: Entity<InputState>,
    settings_path: Entity<InputState>,
    /// Keep the input subscriptions alive for the window's lifetime. The last
    /// two repaint the Settings overlay so its live preview tracks typing.
    _input_sub: Subscription,
    _palette_sub: Subscription,
    _settings_subs: [Subscription; 2],
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

        let settings_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("Display name"));
        let settings_path = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Path to an image file… (optional)")
        });
        // Repaint the Settings overlay on every edit so its preview is live.
        let _settings_subs = [
            cx.subscribe_in(&settings_name, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
            cx.subscribe_in(&settings_path, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
        ];

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
            chat_tab: ChatTab::Chat,
            palette_open: false,
            palette_input,
            settings_open: false,
            settings_target: SettingsTarget::Human,
            settings_name,
            settings_path,
            _input_sub,
            _palette_sub,
            _settings_subs,
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
        // Round the full-height content itself to match the client frame, rather
        // than the (too-short) top/bottom strips — a 24px status bar can't reach
        // the ~20px radius, so its square corners poked past the frame's arc. The
        // strips are now transparent; the content's own rounded fill owns all four
        // corners. Zero on any tiled edge, so a maximized/snapped window stays square.
        let (top_radius, bottom_radius) = frame_corner_radii(window);
        let titlebar = self.titlebar(window, cx);
        let body = self.body(cx);
        let status = self.status_bar();
        let overlay = self.palette_open.then(|| self.palette_overlay(cx));
        let settings = self.settings_open.then(|| self.settings_overlay(cx));

        let content = v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_toggle_palette))
            .relative()
            .size_full()
            .bg(theme::sidebar())
            .rounded_tl(top_radius)
            .rounded_tr(top_radius)
            .rounded_bl(bottom_radius)
            .rounded_br(bottom_radius)
            .text_color(theme::text())
            .child(titlebar)
            .child(body)
            .child(status)
            .children(overlay)
            .children(settings);

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
            // Transparent: the content behind (theme::sidebar) shows through, and
            // its rounded top corners own the frame's arc — an opaque strip here
            // would paint square nubs past it.
            // App/options menu (the 3-line menu; options land later) in the far
            // left corner.
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .pl(px(8.0))
                    .child(menu_button()),
            )
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
            // Transparent: the content's rounded bottom corners (theme::sidebar)
            // own the frame's arc — this 24px strip can't round tight enough to.
            .text_xs()
            .text_color(theme::text_muted())
            .child(div().child("ready"))
            .child(div().child(format!("{} quark(s)", self.view.roster.len())))
    }

    /// The body: the left roster ("friends list") at a locked width, then the
    /// resizable chat + terminal group. The roster sits *outside* the group so
    /// dragging the terminal never disturbs it — only the terminal is draggable,
    /// and the chat flexes to fill whatever's left (so a window resize reflows
    /// into the chat instead of stranding a stored width). A collapsed rail is a
    /// thin strip. The group is re-keyed on the terminal's presence so a fresh
    /// sizing state seeds its width from prefs; `on_resize` persists it back.
    fn body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let roster_collapsed = self.prefs.roster_collapsed;
        let inspector_collapsed = self.prefs.inspector_collapsed;
        let chamber = cx.entity();

        let group_id = SharedString::from(format!("chamber-body-{}", inspector_collapsed as u8));

        let mut group = h_resizable(group_id).on_resize(move |state, _window, app| {
            let sizes = state.read(app).sizes().clone();
            chamber.update(app, |this, _cx| {
                // Only the terminal carries a stored width now; it's the last
                // panel in the group (chat is the flex first panel).
                if !this.prefs.inspector_collapsed {
                    if let Some(w) = sizes.last() {
                        this.prefs.inspector_width = w.as_f32();
                    }
                }
                let _ = config::save(&this.prefs);
            });
        });

        // Chat: flex (no fixed size) so it absorbs slack on resize.
        group = group.child(resizable_panel().child(self.chat_pane(cx)));
        if !inspector_collapsed {
            group = group.child(
                resizable_panel()
                    .size(px(self.prefs.inspector_width))
                    .size_range(px(RAIL_MIN)..px(RAIL_MAX))
                    .child(self.terminal_pane(cx)),
            );
        }

        // Left rail: a fixed-width column (locked, not draggable) or a thin strip
        // when collapsed — a sibling of the group, never part of the drag.
        let left = if roster_collapsed {
            self.rail_strip(Rail::Roster, cx).into_any_element()
        } else {
            div()
                .flex_none()
                .w(px(self.prefs.roster_width))
                .h_full()
                .child(self.roster_pane(cx))
                .into_any_element()
        };

        h_flex()
            .flex_1()
            .w_full()
            .child(left)
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
            .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
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
            col = col.child(roster_row(&self.resolve_identity(&r.id), r));
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

    /// The center column: a segmented Chat / Log / Timeline tab bar over the
    /// selected view, with the human's message box pinned at the foot. The whole
    /// thing is a rounded, filled card that floats on the unified canvas.
    fn chat_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.chat_tab;
        let tabs = TabBar::new("chat-tabs")
            .segmented()
            .selected_index(selected.index())
            .children(ChatTab::ALL.map(|t| Tab::new().label(t.label())))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.chat_tab = ChatTab::from_index(*ix);
                cx.notify();
            }));

        let header = h_flex().flex_none().items_center().px_3().py_2().child(tabs);

        // The scrolling viewport: the selected view stacks to its natural height
        // and scrolls *within* the card, instead of growing the card and pushing
        // the input (and the whole layout) off the bottom.
        let body = div()
            .id("chat-body-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(match selected {
                ChatTab::Chat => self.chat_view().into_any_element(),
                ChatTab::Log => self.log_view().into_any_element(),
                ChatTab::Timeline => self.timeline_view().into_any_element(),
            });

        let input = h_flex()
            .flex_none()
            .m_4()
            .px_1()
            .rounded_lg()
            .bg(theme::input_bg())
            .child(Input::new(&self.input));

        // The floating chat card: darker + rounded, inset from the lighter
        // unified space that shows around it.
        let card = v_flex()
            .flex_1()
            .min_h_0()
            .rounded(INNER_RADIUS)
            .overflow_hidden()
            .bg(theme::bg())
            .child(header)
            .child(body)
            .child(input);

        v_flex()
            .w_full()
            .h_full()
            .p_2()
            .bg(theme::sidebar())
            .child(card)
    }

    /// The Chat tab: the conversation only (message events), styled like a chat
    /// with each author's avatar and name.
    fn chat_view(&self) -> impl IntoElement {
        let mut col = v_flex().gap_4().p_4();
        let mut any = false;
        for m in &self.view.messages {
            if m.kind_label == "message" {
                col = col.child(chat_message_row(&self.resolve_identity(&m.from), m));
                any = true;
            }
        }
        if !any {
            col = col.child(empty_hint("No messages yet — say something below."));
        }
        col
    }

    /// The Log tab: every event on the field, compact (the raw activity).
    fn log_view(&self) -> impl IntoElement {
        let mut col = v_flex().gap_3().p_4();
        if self.view.messages.is_empty() {
            col = col.child(empty_hint("The field is empty."));
        }
        for m in &self.view.messages {
            col = col.child(message_row(m));
        }
        col
    }

    /// The Timeline tab: a vertical [`Stepper`] over the run's milestones — the
    /// non-message activity (status changes, edits, commands, snapshots), most
    /// recent marked as the current step.
    fn timeline_view(&self) -> impl IntoElement {
        let steps: Vec<&MessageRow> = self
            .view
            .messages
            .iter()
            .filter(|m| m.kind_label != "message")
            .collect();

        let mut col = v_flex().p_4();
        if steps.is_empty() {
            return col.child(empty_hint("No activity yet — the timeline fills as quarks work."));
        }

        let current = steps.len().saturating_sub(1);
        let stepper = Stepper::new("timeline")
            .vertical()
            .selected_index(current)
            .items(steps.into_iter().map(|m| {
                StepperItem::new().pb_6().icon(kind_icon(m.kind_label)).child(
                    v_flex()
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text())
                                .child(format!("{}  ·  {}", m.from, m.kind_label)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(m.body.clone()),
                        ),
                )
            }));
        col = col.child(stepper);
        col
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

    /// Resolve an actor's display identity: prefs overrides over code defaults.
    /// `actor` is `"human"` or a quark id (as it appears in [`MessageRow::from`]
    /// / [`RosterRow::id`]).
    fn resolve_identity(&self, actor: &str) -> ResolvedIdentity {
        let stored: Option<&Identity> = if actor == "human" {
            Some(&self.prefs.human)
        } else {
            self.prefs.quarks.get(actor)
        };
        let default_name = if actor == "human" { "You" } else { actor };
        let name = stored
            .and_then(|i| i.display_name.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| default_name.to_string());
        let color: Hsla = stored
            .and_then(|i| i.color.as_deref())
            .and_then(parse_hex)
            .map(Into::into)
            .unwrap_or_else(|| theme::actor_hue(actor).into());
        let image = stored
            .and_then(|i| i.image_path.clone())
            .filter(|p| !p.trim().is_empty());
        ResolvedIdentity { name, color, image }
    }

    /// Open the Settings overlay, editing the human's identity first.
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = true;
        self.settings_target = SettingsTarget::Human;
        self.load_settings_inputs(window, cx);
        cx.notify();
    }

    /// Commit the name/image inputs, then close the overlay and refocus root.
    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_settings_inputs(cx);
        self.settings_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// A mutable handle to the identity currently being edited (creating an
    /// empty quark entry on first edit).
    fn settings_identity_mut(&mut self) -> &mut Identity {
        match &self.settings_target {
            SettingsTarget::Human => &mut self.prefs.human,
            SettingsTarget::Quark(id) => self.prefs.quarks.entry(id.clone()).or_default(),
        }
    }

    /// The stored color override for the current target, if any (`#rrggbb`).
    fn settings_color(&self) -> Option<String> {
        let key = self.settings_target.key();
        let id = if key == "human" {
            Some(&self.prefs.human)
        } else {
            self.prefs.quarks.get(key)
        };
        id.and_then(|i| i.color.clone())
    }

    /// Load the current target's name + image path into the editor inputs.
    fn load_settings_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (name, path) = {
            let key = self.settings_target.key();
            let id = if key == "human" {
                Some(&self.prefs.human)
            } else {
                self.prefs.quarks.get(key)
            };
            (
                id.and_then(|i| i.display_name.clone()).unwrap_or_default(),
                id.and_then(|i| i.image_path.clone()).unwrap_or_default(),
            )
        };
        self.settings_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.settings_path
            .update(cx, |s, cx| s.set_value(path, window, cx));
    }

    /// Write the editor inputs back into the current target identity and persist.
    fn commit_settings_inputs(&mut self, cx: &mut Context<Self>) {
        let name = self.settings_name.read(cx).value().trim().to_string();
        let path = self.settings_path.read(cx).value().trim().to_string();
        let id = self.settings_identity_mut();
        id.display_name = (!name.is_empty()).then_some(name);
        id.image_path = (!path.is_empty()).then_some(path);
        let _ = config::save(&self.prefs);
    }

    /// Switch which identity the overlay edits (committing the current one).
    fn select_settings_target(
        &mut self,
        target: SettingsTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_settings_inputs(cx);
        self.settings_target = target;
        self.load_settings_inputs(window, cx);
        cx.notify();
    }

    /// Set the current target's accent/avatar color from a swatch.
    fn set_settings_color(&mut self, hex: u32, cx: &mut Context<Self>) {
        self.commit_settings_inputs(cx);
        self.settings_identity_mut().color = Some(format!("#{hex:06x}"));
        let _ = config::save(&self.prefs);
        cx.notify();
    }

    /// Clear the current target's image (falling back to color + initials).
    fn clear_settings_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_identity_mut().image_path = None;
        self.settings_path
            .update(cx, |s, cx| s.set_value("", window, cx));
        let _ = config::save(&self.prefs);
        cx.notify();
    }

    /// Reset the current target to its code defaults.
    fn reset_settings_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.settings_target.clone() {
            SettingsTarget::Human => self.prefs.human = Identity::default(),
            SettingsTarget::Quark(id) => {
                self.prefs.quarks.remove(&id);
            }
        }
        self.load_settings_inputs(window, cx);
        let _ = config::save(&self.prefs);
        cx.notify();
    }

    /// The Settings overlay: a dim backdrop (click to dismiss) behind a card
    /// that edits one identity — an avatar switcher, a live preview, a display
    /// name, a color swatch row, and an image path (image wins over color).
    fn settings_overlay(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.settings_target.clone();

        // Everyone editable: the human, then each quark on the roster.
        let mut switcher = h_flex().gap_2().flex_wrap().child(self.settings_avatar_button(
            SettingsTarget::Human,
            &target,
            cx,
        ));
        for r in &self.view.roster {
            switcher = switcher.child(self.settings_avatar_button(
                SettingsTarget::Quark(r.id.clone()),
                &target,
                cx,
            ));
        }

        // Live preview: the target resolved, but with the in-progress name/image
        // from the inputs so it tracks typing.
        let live_name = self.settings_name.read(cx).value().to_string();
        let live_path = self.settings_path.read(cx).value().trim().to_string();
        let mut preview = self.resolve_identity(target.key());
        if !live_name.trim().is_empty() {
            preview.name = live_name;
        }
        preview.image = (!live_path.is_empty()).then_some(live_path);
        let preview_row = h_flex()
            .items_center()
            .gap_3()
            .child(identity_avatar(&preview, 44.0))
            .child(
                div()
                    .text_color(preview.color)
                    .child(preview.name.clone()),
            );

        // Color swatches; the stored color (if any) gets a bright ring.
        let selected = self.settings_color();
        let mut swatches = h_flex().gap_2().flex_wrap();
        for hex in IDENTITY_SWATCHES {
            let is_sel = selected.as_deref() == Some(format!("#{hex:06x}").as_str());
            swatches = swatches.child(
                div()
                    .id(SharedString::from(format!("swatch-{hex:06x}")))
                    .size(px(22.0))
                    .rounded_full()
                    .bg(rgb(hex))
                    .border_2()
                    .border_color(if is_sel { theme::text() } else { theme::border() })
                    .hover(|s| s.border_color(theme::text_secondary()))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_settings_color(hex, cx))),
            );
        }

        let card = v_flex()
            .occlude()
            .mt(px(80.0))
            .w(px(520.0))
            .p_4()
            .gap_4()
            .rounded_lg()
            .bg(theme::sidebar())
            .border_1()
            .border_color(theme::border())
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_color(theme::text()).child("Settings"))
                    .child(
                        div()
                            .id("settings-close")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(24.0))
                            .rounded_full()
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::surface_raised()).text_color(theme::text()))
                            .child(Icon::new(IconName::WindowClose).small())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                            ),
                    ),
            )
            .child(settings_field("Identity", switcher.into_any_element()))
            .child(settings_field("Preview", preview_row.into_any_element()))
            .child(settings_field(
                "Display name",
                Input::new(&self.settings_name).into_any_element(),
            ))
            .child(settings_field("Color", swatches.into_any_element()))
            .child(settings_field(
                "Image",
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(Input::new(&self.settings_path)))
                    .child(
                        text_button("settings-clear-img", "Clear")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_settings_image(window, cx)
                            })),
                    )
                    .into_any_element(),
            ))
            .child(
                h_flex()
                    .justify_between()
                    .pt_1()
                    .child(
                        text_button("settings-reset", "Reset to default").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.reset_settings_target(window, cx)
                            }),
                        ),
                    )
                    .child(
                        div()
                            .id("settings-done")
                            .px_3()
                            .py_1p5()
                            .rounded_md()
                            .bg(theme::accent())
                            .text_color(theme::text())
                            .hover(|s| s.opacity(0.9))
                            .child("Done")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                            ),
                    ),
            );

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .bg(rgba(0x00000088))
            .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
            .child(card)
    }

    /// One avatar button in the Settings identity switcher.
    fn settings_avatar_button(
        &self,
        who: SettingsTarget,
        current: &SettingsTarget,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let resolved = self.resolve_identity(who.key());
        let selected = &who == current;
        let id = SharedString::from(format!("settings-id-{}", who.key()));
        div()
            .id(id)
            .p_0p5()
            .rounded_full()
            .border_2()
            .border_color(if selected {
                theme::accent()
            } else {
                theme::border()
            })
            .hover(|s| s.border_color(theme::text_secondary()))
            .child(identity_avatar(&resolved, 32.0))
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.select_settings_target(who.clone(), window, cx)
                }),
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

/// Corner radii for the full-height content container, matching the client
/// frame ([`crate::window_frame`]). Rounds at the frame's own radius so the
/// content tucks *inside* the 1px border (a hair rounder never pokes past the
/// arc; the frame's matching sidebar fill hides the sub-pixel sliver). Zero on
/// a tiled edge (maximized/snapped) so those corners stay square.
fn frame_corner_radii(window: &Window) -> (Pixels, Pixels) {
    let r = crate::window_frame::FRAME_RADIUS;
    match window.window_decorations() {
        Decorations::Client { tiling } => (
            if tiling.top { px(0.0) } else { r },
            if tiling.bottom { px(0.0) } else { r },
        ),
        Decorations::Server => (px(0.0), px(0.0)),
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

/// One roster entry, styled as a presence list-item: the resolved avatar with a
/// status [`Badge`] dot, a display name, and a one-word presence subtitle, with a
/// tooltip on hover.
fn roster_row(id: &ResolvedIdentity, r: &RosterRow) -> impl IntoElement {
    let name = id.name.clone();
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
                .child(identity_avatar(id, 28.0)),
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

/// A labeled row in the Settings card: a muted caption above its control.
fn settings_field(label: &'static str, content: gpui::AnyElement) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(
            div()
                .text_xs()
                .text_color(theme::text_muted())
                .child(label),
        )
        .child(content)
}

/// A small, subtle text button for secondary actions (caller attaches on_click).
fn text_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .text_sm()
        .text_color(theme::text_secondary())
        .hover(|s| s.bg(theme::surface_raised()).text_color(theme::text()))
        .child(label)
}

/// A muted placeholder line shown when a tab view has nothing to render.
fn empty_hint(text: &'static str) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(theme::text_muted())
        .child(text)
}

/// Map an event kind to a timeline step icon.
fn kind_icon(kind_label: &str) -> IconName {
    match kind_label {
        "status" => IconName::Info,
        "edit" => IconName::Folder,
        "command" => IconName::SquareTerminal,
        "snapshot" => IconName::CircleCheck,
        _ => IconName::Asterisk,
    }
}

/// A chat-styled message row: the author's resolved avatar and name, then the
/// body — used by the Chat tab so the field reads like a conversation.
fn chat_message_row(id: &ResolvedIdentity, m: &MessageRow) -> impl IntoElement {
    h_flex()
        .items_start()
        .gap_2p5()
        .child(identity_avatar(id, 28.0))
        .child(
            v_flex()
                .min_w_0()
                .gap_0p5()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(id.color)
                                .child(id.name.clone()),
                        )
                        .when_some(m.to.clone(), |this, to| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(format!("→ {to}")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_color(theme::text_secondary())
                        .child(m.body.clone()),
                ),
        )
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
            // Borderless: the resize handle paints `border` when idle, so match
            // it to the sidebar and it disappears into the unified space; while
            // dragging it paints `drag_border` — keep that on-brand pink so the
            // drag still shows feedback. (This also softens gpui-component's own
            // hairlines, which suits the borderless surfaces.)
            t.border = rgb(0x191a1b).into();
            t.drag_border = rgb(0xec4899).into();
            // The chat's segmented tab track: a step above the dark card (bg) so
            // the control is visible. The active tab reads as a darker cutout (its
            // sliding indicator paints `tokens.background`, which we keep
            // transparent for the frame — so it shows the card behind).
            t.tokens.tab_bar_segmented = gpui::Hsla::from(rgb(0x202122)).into();
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
