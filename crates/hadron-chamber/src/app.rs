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
    actions, div, prelude::*, px, rgb, rgba, AnimationExt, App, Context, Decorations, Entity,
    FocusHandle, Hsla, KeyBinding, MouseButton, Pixels, Render, Rgba, ScrollHandle, SharedString,
    Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowControlArea,
    WindowDecorations, WindowOptions,
};
use gpui_component::avatar::Avatar;
// badge removed
use gpui_component::chart::{BarChart, LineChart};
use gpui_component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use gpui_component::stepper::{Stepper, StepperItem};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::tag::Tag;
use gpui_component::tooltip::Tooltip;

// table imports removed
use gpui_component::{
    h_flex, v_flex, Icon, IconName, Root, Sizable, Size, Theme, ThemeMode, TitleBar,
};
use hadron_lattice::{io, load_team, Actor, Event, Kind, Mode, QuarkId, QuarkState, Team};

use crate::config::{self, ChamberPrefs, Identity};
use crate::model::{self, ChamberView, MessageRow, RosterRow};
use crate::theme;

actions!(chamber, [TogglePalette, CycleMode]);

/// Key-dispatch context for the chamber's window-level actions.
const KEY_CONTEXT: &str = "Chamber";

/// Width of a collapsed rail's strip (just the expand affordance).
const RAIL_STRIP: f32 = 44.0;
/// Minimum drag width for the resizable terminal rail.
const RAIL_MIN: f32 = 160.0;
/// The terminal/multitool has no meaningful upper width — cap it far past any
/// screen so the only real limit is the chat keeping its [`CHAT_MIN`].
const TERMINAL_MAX: f32 = 6000.0;
/// The chat can be squeezed but never collapsed to nothing.
const CHAT_MIN: f32 = 360.0;

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
    /// Per-quark session stats: turns, tokens, context, quota.
    Session,
}

impl ChatTab {
    const ALL: [ChatTab; 4] = [
        ChatTab::Chat,
        ChatTab::Log,
        ChatTab::Timeline,
        ChatTab::Session,
    ];

    /// Sizes every per-tab array. A tab added to `ALL` without growing those
    /// arrays is an index-out-of-bounds the moment the tab is opened, so they
    /// take their length from here rather than restating it.
    const COUNT: usize = Self::ALL.len();

    fn index(self) -> usize {
        match self {
            ChatTab::Chat => 0,
            ChatTab::Log => 1,
            ChatTab::Timeline => 2,
            ChatTab::Session => 3,
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
            ChatTab::Session => "Session",
        }
    }
}

fn format_num(n: u32) -> String {
    if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        let s = n.to_string();
        if s.len() > 3 {
            let (head, tail) = s.split_at(s.len() - 3);
            format!("{},{}", head, tail)
        } else {
            s
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RightRailTab {
    Terminal,
    FileTree,
    Changes,
}

impl RightRailTab {
    const ALL: [RightRailTab; 3] = [
        RightRailTab::Terminal,
        RightRailTab::FileTree,
        RightRailTab::Changes,
    ];

    fn index(self) -> usize {
        match self {
            RightRailTab::Terminal => 0,
            RightRailTab::FileTree => 1,
            RightRailTab::Changes => 2,
        }
    }

    fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(RightRailTab::Terminal)
    }

    fn label(self) -> &'static str {
        match self {
            RightRailTab::Terminal => "Terminal",
            RightRailTab::FileTree => "File Tree",
            RightRailTab::Changes => "Changes",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct AgentDescriptor {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
struct AuthMethod {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, PartialEq, Eq)]
enum ProviderState {
    NotConnected,
    Connecting,
    NeedsAuth(Vec<AuthMethod>),
    Ready { model: String },
    Failed(String),
}

#[derive(Clone, PartialEq, Eq)]
struct ConfiguredQuark {
    pub id: String,
    pub transport: String,
    pub state: ProviderState,
}

#[derive(Clone, PartialEq, Eq)]
enum WizardState {
    None,
    PickPreset,
    Connecting(AgentDescriptor, ProviderState),
}

/// Which identity the Settings overlay is currently editing.
#[derive(Clone, PartialEq, Eq)]
enum SettingsTarget {
    Providers,
    Human,
    Quark(String),
}

impl SettingsTarget {
    /// The actor key used for identity resolution / prefs lookup.
    fn key(&self) -> &str {
        match self {
            SettingsTarget::Providers => "providers",
            SettingsTarget::Human => "human",
            SettingsTarget::Quark(id) => id,
        }
    }
}

struct Chamber {
    view: ChamberView,
    prefs: ChamberPrefs,
    /// The seated team (id → provider/model/flavor), read from `team.json`, used
    /// to make roster rows legible. Read-only in the chamber.
    team: Team,
    /// The field file this chamber reads from and steers into.
    path: PathBuf,
    /// The human's message box at the foot of the chat column.
    input: Entity<InputState>,
    /// Root focus target, so Ctrl+Shift+P dispatches regardless of what's focused.
    focus_handle: FocusHandle,
    /// Which view the chat column's segmented tabs are showing.
    chat_tab: ChatTab,
    /// Which view the right rail's segmented tabs are showing. The right rail is
    /// independent of the chat column: changing the chat tab must not move it.
    right_rail_tab: RightRailTab,
    /// Cached diff string for the Changes rail
    working_diff: Option<Vec<crate::vcs::FileDiff>>,
    changes_open_ixs: std::collections::HashSet<usize>,
    changes_scroll: ScrollHandle,
    /// Virtual list state for the Chat tab.
    chat_list_state: gpui::ListState,
    log_list_state: gpui::ListState,
    log_expanded_ixs: std::collections::HashSet<usize>,
    /// Maps a virtual list item index to the message's true index in `view.messages`.
    chat_message_ixs: Vec<usize>,
    /// Scroll position for each of the three tabs.
    chat_scrolls: [ScrollHandle; 4],
    /// Cache of parsed Markdown to HTML, keyed by message index
    parsed_markdown: std::cell::RefCell<std::collections::HashMap<usize, String>>,
    /// A debounced window-bounds save is already in flight, so a drag (which
    /// re-renders every frame) coalesces into one write instead of one per frame.
    bounds_save_pending: bool,
    /// Whether the Ctrl+Shift+P command palette overlay is showing.
    palette_open: bool,
    /// The palette's filter box.
    palette_input: Entity<InputState>,
    /// Which filtered command the palette has highlighted (Up/Down move it,
    /// Enter runs it). Reset to 0 on open and whenever the query changes.
    palette_selected: usize,
    /// Whether the Settings overlay is showing, and which identity it edits.
    settings_open: bool,
    settings_target: SettingsTarget,
    /// Settings editor fields (display name + image path for the current target).
    settings_name: Entity<InputState>,
    settings_path: Entity<InputState>,
    settings_effort: Entity<InputState>,
    settings_mode_config: Entity<InputState>,
    /// Keep the input subscriptions alive for the window's lifetime. The last
    /// two repaint the Settings overlay so its live preview tracks typing.
    _input_sub: Subscription,
    _palette_sub: Subscription,
    _settings_subs: [Subscription; 2],
    providers: Vec<ConfiguredQuark>,
    wizard_state: WizardState,
    file_tree_paths: Vec<String>,
    completion_files: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    file_tree_open: Option<(String, String)>,
    file_tree_expanded: std::collections::HashSet<String>,
    terminal_history: Vec<(String, String, String)>,
    terminal_input: Entity<InputState>,
    _terminal_sub: Subscription,
    terminal: Option<crate::sys::Terminal>,
    info_panel: Option<String>,
    /// The About dialog, opened from the app menu.
    about_open: bool,
    terminal_scroll: ScrollHandle,
    file_tree_scroll: ScrollHandle,
    file_tree_open_scroll: ScrollHandle,
}

#[derive(Clone, Debug)]
pub enum ContextMenuAction {
    OpenFile(String),
    OpenInEditor(String),
    OpenInFolder(String),
    CopyPath(String),
    QuarkInfo(String),
    ToggleQuark(String),
    SetFlavor(String, hadron_lattice::Flavor),
}

impl Chamber {
    fn new(
        view: ChamberView,
        prefs: ChamberPrefs,
        team: Team,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let repo_root = crate::vcs::repo_root_of(&path);
        let files = crate::sys::list_workspace_files(&repo_root);
        let completion_files = std::rc::Rc::new(std::cell::RefCell::new(files.clone()));
        let files_for_completion = completion_files.clone();

        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .auto_grow(1, 4)
                .submit_on_enter(true)
                .placeholder("Type @quark a message…  (Enter to send · Shift+Enter for newline)");
            let team_quarks = team
                .quarks
                .iter()
                .map(|q| (q.id.0.clone(), q.display_name.clone()))
                .collect::<Vec<_>>();
            state.lsp.completion_provider = Some(std::rc::Rc::new(
                crate::completions::ChatCompletionProvider {
                    quarks: team_quarks,
                    files: files_for_completion,
                },
            ));
            state
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

        let settings_name = cx.new(|cx| InputState::new(window, cx).placeholder("Display name"));
        let settings_path = cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/image.png"));
        let settings_effort = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. low, standard, high"));
        let settings_mode_config = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. architect, code, ask"));
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

        // Precompute the message indices for the virtualized chat.
        let chat_message_ixs: Vec<usize> = view
            .messages
            .iter()
            .enumerate()
            .filter_map(|(ix, m)| (m.kind_label == "message").then_some(ix))
            .collect();

        let chat_list_state = gpui::ListState::new(
            chat_message_ixs.len(),
            gpui::ListAlignment::Bottom,
            px(1000.),
        );

        let log_list_state = gpui::ListState::new(
            view.messages.len(),
            gpui::ListAlignment::Bottom,
            px(1000.),
        );

        // Open showing the newest message: honoured on the first paint, once the
        // content is laid out.
        let chat_scrolls = [
            ScrollHandle::new(),
            ScrollHandle::new(),
            ScrollHandle::new(),
            ScrollHandle::new(),
        ];
        chat_scrolls[0].scroll_to_bottom(); // Chat tab
        let terminal_input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 4)
                .submit_on_enter(true)
                .placeholder("$ command...")
        });
        let _terminal_sub = cx.subscribe_in(&terminal_input, window, Self::on_terminal_submit);
        let providers: Vec<ConfiguredQuark> = team
            .quarks
            .iter()
            .map(|seat| ConfiguredQuark {
                id: seat.id.0.clone(),
                transport: seat.provider.clone(),
                state: ProviderState::Ready {
                    model: seat.model.clone(),
                },
            })
            .collect();

        Chamber {
            view,
            prefs,
            team,
            path,
            input,
            focus_handle,
            chat_tab: ChatTab::Chat,
            right_rail_tab: RightRailTab::Terminal,
            working_diff: None,
            changes_open_ixs: std::collections::HashSet::new(),
            changes_scroll: ScrollHandle::new(),
            chat_list_state,
            log_list_state,
            log_expanded_ixs: std::collections::HashSet::new(),
            chat_message_ixs,
            chat_scrolls,
            parsed_markdown: std::cell::RefCell::new(std::collections::HashMap::new()),
            bounds_save_pending: false,
            palette_open: false,
            palette_input,
            palette_selected: 0,
            settings_open: false,
            settings_target: SettingsTarget::Human,
            settings_name,
            settings_path,
            settings_effort,
            settings_mode_config,
            _input_sub,
            _palette_sub,
            _settings_subs,
            providers,
            wizard_state: WizardState::None,
            file_tree_paths: files,
            completion_files,
            file_tree_open: None,
            file_tree_expanded: std::collections::HashSet::new(),
            terminal_history: Vec::new(),
            terminal_input,
            _terminal_sub,
            terminal: None,
            info_panel: None,
            about_open: false,
            terminal_scroll: ScrollHandle::new(),
            file_tree_scroll: ScrollHandle::new(),
            file_tree_open_scroll: ScrollHandle::new(),
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
            self.palette_selected = 0;
            self.palette_input.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn on_terminal_submit(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::PressEnter { .. } = event {
            let cmd = input.read(cx).value().trim().to_string();
            if cmd.is_empty() {
                return;
            }
            input.update(cx, |state, cx| state.set_value("", window, cx));

            let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();

            if self.terminal.is_none() {
                match crate::sys::Terminal::new(&repo_root) {
                    Ok(term) => self.terminal = Some(term),
                    Err(err) => {
                        self.terminal_history.push((String::new(), cmd, err));
                        cx.notify();
                        return;
                    }
                }
            }

            if let Some(term) = &mut self.terminal {
                let cwd = term.cwd.clone();
                let output = match term.execute(&cmd) {
                    Ok(out) => out,
                    Err(err) => err,
                };
                self.terminal_history.push((cwd, cmd, output));
                self.terminal_scroll.scroll_to_bottom();
            }
            cx.notify();
        }
    }

    /// React to the palette filter: Enter runs the highlighted command; typing
    /// re-filters, so the highlight resets to the top match.
    fn on_palette_submit(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => {
                let query = input.read(cx).value().to_lowercase();
                let cmds = self.filtered_commands(&query);
                if let Some(&cmd) = cmds.get(self.palette_selected).or_else(|| cmds.first()) {
                    self.run_command(cmd, window, cx);
                }
            }
            InputEvent::Change => {
                self.palette_selected = 0;
                cx.notify();
            }
            _ => {}
        }
    }

    /// Move the palette highlight by `delta`, clamped to the filtered list.
    fn move_palette_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let query = self.palette_input.read(cx).value().to_lowercase();
        let len = self.filtered_commands(&query).len();
        if len == 0 {
            self.palette_selected = 0;
            return;
        }
        let max = len as isize - 1;
        self.palette_selected = (self.palette_selected as isize + delta).clamp(0, max) as usize;
        cx.notify();
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
        // The highlighted row (Up/Down move it, Enter runs it), clamped in case
        // the filtered list shrank since the selection last moved.
        let sel = self.palette_selected.min(cmds.len().saturating_sub(1));

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
                let selected = i == sel; // the Enter target
                list = list.child(
                    div()
                        .id(("palette-cmd", i))
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .bg(if selected {
                            theme::bg_surface_raised()
                        } else {
                            theme::bg_surface()
                        })
                        .hover(|s| s.bg(theme::bg_surface_raised()))
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
            // Column so items_center handles the horizontal centering and the
            // card sits at a fixed top offset — deterministic, not leaning on the
            // default stretch/align that was dropping it to the window's foot.
            .flex_col()
            .items_center()
            .justify_start()
            .bg(rgba(0x00000066))
            .on_click(cx.listener(|this, _, window, cx| {
                this.palette_open = false;
                window.focus(&this.focus_handle, cx);
                cx.notify();
            }))
            .child(
                v_flex()
                    .occlude()
                    // The focused Input binds Up/Down/Escape (to cursor moves /
                    // clear) at the deepest node, so an ancestor *key* binding
                    // can't outrank it. Intercept the resulting Input *actions*
                    // in the capture phase — which runs ancestor-first, before
                    // the Input's own bubble handler — and stop them there. Enter
                    // still flows to the Input's submit (see on_palette_submit).
                    .capture_action(cx.listener(|this, _: &MoveDown, _window, cx| {
                        this.move_palette_selection(1, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &MoveUp, _window, cx| {
                        this.move_palette_selection(-1, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                        this.palette_open = false;
                        window.focus(&this.focus_handle, cx);
                        cx.notify();
                        cx.stop_propagation();
                    }))
                    .mt(px(96.0))
                    .w(px(480.0))
                    .p_2()
                    .rounded_lg()
                    .bg(theme::bg_elevated())
                    .border_1()
                    .border_color(theme::border())
                    .child(Input::new(&self.palette_input))
                    .child(list),
            )
    }

    fn info_panel_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let qid = self.info_panel.as_ref().unwrap();
        let roster_row = self.view.roster.iter().find(|r| &r.id == qid).unwrap();

        let stats = self.view.session_stats();
        let q_stats = stats
            .per_quark
            .into_iter()
            .find(|(id, _)| id == qid)
            .map(|(_, s)| s)
            .unwrap_or_default();

        let (agent_str, model_str) = match roster_row.transport {
            hadron_lattice::Transport::Acp => {
                let agent = if roster_row.model.is_empty() {
                    "unknown"
                } else {
                    &roster_row.model
                };
                (agent, "unknown")
            }
            hadron_lattice::Transport::Cli => (
                "hadron-adapter",
                if roster_row.model.is_empty() {
                    "unknown"
                } else {
                    &roster_row.model
                },
            ),
        };

        let flavor_str = match &roster_row.flavor {
            Some(hadron_lattice::Flavor::Orchestrator) => "Orchestrator",
            Some(hadron_lattice::Flavor::Worker) => "Worker",
            None => "None",
        };

        let transport_str = match roster_row.transport {
            hadron_lattice::Transport::Cli => "CLI (one-shot)",
            hadron_lattice::Transport::Acp => "ACP (resident)",
        };

        let enabled_str = if roster_row.enabled {
            "Enabled"
        } else {
            "Disabled"
        };
        let mode_str = if roster_row.mode_is_override {
            format!("{:?} (override)", roster_row.mode)
        } else {
            format!("{:?} (global default)", roster_row.mode)
        };

        let first_seen_str = q_stats
            .first_seen
            .map(|ts| ts.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "Never".to_string());
        let last_active_str = q_stats
            .last_active
            .map(|ts| ts.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "Never".to_string());

        let mut stats_block = v_flex()
            .gap_1()
            .mt_4()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::text())
                    .child("Session Stats"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("Turns: {}", q_stats.turns)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!(
                        "Spent: {} fresh, {} cached",
                        format_num(q_stats.fresh),
                        format_num(q_stats.cached)
                    )),
            );

        if roster_row.unknown_turns > 0 {
            stats_block = stats_block.child(div().text_xs().text_color(theme::text_muted()).child(
                format!("+{} turns of unknown spend", roster_row.unknown_turns),
            ));
        }

        stats_block = stats_block
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("First Seen: {}", first_seen_str)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("Last Active: {}", last_active_str)),
            );

        if let Some(ctx) = q_stats.context.as_ref() {
            stats_block = stats_block.child(div().text_xs().text_color(theme::text_muted()).child(
                format!(
                    "Context: {:.1}% ({} / {})",
                    ctx.used_percentage,
                    format_num(ctx.used_tokens),
                    format_num(ctx.context_window_size)
                ),
            ));
            let q_color = theme::actor_hue(&roster_row.id);
            let ctx_data = vec![
                ("Used".to_string(), ctx.used_percentage as f64),
                (
                    "Remaining".to_string(),
                    100.0 - (ctx.used_percentage as f64).min(100.0),
                ),
            ];
            stats_block = stats_block.child(
                div().h(px(60.0)).w_full().child(
                    BarChart::new(ctx_data)
                        .id(format!("info-ctx-chart-{}", roster_row.id))
                        .name("Context %")
                        .band(|d| d.0.clone())
                        .value(|d| d.1)
                        .fill(move |d, _, _, _| -> gpui::Background {
                            if d.0 == "Used" {
                                q_color.into()
                            } else {
                                gpui::rgba(0x00000033).into()
                            }
                        }),
                ),
            );
        }

        if !q_stats.spend_history.is_empty() {
            let q_color = theme::actor_hue(&roster_row.id);
            stats_block = stats_block.child(
                div().h(px(100.0)).w_full().mt_2().child(
                    LineChart::new(q_stats.spend_history.clone())
                        .id(format!("info-spend-chart-{}", roster_row.id))
                        .name("Fresh Spent")
                        .x(|d| format!("T{}", d.turn))
                        .y(|d| d.fresh as f64)
                        .stroke(q_color),
                ),
            );
        }
        if !q_stats.quota.is_empty() {
            for bucket in q_stats.quota {
                stats_block =
                    stats_block.child(div().text_xs().text_color(theme::text_muted()).child(
                        format!(
                            "Quota [{}]: {:.1}% remaining",
                            bucket.key,
                            bucket.remaining_fraction * 100.0
                        ),
                    ));
            }
        }

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000066))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.info_panel = None;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .occlude()
                    .w(px(400.0))
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .rounded_lg()
                    .p_4()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {}) // Prevent overlay dismiss on inner click
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::actor_hue(qid))
                            .child(qid.clone()),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .mt_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child(format!("Flavor: {}", flavor_str)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child(format!("Provider: {}", roster_row.provider)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child(format!("Agent: {}", agent_str)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child(format!("Model: {}", model_str)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child(format!("Transport: {}", transport_str)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child(format!("Mode: {}", mode_str)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child(format!("State: {}", enabled_str)),
                            ),
                    )
                    .child(stats_block),
            )
    }

    /// Re-read the field; if it grew, re-project and repaint. Comparing event
    /// count to the current row count is a cheap change check (projection emits
    /// exactly one row per event), so an unchanged field costs only a read.
    fn reload_if_changed(&mut self, cx: &mut Context<Self>) {
        // Only reproject on a successful read — a transient read error must not
        // blank the current view (which would flash to empty, then repopulate).
        if let Ok(events) = io::read_events(&self.path) {
            let mut changed = false;
            if events.len() != self.view.messages.len() {
                // Decide *before* the content grows: if the user is parked at the
                // bottom, keep them there as the new message lands; if they've
                // scrolled up to read history, leave their position alone.
                let follow = self.chat_at_bottom();
                let old_log_count = self.view.messages.len();
                self.view = model::project_with_team(&events, &self.team);

                let old_chat_count = self.chat_message_ixs.len();
                self.chat_message_ixs = self
                    .view
                    .messages
                    .iter()
                    .enumerate()
                    .filter_map(|(ix, m)| (m.kind_label == "message").then_some(ix))
                    .collect();
                let new_chat_count = self.chat_message_ixs.len();
                let new_log_count = self.view.messages.len();

                if new_chat_count > old_chat_count {
                    self.chat_list_state.splice(
                        old_chat_count..old_chat_count,
                        new_chat_count - old_chat_count,
                    );
                } else if new_chat_count < old_chat_count {
                    // Should not happen since field is append-only, but just in case
                    self.chat_list_state.reset(new_chat_count);
                }

                if new_log_count > old_log_count {
                    self.log_list_state.splice(
                        old_log_count..old_log_count,
                        new_log_count - old_log_count,
                    );
                } else if new_log_count < old_log_count {
                    self.log_list_state.reset(new_log_count);
                }

                if follow {
                    for scroll in &self.chat_scrolls {
                        scroll.scroll_to_bottom();
                    }
                    self.chat_list_state
                        .scroll_to_reveal_item(new_chat_count.saturating_sub(1));
                    self.log_list_state
                        .scroll_to_reveal_item(new_log_count.saturating_sub(1));
                }
                changed = true;
            }
            if self.right_rail_tab == RightRailTab::Changes {
                let root = crate::vcs::repo_root_of(&self.path);
                let diff = crate::vcs::working_diff(root);
                if diff != self.working_diff {
                    self.working_diff = diff;
                    changed = true;
                }
            }
            // The file tree is a live view of the disk, not a boot-time snapshot:
            // rescan while it is on screen, exactly as the Changes pane does.
            if self.right_rail_tab == RightRailTab::FileTree {
                let root = crate::vcs::repo_root_of(&self.path);
                let files = crate::sys::list_workspace_files(root);
                if files != self.file_tree_paths {
                    *self.completion_files.borrow_mut() = files.clone();
                    self.file_tree_paths = files;
                    changed = true;
                }
            }
            if changed {
                cx.notify();
            }
        }
    }

    /// Whether the chat viewport is scrolled to (or within a message-height of)
    /// the bottom. `offset.y` grows more negative scrolling down and bottoms out
    /// at `-max_offset.y`, so their sum is ~0 at the bottom.
    fn chat_at_bottom(&self) -> bool {
        let scroll = &self.chat_scrolls[self.chat_tab.index()];
        let off = scroll.offset().y;
        let max = scroll.max_offset().y;
        off + max <= px(48.0)
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
        if let InputEvent::Change = event {
            let input = input.clone();
            window.on_next_frame(move |window, cx| {
                input.update(cx, |state, cx| {
                    let pos = state.cursor_position();
                    state.set_cursor_position(pos, window, cx);
                });
            });
            return;
        }

        let InputEvent::PressEnter { shift, .. } = event else {
            return;
        };
        println!("App received PressEnter! shift={}", shift);
        if *shift {
            return;
        }
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }

        if text.starts_with('/') && text.len() > 1 {
            let (cmd_name, args) = match text[1..].split_once(char::is_whitespace) {
                Some((n, a)) => (n, a.trim()),
                None => (&text[1..], ""),
            };

            if self.handle_chat_command(cmd_name, args, window, cx) {
                input.update(cx, |state, cx| state.set_value("", window, cx));
                return;
            }
        }

        // Write the raw text with `to: None`, leaving any `@mentions` in the body.
        // The daemon resolves addressees from the body, so ONE message can address
        // several quarks ("@opus do X and @agy do Y") — each is fanned out in turn.
        // (Stripping a single leading mention into `to` would drop the others.)
        let ev = Event::new(Actor::Human, None, Kind::Message { body: text });
        if let Err(e) = io::append_event(&self.path, &ev) {
            eprintln!("chamber: failed to append steering message: {e}");
            return;
        }

        input.update(cx, |state, cx| state.set_value("", window, cx));
        let events = io::read_events(&self.path).unwrap_or_default();
        let old_log_count = self.view.messages.len();
        self.view = model::project_with_team(&events, &self.team);

        let old_chat_count = self.chat_message_ixs.len();
        self.chat_message_ixs = self
            .view
            .messages
            .iter()
            .enumerate()
            .filter_map(|(ix, m)| (m.kind_label == "message").then_some(ix))
            .collect();
        let new_chat_count = self.chat_message_ixs.len();
        let new_log_count = self.view.messages.len();
        
        if new_chat_count > old_chat_count {
            self.chat_list_state.splice(
                old_chat_count..old_chat_count,
                new_chat_count - old_chat_count,
            );
        }
        if new_log_count > old_log_count {
            self.log_list_state.splice(
                old_log_count..old_log_count,
                new_log_count - old_log_count,
            );
        }

        // The human just spoke — always snap to their new message.
        for scroll in &self.chat_scrolls {
            scroll.scroll_to_bottom();
        }
        self.chat_list_state
            .scroll_to_reveal_item(new_chat_count.saturating_sub(1));
        self.log_list_state
            .scroll_to_reveal_item(new_log_count.saturating_sub(1));
        cx.notify();
    }
    fn handle_chat_command(
        &mut self,
        cmd: &str,
        args: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match cmd {
            "team-brainstorm" => {
                let body = format!("@team Let's brainstorm. {args}").trim().to_string();
                let ev = Event::new(Actor::Human, None, Kind::Message { body });
                if let Err(e) = io::append_event(&self.path, &ev) {
                    eprintln!("chamber: failed to append team-brainstorm message: {e}");
                } else {
                    let events = io::read_events(&self.path).unwrap_or_default();
                    self.view = model::project_with_team(&events, &self.team);
                    
                    let old_chat_count = self.chat_message_ixs.len();
                    self.chat_message_ixs = self
                        .view
                        .messages
                        .iter()
                        .enumerate()
                        .filter_map(|(ix, m)| (m.kind_label == "message").then_some(ix))
                        .collect();
                    let new_chat_count = self.chat_message_ixs.len();
                    if new_chat_count > old_chat_count {
                        self.chat_list_state.splice(
                            old_chat_count..old_chat_count,
                            new_chat_count - old_chat_count,
                        );
                    }
                    for scroll in &self.chat_scrolls {
                        scroll.scroll_to_bottom();
                    }
                    self.chat_list_state.scroll_to_reveal_item(new_chat_count.saturating_sub(1));
                    cx.notify();
                }
                true
            }
            _ => {
                // If it contains a slash, it's probably a path. 
                // Return false to let it pass through as a normal message.
                if cmd.contains('/') {
                    return false;
                }
                // Later we could show a local error message for unknown commands.
                false
            }
        }
    }

    fn handle_context_menu_action(&mut self, action: ContextMenuAction, cx: &mut Context<Self>) {
        match action {
            ContextMenuAction::QuarkInfo(id) => {
                self.info_panel = Some(id);
            }
            ContextMenuAction::ToggleQuark(id) => {
                self.toggle_quark_enabled(&id, cx);
            }
            ContextMenuAction::SetFlavor(id, flavor) => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let team_path = hadron_lattice::team_for_field(&self.path)
                    .unwrap_or_else(|| repo_root.join(".hadron").join("team.json"));
                let mut team = hadron_lattice::load_team(&team_path);

                let mut new_team = team.clone();
                if let Some(seat) = new_team.quarks.iter_mut().find(|s| s.id.0 == id) {
                    seat.flavor = flavor;

                    let orchestrators = new_team
                        .quarks
                        .iter()
                        .filter(|s| s.flavor == hadron_lattice::Flavor::Orchestrator)
                        .count();
                    if orchestrators > 0 {
                        let _ = hadron_lattice::save_team(&team_path, &new_team);
                        let events = io::read_events(&self.path).unwrap_or_default();
                        self.team = new_team;
                        self.view = model::project_with_team(&events, &self.team);
                        cx.notify();
                    } else {
                        eprintln!(
                            "Refusing to change flavor of {}: cannot have zero orchestrators.",
                            id
                        );
                    }
                }
            }
            ContextMenuAction::OpenFile(path) => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                if let Some(content) = crate::sys::read_workspace_file(&repo_root, &path) {
                    self.file_tree_open = Some((path, content));
                }
            }
            ContextMenuAction::CopyPath(path) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(path));
            }
            ContextMenuAction::OpenInEditor(path) => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let full_path = repo_root.join(path);

                #[cfg(target_os = "macos")]
                let default_cmd = "open";
                #[cfg(target_os = "windows")]
                let default_cmd = "explorer";
                #[cfg(target_os = "linux")]
                let default_cmd = "xdg-open";

                let editor = std::env::var("EDITOR").unwrap_or_else(|_| default_cmd.to_string());
                let _ = std::process::Command::new(&editor).arg(&full_path).spawn();
            }
            ContextMenuAction::OpenInFolder(path) => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let full_path = repo_root.join(path);
                let target = if full_path.is_file() {
                    full_path.parent().unwrap_or(&full_path).to_path_buf()
                } else {
                    full_path
                };

                #[cfg(target_os = "macos")]
                let cmd = "open";
                #[cfg(target_os = "windows")]
                let cmd = "explorer";
                #[cfg(target_os = "linux")]
                let cmd = "xdg-open";

                let _ = std::process::Command::new(cmd).arg(&target).spawn();
            }
        }
        cx.notify();
    }
}

impl Render for Chamber {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Track the window's geometry so it can be restored next launch. The write
        // is debounced, not immediate: a drag or resize re-renders every frame, and
        // saving inline here would put a `chamber.json` write on the render thread
        // ~60×/sec. Updating `prefs` in memory is free; the timer coalesces the
        // burst into a single trailing write once the geometry settles.
        if let gpui::WindowBounds::Windowed(bounds) = window.window_bounds() {
            let wb = config::WindowBoundsPrefs {
                x: bounds.origin.x.into(),
                y: bounds.origin.y.into(),
                width: bounds.size.width.into(),
                height: bounds.size.height.into(),
            };
            if self.prefs.window_bounds.as_ref() != Some(&wb) {
                self.prefs.window_bounds = Some(wb);
                if !self.bounds_save_pending {
                    self.bounds_save_pending = true;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(500))
                            .await;
                        let _ = this.update(cx, |this, _cx| {
                            this.bounds_save_pending = false;
                            // Saves whatever the geometry settled on, not the value
                            // that happened to trip the timer.
                            let _ = config::save(&this.prefs);
                        });
                    })
                    .detach();
                }
            }
        }

        // Round the full-height content itself to match the client frame, rather
        // than the (too-short) top/bottom strips — a 24px status bar can't reach
        // the ~20px radius, so its square corners poked past the frame's arc. The
        // strips are now transparent; the content's own rounded fill owns all four
        // corners. Zero on any tiled edge, so a maximized/snapped window stays square.
        let (top_radius, bottom_radius) = frame_corner_radii(window);
        let titlebar = self.titlebar(window, cx);
        let body = self.body(cx);
        let status = self.status_bar(cx);
        let overlay = self.palette_open.then(|| self.palette_overlay(cx));
        let settings = self.settings_open.then(|| self.settings_overlay(cx));
        let info = self
            .info_panel
            .is_some()
            .then(|| self.info_panel_overlay(cx));
        let about = self.about_open.then(|| self.about_overlay(cx));

        let content = v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_toggle_palette))
            .on_action(cx.listener(|this, _: &CycleMode, _, cx| this.cycle_global_mode(cx)))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme::bg_elevated())
            .rounded_tl(top_radius)
            .rounded_tr(top_radius)
            .rounded_bl(bottom_radius)
            .rounded_br(bottom_radius)
            .text_color(theme::text())
            .child(titlebar)
            .child(body)
            .child(status)
            .children(overlay)
            .children(settings)
            .children(info)
            .children(about);

        let wrapped_content = crate::window_frame::window_frame(window, cx, content);

        div().size_full().child(wrapped_content).into_any_element()
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
            .bg(theme::bg_base()) // darker than the titlebar → a recessed search field
            .text_sm()
            .text_color(theme::text_muted())
            .hover(|s| s.bg(theme::bg_surface()))
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
                    .child(menu_button(&cx.entity())),
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
    /// Left: an overall swarm-status tag. Right: the quark count and the global
    /// permission-mode tag (click to cycle Ask → Write → Auto → Bypass).
    fn status_bar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(swarm_status_tag(&self.view))
            .child(
                div().text_xs().text_color(theme::text_muted()).child(
                    self.path
                        .parent()
                        .and_then(|p| {
                            if p.file_name() == Some(std::ffi::OsStr::new(".hadron")) {
                                p.parent()
                            } else {
                                Some(p)
                            }
                        })
                        .unwrap_or(&self.path)
                        .display()
                        .to_string(),
                ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().child(format!("{} quark(s)", self.view.roster.len()))),
            )
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

        // Chat: flex (no fixed size) so it absorbs slack on resize, but floored
        // at CHAT_MIN so the terminal can't stretch over it entirely.
        group = group.child(
            resizable_panel()
                .size_range(px(CHAT_MIN)..px(TERMINAL_MAX))
                .child(self.chat_pane(cx)),
        );
        if !inspector_collapsed {
            group = group.child(
                resizable_panel()
                    .size(px(self.prefs.inspector_width))
                    // No real upper cap — the terminal/multitool can take most of
                    // the window; the chat's own min keeps it from vanishing.
                    .size_range(px(RAIL_MIN)..px(TERMINAL_MAX))
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
            // Bound the height so children shrink to it instead of growing to
            // their content — without this, the chat's min-content height
            // propagates up and nothing below can scroll (it just pushes down).
            .min_h_0()
            .w_full()
            .child(left)
            // The resizable group renders itself `size_full` (width: 100%). As a
            // direct flex item that resolves against the *whole* row, so it fights
            // its fixed-width siblings (the roster, and the collapsed terminal
            // strip) for the same pixels and pushes them past the right edge — the
            // strip vanishes and the chat's own right inset is clipped away. Boxing
            // it in a flex-1 (min-w-0) cell makes that 100% resolve against the
            // slack the siblings *leave*, which is what it always meant.
            .child(div().flex_1().min_w_0().h_full().child(group))
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
            .bg(theme::bg_elevated())
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
            .hover(|s| s.bg(theme::bg_surface()))
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

        // The roster rows, stacked to natural height so they scroll within the
        // rail rather than pushing the pinned Settings button off the bottom.
        let mut rows = v_flex().w_full().gap_2();
        for r in &self.view.roster {
            // The per-quark mode tag is clickable → cycle this quark's override.
            let qid = r.id.clone();
            let mode_el = div()
                .id(SharedString::from(format!("mode-{}", r.id)))
                .cursor_pointer()
                .flex_none()
                .on_click(cx.listener(move |this, _, _, cx| this.cycle_quark_mode(&qid, cx)))
                .child(mode_tag(r.mode, r.mode_is_override))
                .into_any_element();

            // The row needs a stable id: `ContextMenuExt` derives the popup's
            // ElementId from its parent's, and with no parent id it falls back to
            // a stack address — every row in the loop then shares one menu state.
            let row_el = div()
                .id(SharedString::from(format!("roster-row-{}", r.id)))
                .context_menu({
                    let qid_str = r.id.clone();
                    let enable_str = if r.enabled { "Disable" } else { "Enable" };
                    let r_flavor = r.flavor.clone();
                    let view = cx.entity().clone();
                    move |mut menu, _, _| {
                        let qid1 = qid_str.clone();
                        let view1 = view.clone();
                        menu = menu.item(PopupMenuItem::new("Info").on_click(move |_, window, cx| {
                            view1.update(cx, |this, cx| {
                                this.handle_context_menu_action(
                                    ContextMenuAction::QuarkInfo(qid1.clone()),
                                    cx,
                                );
                            });
                            window.refresh();
                        }));
                        let qid2 = qid_str.clone();
                        let view2 = view.clone();
                        menu =
                            menu.item(PopupMenuItem::new(enable_str).on_click(move |_, window, cx| {
                                view2.update(cx, |this, cx| {
                                    this.handle_context_menu_action(
                                        ContextMenuAction::ToggleQuark(qid2.clone()),
                                        cx,
                                    );
                                });
                                window.refresh();
                            }));
                        if let Some(flavor) = &r_flavor {
                            match flavor {
                                hadron_lattice::Flavor::Orchestrator => {
                                    let qid3 = qid_str.clone();
                                    let view3 = view.clone();
                                    menu = menu.item(PopupMenuItem::new("Make Worker").on_click(
                                        move |_, window, cx| {
                                            view3.update(cx, |this, cx| {
                                                this.handle_context_menu_action(
                                                    ContextMenuAction::SetFlavor(
                                                        qid3.clone(),
                                                        hadron_lattice::Flavor::Worker,
                                                    ),
                                                    cx,
                                                );
                                            });
                                            window.refresh();
                                        },
                                    ));
                                }
                                hadron_lattice::Flavor::Worker => {
                                    let qid4 = qid_str.clone();
                                    let view4 = view.clone();
                                    menu =
                                        menu
                                            .item(PopupMenuItem::new("Make Orchestrator").on_click(
                                            move |_, window, cx| {
                                                view4.update(cx, |this, cx| {
                                                    this.handle_context_menu_action(
                                                        ContextMenuAction::SetFlavor(
                                                            qid4.clone(),
                                                            hadron_lattice::Flavor::Orchestrator,
                                                        ),
                                                        cx,
                                                    );
                                                });
                                                window.refresh();
                                            },
                                        ));
                                }
                            }
                        }
                        menu
                    }
                })
                .child(roster_row(&self.resolve_identity(&r.id), r, mode_el));
            rows = rows.child(row_el);
        }
        if self.view.roster.is_empty() {
            rows = rows.child(
                div()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .child("no quarks yet"),
            );
        }

        v_flex()
            .w_full()
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::bg_elevated())
            .child(header) // pinned top
            .child(
                div()
                    .id("roster-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(rows),
            )
            // Settings pinned to the bottom of the rail.
            .child(self.settings_button(cx, false))
    }

    /// The center column: a segmented Chat / Log / Timeline tab bar over the
    /// selected view, with the human's message box pinned at the foot. The whole
    /// thing is a rounded, filled card that floats on the unified canvas.
    fn chat_pane(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.chat_tab;
        let tabs = TabBar::new("chat-tabs")
            .segmented()
            .selected_index(selected.index())
            .children(ChatTab::ALL.map(|t| {
                // The active tab reads as a dark cutout; give its label the pink
                // accent so the selection is unmistakable. Inactive tabs keep the
                // default muted label.
                if t.index() == selected.index() {
                    Tab::new().child(
                        div()
                            .text_color(theme::accent())
                            .child(t.label().to_string()),
                    )
                } else {
                    Tab::new().label(t.label())
                }
            }))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.chat_tab = ChatTab::from_index(*ix);
                cx.notify();
            }));

        let header = h_flex()
            .flex_none()
            .items_center()
            .px_3()
            .py_2()
            .child(tabs);

        // The scrolling viewport: the selected view stacks to its natural height
        // and scrolls *within* the card, instead of growing the card and pushing
        // the input (and the whole layout) off the bottom. The hover scrollbar is
        // an absolute sibling of the scrolled content (not a child of it, or it
        // would scroll away), reading the same handle.
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("chat-body-scroll")
                    .size_full()
                    .child(match selected {
                        ChatTab::Chat => self.chat_view(cx).into_any_element(),
                        ChatTab::Log => self.log_view(cx).into_any_element(),
                        ChatTab::Timeline => div()
                            .id("timeline-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.chat_scrolls[selected.index()])
                            .child(self.timeline_view())
                            .into_any_element(),
                        ChatTab::Session => div()
                            .id("session-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.chat_scrolls[selected.index()])
                            .child(self.session_view())
                            .into_any_element(),
                    }),
            )
            .child(div().absolute().top_0().right_0().bottom_0().when(
                selected != ChatTab::Chat,
                |this| {
                    this.child(
                        Scrollbar::vertical(&self.chat_scrolls[selected.index()])
                            .scrollbar_show(ScrollbarShow::Hover),
                    )
                },
            ));

        // The message box is only meaningful in Chat — you talk to the field
        // there. Log and Timeline are read-only views, so they get no input.
        let input =
            matches!(selected, ChatTab::Chat).then(|| {
                v_flex()
                    .flex_none()
                    .m_4()
                    .child(
                        h_flex()
                            .px_1()
                            .rounded_lg()
                            .bg(theme::input_bg())
                            .child(Input::new(&self.input)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .mt_2()
                            .items_center()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child("Global Mode:"),
                                    )
                                    .child(
                                        div()
                                            .id("global-mode")
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.cycle_global_mode(cx)
                                            }))
                                            .tooltip(|window, cx| {
                                                Tooltip::new(
                                                    "Permission mode — Shift+Tab or click to cycle",
                                                )
                                                .build(window, cx)
                                            })
                                            .child(mode_tag(self.view.global_mode, true)),
                                    ),
                            )
                            .child(
                                div().text_xs().text_color(theme::text_muted()).child(
                                    crate::vcs::repo_root_of(&self.path).display().to_string(),
                                ),
                            ),
                    )
            });

        // The floating chat card: darker + rounded, inset from the lighter
        // unified space that shows around it.
        let card = v_flex()
            .flex_1()
            .min_h_0()
            .rounded(INNER_RADIUS)
            .overflow_hidden()
            .bg(theme::bg_base())
            .child(header)
            .children(self.permission_toast(cx))
            .child(body)
            .children(input);

        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .p_2()
            .bg(theme::bg_elevated())
            .child(card)
    }

    /// The Chat tab: the conversation only (message events), styled like a chat
    /// with each author's avatar and name.
    fn chat_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.chat_message_ixs.is_empty() {
            return v_flex()
                .p_4()
                .child(empty_hint("No messages yet — say something below."))
                .into_any_element();
        }

        let weak_view = cx.entity().downgrade();

        // Wrap the virtual list with padding
        v_flex()
            .size_full()
            .p_4()
            .child(
                gpui::list(self.chat_list_state.clone(), move |ix, _window, cx| {
                    if let Some(view) = weak_view.upgrade() {
                        view.update(cx, |this, _cx| {
                            if let Some(&real_ix) = this.chat_message_ixs.get(ix) {
                                if let Some(m) = this.view.messages.get(real_ix) {
                                    let mut add_divider = false;
                                    if ix > 0 {
                                        if let Some(&prev_real_ix) = this.chat_message_ixs.get(ix - 1) {
                                            if let Some(prev_m) = this.view.messages.get(prev_real_ix) {
                                                if prev_m.ts.date_naive() != m.ts.date_naive() {
                                                    add_divider = true;
                                                }
                                            }
                                        }
                                    } else {
                                        add_divider = true;
                                    }
                                    
                                    let mut row = div().pb(px(16.0));
                                    if add_divider {
                                        let label = crate::model::date_divider_label(
                                            m.ts.date_naive(),
                                            chrono::Local::now().date_naive(),
                                        );
                                        row = row.child(
                                            div().flex().items_center().justify_center().pt_2().pb_6().child(
                                                div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(theme::text_muted()).child(label)
                                            )
                                        );
                                    }
                                    
                                    return row
                                        .child(this.chat_message_row(
                                            &this.resolve_identity(&m.from),
                                            m,
                                            real_ix,
                                            &this.view.roster,
                                        ))
                                        .into_any_element();
                                }
                            }
                            div().into_any_element()
                        })
                    } else {
                        div().into_any_element()
                    }
                })
                .size_full(),
            )
            .into_any_element()
    }

    /// The Log tab: every event on the field, compact (the raw activity).
    fn log_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.view.messages.is_empty() {
            return v_flex().gap_3().p_4()
                .child(empty_hint("The field is empty."))
                .into_any_element();
        }

        let weak_view = cx.entity().downgrade();

        v_flex()
            .size_full()
            .p_4()
            .child(
                gpui::list(self.log_list_state.clone(), move |ix, _window, cx| {
                    if let Some(view) = weak_view.upgrade() {
                        view.update(cx, |this, cx| {
                            if let Some(m) = this.view.messages.get(ix) {
                                let mut add_divider = false;
                                if ix > 0 {
                                    if let Some(prev_m) = this.view.messages.get(ix - 1) {
                                        if prev_m.ts.date_naive() != m.ts.date_naive() {
                                            add_divider = true;
                                        }
                                    }
                                } else {
                                    add_divider = true;
                                }
                                
                                let m_clone = m.clone();
                                let roster_clone = this.view.roster.clone();
                                
                                let mut row = div().pb(px(16.0));
                                if add_divider {
                                    let label = crate::model::date_divider_label(
                                        m.ts.date_naive(),
                                        chrono::Local::now().date_naive(),
                                    );
                                    row = row.child(
                                        div().flex().items_center().justify_center().pt_2().pb_6().child(
                                            div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(theme::text_muted()).child(label)
                                        )
                                    );
                                }
                                
                                return row
                                    .child(this.message_row(&m_clone, ix, &roster_clone, cx))
                                    .into_any_element();
                            }
                            div().into_any_element()
                        })
                    } else {
                        div().into_any_element()
                    }
                })
                .size_full(),
            )
            .into_any_element()
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
            return col.child(empty_hint(
                "No activity yet — the timeline fills as quarks work.",
            ));
        }

        let current = steps.len().saturating_sub(1);
        let stepper = Stepper::new("timeline")
            .vertical()
            .selected_index(current)
            .items(steps.into_iter().map(|m| {
                StepperItem::new()
                    .pb_6()
                    .icon(kind_icon(m.kind_label))
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(theme::actor_hue(&m.from))
                                            .child(m.from.clone())
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(format!("· {}", m.kind_label))
                                    )
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

    fn session_view(&self) -> impl IntoElement {
        let stats = self.view.session_stats();

        let mut col = v_flex().p_4().gap_4();
        col = col.child(
            v_flex()
                .gap_1()
                .p_3()
                .bg(theme::bg_surface_raised())
                .rounded_md()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme::text())
                        .child("Session Totals"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(format!("Turns: {}", stats.total_turns)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(format!(
                            "Spent: {} fresh, {} cached{}",
                            format_num(stats.total_fresh),
                            format_num(stats.total_cached),
                            if let Some(c) = stats.total_cost_usd { format!(" (${:.2})", c) } else { "".to_string() }
                        )),
                ),
        );

        if !stats.per_quark.is_empty() {
            let mut fresh_data = Vec::new();
            for (q, s) in &stats.per_quark {
                fresh_data.push((q.clone(), s.fresh as f64));
            }
            col = col.child(
                v_flex()
                    .gap_1()
                    .p_3()
                    .bg(theme::bg_surface_raised())
                    .rounded_md()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text())
                            .child("Fresh Spend per Quark"),
                    )
                    .child(
                        div().h(px(150.0)).w_full().child(
                            BarChart::new(fresh_data)
                                .id("session-fresh-chart")
                                .name("Fresh Tokens")
                                .band(|d| d.0.clone())
                                .value(|d| d.1)
                                .fill(move |_, _, _, _| -> gpui::Background {
                                    theme::accent().into()
                                }),
                        ),
                    ),
            );
        }

        for (q, s) in &stats.per_quark {
            let q_color = theme::actor_hue(q);
            let mut block = v_flex()
                .gap_1()
                .p_3()
                .bg(theme::bg_surface_raised())
                .rounded_md()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(q_color)
                        .child(q.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(format!("Turns: {}", s.turns)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(format!(
                            "Spent: {} fresh, {} cached",
                            format_num(s.fresh),
                            format_num(s.cached)
                        )),
                );

            if !s.spend_history.is_empty() {
                block = block.child(
                    div().h(px(100.0)).w_full().mt_2().child(
                        LineChart::new(s.spend_history.clone())
                            .id(format!("spend-chart-{}", q))
                            .name("Fresh Spent")
                            .x(|d| format!("T{}", d.turn))
                            .y(|d| d.fresh as f64)
                            .stroke(q_color),
                    ),
                );
            }

            if let Some(ctx) = &s.context {
                block = block.child(div().text_xs().text_color(theme::text_muted()).child(
                    format!(
                        "Context: {:.1}% ({} / {})",
                        ctx.used_percentage,
                        format_num(ctx.used_tokens),
                        format_num(ctx.context_window_size)
                    ),
                ));
                let ctx_data = vec![
                    ("Used".to_string(), ctx.used_percentage as f64),
                    (
                        "Remaining".to_string(),
                        100.0 - (ctx.used_percentage as f64).min(100.0),
                    ),
                ];
                block = block.child(
                    div().h(px(60.0)).w_full().child(
                        BarChart::new(ctx_data)
                            .id(format!("ctx-chart-{}", q))
                            .name("Context %")
                            .band(|d| d.0.clone())
                            .value(|d| d.1)
                            .fill(move |d, _, _, _| -> gpui::Background {
                                if d.0 == "Used" {
                                    q_color.into()
                                } else {
                                    gpui::rgba(0x00000033).into()
                                }
                            }),
                    ),
                );
            }
            // An empty quota list means the provider has no quota concept — not that
            // the quota is spent. Say nothing rather than render a zero.
            for bucket in &s.quota {
                block = block.child(div().text_xs().text_color(theme::text_muted()).child(
                    format!(
                        "Quota [{}]: {:.1}% remaining",
                        bucket.key,
                        bucket.remaining_fraction * 100.0
                    ),
                ));
            }
            col = col.child(block);
        }
        col
    }

    /// The right rail: the swappable Terminal / File Tree / Changes pane.
    /// (Internally still `Rail::Inspector` for collapse/size.)
    fn terminal_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.right_rail_tab;

        let tabs = TabBar::new("right-rail-tabs")
            .segmented()
            .selected_index(selected.index())
            .children(RightRailTab::ALL.map(|t| {
                if t.index() == selected.index() {
                    Tab::new().child(
                        div()
                            .text_color(theme::accent())
                            .child(t.label().to_string()),
                    )
                } else {
                    Tab::new().child(div().child(t.label()))
                }
            }))
            .on_click(cx.listener(move |this, ix: &usize, _window, cx| {
                this.right_rail_tab = RightRailTab::from_index(*ix);
                if this.right_rail_tab == RightRailTab::Changes {
                    let root = crate::vcs::repo_root_of(&this.path);
                    this.working_diff = crate::vcs::working_diff(root);
                }
                cx.notify();
            }));

        let header = h_flex()
            .id("inspector-toggle")
            .w_full()
            .justify_between()
            .items_center()
            .px_3()
            .py_2()
            .text_sm()
            .text_color(theme::text_muted())
            .child(tabs)
            .child(Icon::new(IconName::PanelRightClose).small())
            .active(|s| s.opacity(0.6))
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_rail(Rail::Inspector, window, cx)),
            );

        let content = match selected {
            RightRailTab::Terminal => {
                let risk_notice = v_flex()
                    .p_3()
                    .gap_2()
                    .bg(theme::danger().opacity(0.1))
                    .border_1()
                    .border_color(theme::danger().opacity(0.5))
                    .rounded_md()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(theme::danger()).child("⚠️ RISK: Arbitrary Process Execution"))
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_secondary())
                            .child("This terminal executes subprocesses under the authority of the host user. To prevent unauthorized execution when Bypass is disabled, all quark-issued commands must pause and require explicit human approval via the interaction gate.")
                    );

                let mut history = v_flex().w_full().gap_2();
                for (cwd, cmd, out) in &self.terminal_history {
                    let display_cwd = cwd.replace(&std::env::var("HOME").unwrap_or_default(), "~");
                    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
                    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "local".to_string());
                    let prompt = format!("{}@{}: {}$ {}", user, host, display_cwd, cmd);
                    let mut block = v_flex().gap_1().child(
                        div()
                            .text_sm()
                            .font_family("Cascadia Code")
                            .text_color(theme::accent())
                            .child(prompt),
                    );
                    if !out.is_empty() {
                        block = block.child(
                            div()
                                .text_xs()
                                .font_family("Cascadia Code")
                                .text_color(theme::text_muted())
                                .child(out.clone()),
                        );
                    }
                    history = history.child(block);
                }

                let current_cwd = self
                    .terminal
                    .as_ref()
                    .map(|t| t.cwd.clone())
                    .unwrap_or_else(|| {
                        crate::vcs::repo_root_of(&self.path)
                            .to_string_lossy()
                            .into_owned()
                    });
                let display_cwd =
                    current_cwd.replace(&std::env::var("HOME").unwrap_or_default(), "~");
                let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
                let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "local".to_string());
                let prompt_str = format!("{}@{}: {}$ ", user, host, display_cwd);

                v_flex()
                    .flex_1()
                    .p_3()
                    .gap_4()
                    .child(risk_notice)
                    .child(
                        div()
                            .id("terminal-scroll")
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(
                                div()
                                    .id("terminal-history-scroll")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.terminal_scroll)
                                    .child(history),
                            )
                            .child(
                                div().absolute().top_0().bottom_0().right_0().child(
                                    Scrollbar::vertical(&self.terminal_scroll)
                                        .scrollbar_show(ScrollbarShow::Hover),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .p_2()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(theme::border())
                            .rounded_md()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_family("Cascadia Code")
                                    .text_color(theme::accent())
                                    .child(prompt_str),
                            )
                            .child(div().flex_1().child(Input::new(&self.terminal_input))),
                    )
                    .into_any_element()
            }
            RightRailTab::FileTree => {
                let mut list = v_flex().w_full();
                if let Some((path, content)) = &self.file_tree_open {
                    list = list
                        .child(
                            h_flex()
                                .justify_between()
                                .items_center()
                                .p_2()
                                .bg(theme::bg_surface_raised())
                                .child(div().text_color(theme::text()).child(path.clone()))
                                .child(text_button("close-file", "Close").on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.file_tree_open = None;
                                        cx.notify();
                                    },
                                ))),
                        )
                        .child(
                            div()
                                .id("file-tree-open-container")
                                .flex_1()
                                .min_h_0()
                                .relative()
                                .child(
                                    div()
                                        .id("file-tree-open")
                                        .size_full()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.file_tree_open_scroll)
                                        .p_2()
                                        .bg(theme::input_bg())
                                        .text_color(theme::text())
                                        // Use a fixed index like usize::MAX for the file tree markdown cache
                                        .child(self.markdown_body(
                                            "file-tree-open",
                                            usize::MAX,
                                            content,
                                            &[],
                                        )),
                                )
                                .child(
                                    div().absolute().top_0().bottom_0().right_0().child(
                                        Scrollbar::vertical(&self.file_tree_open_scroll)
                                            .scrollbar_show(ScrollbarShow::Hover),
                                    ),
                                ),
                        );

                    v_flex().flex_1().child(list).into_any_element()
                } else {
                    #[derive(Default)]
                    struct FileTreeNode {
                        children: std::collections::BTreeMap<String, FileTreeNode>,
                        is_file: bool,
                        full_path: String,
                    }
                    impl FileTreeNode {
                        fn insert(&mut self, path: &str, full_path: &str) {
                            let mut current = self;
                            let parts: Vec<&str> = path.split('/').collect();
                            for (i, part) in parts.iter().enumerate() {
                                let is_file = i == parts.len() - 1;
                                current =
                                    current.children.entry(part.to_string()).or_insert_with(|| {
                                        FileTreeNode {
                                            children: std::collections::BTreeMap::new(),
                                            is_file,
                                            full_path: if is_file {
                                                full_path.to_string()
                                            } else {
                                                String::new()
                                            },
                                        }
                                    });
                            }
                        }
                    }

                    let mut root_node = FileTreeNode::default();
                    for file in &self.file_tree_paths {
                        root_node.insert(file, file);
                    }

                    let repo_root =
                        crate::vcs::repo_root_of(std::path::Path::new(&self.path)).to_path_buf();

                    fn render_node(
                        name: &str,
                        node: &FileTreeNode,
                        depth: usize,
                        cx: &mut Context<Chamber>,
                        repo_root: &std::path::PathBuf,
                        current_path: String,
                        expanded_set: &std::collections::HashSet<String>,
                    ) -> gpui::AnyElement {
                        let mut list = v_flex().w_full();
                        // root node has empty name and we don't render it directly
                        if name.is_empty() {
                            for (child_name, child_node) in &node.children {
                                let child_path = child_name.clone();
                                list = list.child(render_node(
                                    child_name,
                                    child_node,
                                    depth,
                                    cx,
                                    repo_root,
                                    child_path,
                                    expanded_set,
                                ));
                            }
                            return list.into_any_element();
                        }

                        let is_expanded = expanded_set.contains(&current_path);

                        // Stable per-path id — see the roster row: a context menu on
                        // an id-less element shares its state with every sibling.
                        let row = h_flex()
                            .id(SharedString::from(format!("tree-row-{}", node.full_path)))
                            .px_2()
                            .py_1()
                            .ml(gpui::px(depth as f32 * 12.0))
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                            .cursor_pointer()
                            .text_color(theme::text())
                            .font_family("Cascadia Code")
                            .text_size(gpui::px(13.56))
                            .gap_2()
                            .child(if node.is_file {
                                Icon::new(IconName::File)
                                    .small()
                                    .text_color(theme::text_muted())
                                    .into_any_element()
                            } else {
                                Icon::new(if is_expanded {
                                    IconName::FolderOpen
                                } else {
                                    IconName::Folder
                                })
                                .small()
                                .text_color(theme::text_muted())
                                .into_any_element()
                            })
                            .child(div().child(name.to_string()));

                        if node.is_file {
                            let file_name = node.full_path.clone();
                            let file_path = node.full_path.clone();
                            let repo = repo_root.clone();
                            let on_dbl_click = cx.listener(
                                move |this, event: &gpui::MouseDownEvent, _window, cx| {
                                    if event.button == gpui::MouseButton::Left
                                        && event.click_count == 2
                                    {
                                        if let Some(content) =
                                            crate::sys::read_workspace_file(&repo, &file_name)
                                        {
                                            this.file_tree_open =
                                                Some((file_name.clone(), content));
                                            cx.notify();
                                        }
                                    }
                                },
                            );

                            let path_clone = file_path.clone();
                            let view = cx.entity().clone();

                            list = list.child(
                                row.on_mouse_down(gpui::MouseButton::Left, on_dbl_click)
                                    .context_menu(move |mut menu, _, _| {
                                        let path1 = path_clone.clone();
                                        let view1 = view.clone();
                                        menu = menu.item(PopupMenuItem::new("Open File").on_click(
                                            move |_, window, cx| {
                                                view1.update(cx, |this, cx| {
                                                    this.handle_context_menu_action(
                                                        ContextMenuAction::OpenFile(path1.clone()),
                                                        cx,
                                                    )
                                                });
                                                window.refresh();
                                            },
                                        ));

                                        let path2 = path_clone.clone();
                                        let view2 = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Open in Editor").on_click(
                                                move |_, window, cx| {
                                                    view2.update(cx, |this, cx| {
                                                        this.handle_context_menu_action(
                                                            ContextMenuAction::OpenInEditor(
                                                                path2.clone(),
                                                            ),
                                                            cx,
                                                        )
                                                    });
                                                    window.refresh();
                                                },
                                            ),
                                        );

                                        let path3 = path_clone.clone();
                                        let view3 = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Open in Folder").on_click(
                                                move |_, window, cx| {
                                                    view3.update(cx, |this, cx| {
                                                        this.handle_context_menu_action(
                                                            ContextMenuAction::OpenInFolder(
                                                                path3.clone(),
                                                            ),
                                                            cx,
                                                        )
                                                    });
                                                    window.refresh();
                                                },
                                            ),
                                        );

                                        let path4 = path_clone.clone();
                                        let view4 = view.clone();
                                        menu = menu.item(PopupMenuItem::new("Copy Path").on_click(
                                            move |_, window, cx| {
                                                view4.update(cx, |this, cx| {
                                                    this.handle_context_menu_action(
                                                        ContextMenuAction::CopyPath(path4.clone()),
                                                        cx,
                                                    )
                                                });
                                                window.refresh();
                                            },
                                        ));

                                        menu
                                    }),
                            );
                        } else {
                            let toggle_path = current_path.clone();
                            let on_click = cx.listener(
                                move |this, event: &gpui::MouseDownEvent, _window, cx| {
                                    if event.button == gpui::MouseButton::Left {
                                        if this.file_tree_expanded.contains(&toggle_path) {
                                            this.file_tree_expanded.remove(&toggle_path);
                                        } else {
                                            this.file_tree_expanded.insert(toggle_path.clone());
                                        }
                                        cx.notify();
                                    }
                                },
                            );

                            let folder_path = node.full_path.clone();
                            let view = cx.entity().clone();

                            list = list.child(
                                row.on_mouse_down(gpui::MouseButton::Left, on_click)
                                    .context_menu(move |mut menu, _, _| {
                                        let path1 = folder_path.clone();
                                        let view1 = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Open in Editor").on_click(
                                                move |_, window, cx| {
                                                    view1.update(cx, |this, cx| {
                                                        this.handle_context_menu_action(
                                                            ContextMenuAction::OpenInEditor(
                                                                path1.clone(),
                                                            ),
                                                            cx,
                                                        )
                                                    });
                                                    window.refresh();
                                                },
                                            ),
                                        );

                                        let path2 = folder_path.clone();
                                        let view2 = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Open in Folder").on_click(
                                                move |_, window, cx| {
                                                    view2.update(cx, |this, cx| {
                                                        this.handle_context_menu_action(
                                                            ContextMenuAction::OpenInFolder(
                                                                path2.clone(),
                                                            ),
                                                            cx,
                                                        )
                                                    });
                                                    window.refresh();
                                                },
                                            ),
                                        );

                                        let path3 = folder_path.clone();
                                        let view3 = view.clone();
                                        menu = menu.item(PopupMenuItem::new("Copy Path").on_click(
                                            move |_, window, cx| {
                                                view3.update(cx, |this, cx| {
                                                    this.handle_context_menu_action(
                                                        ContextMenuAction::CopyPath(path3.clone()),
                                                        cx,
                                                    )
                                                });
                                                window.refresh();
                                            },
                                        ));

                                        menu
                                    }),
                            );

                            if is_expanded {
                                for (child_name, child_node) in &node.children {
                                    let child_path = format!("{}/{}", current_path, child_name);
                                    list = list.child(render_node(
                                        child_name,
                                        child_node,
                                        depth + 1,
                                        cx,
                                        repo_root,
                                        child_path,
                                        expanded_set,
                                    ));
                                }
                            }
                        }
                        list.into_any_element()
                    }

                    div()
                        .id("file-tree-list")
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .child(
                            div()
                                .id("file-tree-scroll-content")
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.file_tree_scroll)
                                .p_2()
                                .child(render_node(
                                    "",
                                    &root_node,
                                    0,
                                    cx,
                                    &repo_root,
                                    String::new(),
                                    &self.file_tree_expanded,
                                )),
                        )
                        .child(
                            div().absolute().top_0().bottom_0().right_0().child(
                                Scrollbar::vertical(&self.file_tree_scroll)
                                    .scrollbar_show(ScrollbarShow::Hover),
                            ),
                        )
                        .into_any_element()
                }
            }
            RightRailTab::Changes => {
                let diff_content = if let Some(diffs) = &self.working_diff {
                    if diffs.is_empty() {
                        div()
                            .p_4()
                            .text_color(theme::text_muted())
                            .child("No changes in working tree.")
                            .into_any_element()
                    } else {
                        let mut list = v_flex().w_full();
                        for (ix, file) in diffs.iter().enumerate() {
                            let title = h_flex()
                                .gap_2()
                                .items_center()
                                .child(div().child(file.path.clone()))
                                .child(
                                    div()
                                        .text_color(gpui::rgb(0x34d399))
                                        .child(format!("+{}", file.added)),
                                )
                                .child(
                                    div()
                                        .text_color(gpui::rgb(0xfb7185))
                                        .child(format!("-{}", file.removed)),
                                );

                            let is_open = self.changes_open_ixs.contains(&ix);

                            let header = h_flex()
                                .id(ix)
                                .w_full()
                                .justify_between()
                                .items_center()
                                .py_1()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.changes_open_ixs.contains(&ix) {
                                        this.changes_open_ixs.remove(&ix);
                                    } else {
                                        this.changes_open_ixs.insert(ix);
                                    }
                                    cx.notify();
                                }))
                                .child(title)
                                .child(
                                    Icon::new(if is_open {
                                        IconName::ChevronUp
                                    } else {
                                        IconName::ChevronDown
                                    })
                                    .small()
                                    .text_color(theme::text_muted()),
                                );

                            let mut row = v_flex().w_full().child(header);

                            if is_open {
                                let mut lines_list = v_flex()
                                    .w_full()
                                    .text_sm()
                                    .pt_2()
                                    .font_family("Cascadia Code");
                                for (_, hunk) in file.hunks.iter().enumerate() {
                                    lines_list = lines_list.child(
                                        div()
                                            .w_full()
                                            .px_2()
                                            .py_1()
                                            .text_color(theme::text_muted())
                                            .child(hunk.header.clone()),
                                    );
                                    for line in &hunk.lines {
                                        match line {
                                            crate::vcs::DiffLine::Context(c) => {
                                                lines_list = lines_list.child(
                                                    div()
                                                        .w_full()
                                                        .px_2()
                                                        .text_color(theme::text())
                                                        .child(format!(" {}", c)),
                                                );
                                            }
                                            crate::vcs::DiffLine::Added(a) => {
                                                lines_list = lines_list.child(
                                                    div()
                                                        .w_full()
                                                        .px_2()
                                                        .bg(gpui::rgba(0x34d39922))
                                                        .text_color(gpui::rgb(0x34d399))
                                                        .child(format!("+{}", a)),
                                                );
                                            }
                                            crate::vcs::DiffLine::Removed(r) => {
                                                lines_list = lines_list.child(
                                                    div()
                                                        .w_full()
                                                        .px_2()
                                                        .bg(gpui::rgba(0xfb718522))
                                                        .text_color(gpui::rgb(0xfb7185))
                                                        .child(format!("-{}", r)),
                                                );
                                            }
                                        }
                                    }
                                }
                                row = row.child(lines_list);
                            }

                            list = list.child(row.border_b_1().border_color(theme::border()));
                        }
                        list.into_any_element()
                    }
                } else {
                    div()
                        .p_4()
                        .text_color(theme::text_muted())
                        .child("Failed to load diff.")
                        .into_any_element()
                };

                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("changes-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.changes_scroll)
                            .p_3()
                            .text_sm()
                            .text_color(theme::text())
                            .child(diff_content),
                    )
                    .child(
                        div().absolute().top_0().bottom_0().right_0().child(
                            Scrollbar::vertical(&self.changes_scroll)
                                .scrollbar_show(ScrollbarShow::Hover),
                        ),
                    )
                    .into_any_element()
            }
        };

        let card = v_flex()
            .flex_1()
            .min_h_0()
            .rounded(INNER_RADIUS)
            .overflow_hidden()
            .bg(theme::bg_base())
            .child(header)
            .child(content);

        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .p_2()
            .bg(theme::bg_elevated())
            .child(card)
    }

    /// Answer an outstanding permission request by appending a human
    /// `PermissionGrant` (addressed back to the asking quark, so the daemon
    /// resumes it) — the same bus the quarks use. Mirrors [`Self::on_input_submit`].
    fn answer_permission(&mut self, approved: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.view.pending_permission.clone() else {
            return;
        };
        let ev = hadron_gatekeeper::grant(&pending, approved);
        if let Err(e) = io::append_event(&self.path, &ev) {
            eprintln!("chamber: failed to append permission grant: {e}");
            return;
        }
        let events = io::read_events(&self.path).unwrap_or_default();
        self.view = model::project_with_team(&events, &self.team);
        for scroll in &self.chat_scrolls {
            scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// The non-blocking permission toast: when a quark is waiting on the human,
    /// a banner drops in with Approve / Deny. `None` when nothing is pending.
    fn permission_toast(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let pending = self.view.pending_permission.as_ref()?;
        let text = format!(
            "⚠️ {} wants to: {} ({:?})",
            pending.quark.as_str(),
            pending.description,
            pending.risk,
        );
        Some(
            h_flex()
                .flex_none()
                .mx_4()
                .mt_2()
                .px_3()
                .py_2()
                .gap_3()
                .items_center()
                .rounded_lg()
                .bg(theme::bg_surface_raised())
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(theme::text())
                        .child(text),
                )
                .child(
                    text_button("perm-approve", "Approve")
                        .on_click(cx.listener(|this, _, _, cx| this.answer_permission(true, cx))),
                )
                // "Always allow" remembers this (quark, op) so Auto mode won't ask again.
                .child(
                    text_button("perm-always", "Always allow").on_click(
                        cx.listener(|this, _, _, cx| this.answer_permission_remember(cx)),
                    ),
                )
                .child(
                    text_button("perm-deny", "Deny")
                        .on_click(cx.listener(|this, _, _, cx| this.answer_permission(false, cx))),
                ),
        )
    }

    /// "Always allow" the pending op: append a *remembering* grant so the
    /// gatekeeper's allow-list auto-approves the same `(quark, op)` next time.
    fn answer_permission_remember(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.view.pending_permission.clone() else {
            return;
        };
        self.append_and_reload(hadron_gatekeeper::grant_remembering(&pending), cx);
    }

    /// Cycle the global default permission mode (Ask → Write → Auto → Bypass →
    /// Ask) by appending a global `ModeSet`. The daemon honours it next tick.
    fn cycle_global_mode(&mut self, cx: &mut Context<Self>) {
        let next = next_mode(self.view.global_mode);
        self.append_and_reload(
            Event::new(Actor::Human, None, Kind::ModeSet { mode: next }),
            cx,
        );
    }

    /// Cycle a single quark's permission mode by appending a per-quark `ModeSet`
    /// (addressed to it). This always creates/updates an explicit override.
    fn cycle_quark_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        let qid = QuarkId::new(id);
        let current = self
            .view
            .roster
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.mode)
            .unwrap_or_default();
        let next = next_mode(current);
        self.append_and_reload(
            Event::new(Actor::Human, Some(qid), Kind::ModeSet { mode: next }),
            cx,
        );
    }

    fn toggle_quark_enabled(&mut self, id: &str, cx: &mut Context<Self>) {
        let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        let team_path = hadron_lattice::team_for_field(&self.path)
            .unwrap_or_else(|| repo_root.join(".hadron").join("team.json"));
        let mut team = hadron_lattice::load_team(&team_path);
        let qid = QuarkId::new(id);
        if let Some(seat) = team.quarks.iter_mut().find(|s| s.id == qid) {
            seat.enabled = !seat.enabled;
            if let Err(e) = hadron_lattice::save_team(&team_path, &team) {
                eprintln!("chamber: failed to save team.json: {}", e);
            } else {
                let events = io::read_events(&self.path).unwrap_or_default();
                self.view = crate::model::project_with_team(&events, &team);
                cx.notify();
            }
        }
    }

    /// Append an event to the field and re-project the view (the shared write
    /// path for permission grants and mode changes — the same bus the quarks use).
    fn append_and_reload(&mut self, ev: Event, cx: &mut Context<Self>) {
        if let Err(e) = io::append_event(&self.path, &ev) {
            eprintln!("chamber: failed to append event: {e}");
            return;
        }
        let events = io::read_events(&self.path).unwrap_or_default();
        self.view = model::project_with_team(&events, &self.team);
        cx.notify();
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
        let default_name = if actor == "human" {
            "You".to_string()
        } else {
            let mut c = actor.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        };
        let name = stored
            .and_then(|i| i.display_name.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| default_name);
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

    /// The About dialog. Every value here is read from the build, not typed in: the
    /// version comes from the crate's own manifest, so it cannot drift from what
    /// shipped.
    fn about_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close = cx.listener(|this, _, _, cx| {
            this.about_open = false;
            cx.notify();
        });

        let row = |label: &'static str, value: String| {
            h_flex()
                .w_full()
                .justify_between()
                .gap_4()
                .text_sm()
                .child(div().text_color(theme::text_muted()).child(label))
                .child(div().text_color(theme::text()).child(value))
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.5))
            .child(
                v_flex()
                    .w(px(420.0))
                    .p_5()
                    .gap_3()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child("Hadron"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_muted())
                            .child("A multi-agent operating system. Quarks take turns in one shared workspace, on one shared field."),
                    )
                    .child(row("Version", env!("CARGO_PKG_VERSION").to_string()))
                    .child(row("Licence", "Apache-2.0".to_string()))
                    .child(row(
                        "Workspace",
                        crate::vcs::repo_root_of(&self.path)
                            .to_string_lossy()
                            .to_string(),
                    ))
                    .child(row("Quarks seated", self.view.roster.len().to_string()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child("Built on GPUI (Zed) and gpui-component (Longbridge), and speaks the Agent Client Protocol."),
                    )
                    .child(
                        div()
                            .id("about-close")
                            .mt_2()
                            .self_end()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(theme::bg_surface_raised())
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.85))
                            .text_sm()
                            .child("Close")
                            .on_click(close),
                    ),
            )
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
    fn settings_identity_mut(&mut self) -> Option<&mut Identity> {
        match &self.settings_target {
            SettingsTarget::Human => Some(&mut self.prefs.human),
            SettingsTarget::Quark(id) => Some(self.prefs.quarks.entry(id.clone()).or_default()),
            SettingsTarget::Providers => None,
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
        let (name, path, effort, mode) = {
            let key = self.settings_target.key();
            let mut eff = None;
            let mut mod_cfg = None;
            let id = if key == "human" {
                Some(&self.prefs.human)
            } else {
                if let Some(seat) = self.team.quarks.iter().find(|s| s.id.as_str() == key) {
                    eff = seat.effort.clone();
                    mod_cfg = seat.mode_config.clone();
                }
                self.prefs.quarks.get(key)
            };
            (
                id.and_then(|i| i.display_name.clone()).unwrap_or_default(),
                id.and_then(|i| i.image_path.clone()).unwrap_or_default(),
                eff.unwrap_or_default(),
                mod_cfg.unwrap_or_default(),
            )
        };
        self.settings_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.settings_path
            .update(cx, |s, cx| s.set_value(path, window, cx));
        self.settings_effort
            .update(cx, |s, cx| s.set_value(effort, window, cx));
        self.settings_mode_config
            .update(cx, |s, cx| s.set_value(mode, window, cx));
    }

    /// Write the editor inputs back into the current target identity and persist.
    fn commit_settings_inputs(&mut self, cx: &mut Context<Self>) {
        let name = self.settings_name.read(cx).value().trim().to_string();
        let path = self.settings_path.read(cx).value().trim().to_string();
        let effort_val = self.settings_effort.read(cx).value().trim().to_string();
        let mode_val = self.settings_mode_config.read(cx).value().trim().to_string();
        
        let key = self.settings_target.key();
        if key != "human" && key != "providers" {
            let qid = QuarkId::new(key);
            if let Some(seat) = self.team.quarks.iter_mut().find(|s| s.id == qid) {
                seat.effort = (!effort_val.is_empty()).then_some(effort_val);
                seat.mode_config = (!mode_val.is_empty()).then_some(mode_val);
                seat.display_name = (!name.is_empty()).then_some(name.clone());
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let team_path = hadron_lattice::team_for_field(&self.path)
                    .unwrap_or_else(|| repo_root.join(".hadron").join("team.json"));
                let _ = hadron_lattice::save_team(&team_path, &self.team);
            }
        }
        
        if let Some(id) = self.settings_identity_mut() {
            id.display_name = (!name.is_empty()).then_some(name);
            id.image_path = (!path.is_empty()).then_some(path);
            let _ = config::save(&self.prefs);
        }
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
        if let Some(id) = self.settings_identity_mut() {
            id.color = Some(format!("#{hex:06x}"));
            let _ = config::save(&self.prefs);
            cx.notify();
        }
    }

    /// Clear the current target's image (falling back to color + initials).
    fn clear_settings_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(identity) = self.settings_identity_mut() {
            identity.image_path = None;
            self.settings_path
                .update(cx, |s, cx| s.set_value("", window, cx));
            let _ = config::save(&self.prefs);
            cx.notify();
        }
    }

    /// Reset the current target to its code defaults.
    fn reset_settings_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.settings_target.clone() {
            SettingsTarget::Human => self.prefs.human = Identity::default(),
            SettingsTarget::Quark(id) => {
                self.prefs.quarks.remove(&id);
            }
            SettingsTarget::Providers => {}
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

        // Left nav: every editable identity — the human, then each quark.
        let mut nav = v_flex()
            .gap_0p5()
            .child(
                div()
                    .px_1()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("GLOBAL"),
            )
            .child(self.settings_nav_row(SettingsTarget::Providers, &target, cx))
            .child(
                div()
                    .px_1()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("IDENTITIES"),
            )
            .child(self.settings_nav_row(SettingsTarget::Human, &target, cx));
        for r in &self.view.roster {
            nav =
                nav.child(self.settings_nav_row(SettingsTarget::Quark(r.id.clone()), &target, cx));
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
            .child(div().text_color(preview.color).child(preview.name.clone()));

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
                    .border_color(if is_sel {
                        theme::text()
                    } else {
                        theme::border()
                    })
                    .hover(|s| s.border_color(theme::text_secondary()))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_settings_color(hex, cx))),
            );
        }

        // Left sidebar: a recessed, scrollable nav column of identities.
        let sidebar = v_flex()
            .flex_none()
            .w(px(190.0))
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::bg_base())
            .border_r(px(1.0))
            .border_color(theme::border())
            .child(div().px_1().text_color(theme::text()).child("Settings"))
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("SETTINGS"),
            )
            .child(
                div()
                    .id("settings-nav-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(nav),
            );

        // Right panel: header (who + close), the scrollable editor fields, and a
        // pinned footer (Reset / Done).
        let header = h_flex()
            .flex_none()
            .items_center()
            .justify_between()
            .child(div().text_color(theme::text_secondary()).child(
                if target == SettingsTarget::Providers {
                    "Providers".to_string()
                } else {
                    format!("Editing {}", preview.name)
                },
            ))
            .child(
                div()
                    .id("settings-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .rounded_full()
                    .text_color(theme::text_secondary())
                    .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
                    .child(Icon::new(IconName::WindowClose).small())
                    .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx))),
            );

        let fields = if target == SettingsTarget::Providers {
            self.providers_view(cx).into_any_element()
        } else {
            v_flex()
                .gap_4()
                .child(settings_field("Preview", preview_row.into_any_element()))
                .child(settings_field(
                    "Display name",
                    Input::new(&self.settings_name).into_any_element(),
                ))
                .child(settings_field(
                    "Effort",
                    Input::new(&self.settings_effort).into_any_element(),
                ))
                .child(settings_field(
                    "Mode",
                    Input::new(&self.settings_mode_config).into_any_element(),
                ))
                .child(settings_field("Color", swatches.into_any_element()))
                .child(settings_field(
                    "Image",
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1().child(Input::new(&self.settings_path)))
                        .child(text_button("settings-clear-img", "Clear").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.clear_settings_image(window, cx)
                            }),
                        ))
                        .into_any_element(),
                ))
                .into_any_element()
        };

        let footer = if target == SettingsTarget::Providers {
            div().into_any_element()
        } else {
            h_flex()
                .flex_none()
                .justify_between()
                .pt_1()
                .child(text_button("settings-reset", "Reset to default").on_click(
                    cx.listener(|this, _, window, cx| this.reset_settings_target(window, cx)),
                ))
                .child(
                    div()
                        .id("settings-done")
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme::accent())
                        .text_color(theme::text())
                        .hover(|s| s.opacity(0.9))
                        .active(|s| s.opacity(0.8))
                        .child("Done")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                        ),
                )
                .into_any_element()
        };
        let panel = v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .p_4()
            .gap_4()
            .child(header)
            .child(
                div()
                    .id("settings-fields-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(fields),
            )
            .child(footer);

        let card = h_flex()
            .occlude()
            .w_full()
            .h_full()
            .max_w(px(960.0))
            .max_h(px(640.0))
            .rounded_lg()
            .overflow_hidden()
            .bg(theme::bg_elevated())
            .border_1()
            .border_color(theme::border())
            .child(sidebar)
            .child(panel);

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .p_8()
            .flex()
            // Center on both axes deterministically (was relying on default
            // align + a top margin, which sank the card to the window's foot).
            .flex_col()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
            .child(card)
    }

    /// One row in the Settings identity nav: avatar + name, highlighted when it's
    /// the identity currently being edited.
    fn settings_nav_row(
        &self,
        who: SettingsTarget,
        current: &SettingsTarget,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let resolved = self.resolve_identity(who.key());
        let selected = &who == current;
        let id = SharedString::from(format!("settings-id-{}", who.key()));
        h_flex()
            .id(id)
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_1p5()
            .rounded_md()
            .bg(if selected {
                theme::bg_surface_raised()
            } else {
                theme::bg_base()
            })
            .hover(|s| s.bg(theme::bg_surface()))
            .child(if who == SettingsTarget::Providers {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Cpu).small())
                    .into_any_element()
            } else {
                identity_avatar(&resolved, 24.0).into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(if selected {
                        theme::text()
                    } else {
                        theme::text_secondary()
                    })
                    .child(if who == SettingsTarget::Providers {
                        "Providers".to_string()
                    } else {
                        resolved.name.clone()
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_settings_target(who.clone(), window, cx)
            }))
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

    fn providers_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.wizard_state {
            WizardState::None => {
                let mut list = v_flex().gap_3();
                for provider in &self.providers {
                    let (state_text, state_color) = match &provider.state {
                        ProviderState::NotConnected => {
                            ("Not Connected".to_string(), theme::text_muted())
                        }
                        ProviderState::Connecting => {
                            ("Connecting…".to_string(), theme::text_muted())
                        }
                        ProviderState::NeedsAuth(_) => ("Needs Auth".to_string(), theme::accent()),
                        ProviderState::Ready { model } => {
                            (format!("Ready ({})", model), gpui::rgb(0x22c55e))
                        }
                        ProviderState::Failed(e) => (format!("Failed: {}", e), theme::danger()),
                    };
                    list = list.child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_base()
                                            .text_color(theme::text())
                                            .child(provider.id.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(format!("Transport: {}", provider.transport)),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().size(px(8.0)).rounded_full().bg(state_color))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::text_secondary())
                                            .child(state_text),
                                    ),
                            ),
                    );
                }

                v_flex()
                    .size_full()
                    .gap_6()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(theme::text())
                                    .child("Configured Providers"),
                            )
                            .child(text_button("add-quark", "Add Quark").on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.wizard_state = WizardState::PickPreset;
                                    cx.notify();
                                },
                            ))),
                    )
                    .child(list)
            }
            WizardState::PickPreset => {
                let presets = hadron_gluon::adapter::registry::QuarkKind::available_presets()
                    .into_iter()
                    .map(|(id, name, cmd, args)| AgentDescriptor {
                        id: id.into(),
                        name: name.into(),
                        command: cmd.into(),
                        args: args.into_iter().map(String::from).collect(),
                    })
                    .collect::<Vec<_>>();

                let mut list = v_flex().gap_2();
                for preset in presets {
                    let preset_clone = preset.clone();
                    list = list.child(
                        h_flex()
                            .id(SharedString::from(format!("preset-{}", preset.id)))
                            .items_center()
                            .justify_between()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                            .cursor_pointer()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_base()
                                            .text_color(theme::text())
                                            .child(preset.name.clone()),
                                    )
                                    .child(div().text_xs().text_color(theme::text_muted()).child(
                                        format!("{} {}", preset.command, preset.args.join(" ")),
                                    )),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .child("Configure →"),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.wizard_state = WizardState::Connecting(
                                    preset_clone.clone(),
                                    ProviderState::NotConnected,
                                );
                                cx.notify();
                            })),
                    );
                }

                // Add custom option
                list = list.child(
                    h_flex()
                        .id("preset-custom")
                        .items_center()
                        .justify_between()
                        .p_4()
                        .rounded_lg()
                        .bg(theme::bg_surface())
                        .border_1()
                        .border_color(theme::border())
                        .hover(|s| s.bg(theme::bg_surface_raised()))
                        .cursor_pointer()
                        .child(
                            div()
                                .text_base()
                                .text_color(theme::text())
                                .child("Custom command…"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Configure →"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.wizard_state = WizardState::Connecting(
                                AgentDescriptor {
                                    id: "custom".into(),
                                    name: "Custom".into(),
                                    command: "".into(),
                                    args: vec![],
                                },
                                ProviderState::NotConnected,
                            );
                            cx.notify();
                        })),
                );

                v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-wizard", "← Back").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.wizard_state = WizardState::None;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child("Select a Preset"),
                    )
                    .child(list)
            }

            WizardState::Connecting(desc, state) => {
                let desc_clone = desc.clone();
                let state_ui = match state {
                    ProviderState::Connecting => v_flex()
                        .gap_4()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Connecting..."),
                        )
                        .into_any_element(),
                    ProviderState::NotConnected => {
                        v_flex()
                            .gap_4()
                            .child(text_button("connect-btn", "Connect").on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.wizard_state = WizardState::Connecting(
                                        desc_clone.clone(),
                                        ProviderState::Connecting,
                                    );
                                    cx.notify();

                                    // Connect = boot the agent and complete ACP's `initialize`.
                                    // The probe lives in the daemon (`hadron-gluon`), which is the
                                    // thing that will actually drive this agent — so the UI cannot
                                    // claim a provider works over a client the daemon never uses.
                                    let target = hadron_gluon::adapter::registry::AcpTarget {
                                        program: desc_clone.command.clone(),
                                        args: desc_clone.args.clone(),
                                    };
                                    let desc_for_task = desc_clone.clone();
                                    cx.spawn(
                                        |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                                            // The async block outlives the borrow, so it gets its own
                                            // handle on the app rather than holding a reference.
                                            let mut cx = cx.clone();
                                            async move {
                                                // Blocking boot, off the UI thread: a slow `npx` must not
                                                // freeze the window.
                                                let result = cx
                                                    .background_spawn(async move {
                                                        hadron_gluon::adapter::acp::probe(&target)
                                                    })
                                                    .await
                                                    .map_err(|e| e.to_string());

                                                this.update(&mut cx, |this, cx| {
                                                    let state = match result {
                                                        Ok(model) => ProviderState::Ready { model },
                                                        Err(e) => ProviderState::Failed(e),
                                                    };
                                                    this.wizard_state = WizardState::Connecting(
                                                        desc_for_task,
                                                        state,
                                                    );
                                                    cx.notify();
                                                })
                                                .ok();
                                            }
                                        },
                                    )
                                    .detach();
                                },
                            )))
                            .into_any_element()
                    }
                    ProviderState::NeedsAuth(methods) => {
                        let mut auth_list = v_flex().gap_2();
                        for method in methods {
                            let method_clone = method.clone();
                            let desc_inner = desc.clone();
                            auth_list = auth_list.child(
                                v_flex()
                                    .gap_2()
                                    .p_3()
                                    .border_1()
                                    .border_color(theme::border())
                                    .rounded_md()
                                    .child(
                                        div().text_color(theme::text()).child(method.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::text_muted())
                                            .child(method.description.clone()),
                                    )
                                    .child(
                                        text_button(
                                            &format!("auth-btn-{}", method.id),
                                            &method.name,
                                        )
                                        .on_click(cx.listener(
                                            move |this, _, _, cx| {
                                                this.wizard_state = WizardState::Connecting(
                                                    desc_inner.clone(),
                                                    ProviderState::Connecting,
                                                );
                                                cx.notify();

                                                let target = hadron_gluon::adapter::registry::AcpTarget {
                                                    program: desc_inner.command.clone(),
                                                    args: desc_inner.args.clone(),
                                                };
                                                let desc_for_task = desc_inner.clone();
                                                cx.spawn(
                                                    |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                                                        let mut cx = cx.clone();
                                                        async move {
                                                            let result = cx
                                                                .background_spawn(async move {
                                                                    hadron_gluon::adapter::acp::probe(&target)
                                                                })
                                                                .await
                                                                .map_err(|e| e.to_string());

                                                            this.update(&mut cx, |this, cx| {
                                                                let state = match result {
                                                                    Ok(model) => ProviderState::Ready { model },
                                                                    Err(e) => ProviderState::Failed(e),
                                                                };
                                                                this.wizard_state = WizardState::Connecting(
                                                                    desc_for_task,
                                                                    state,
                                                                );
                                                                cx.notify();
                                                            })
                                                            .ok();
                                                        }
                                                    },
                                                )
                                                .detach();
                                            },
                                        )),
                                    ),
                            );
                        }
                        auth_list.into_any_element()
                    }
                    ProviderState::Ready { model } => {
                        let desc_inner = desc.clone();
                        let state_inner = state.clone();
                        let model_inner = model.clone();
                        v_flex()
                            .gap_4()
                            .child(
                                div()
                                    .text_color(theme::accent())
                                    .child(format!("Ready! Model available: {}", model)),
                            )
                            .child(text_button("save-provider", "Save Provider").on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.providers.push(ConfiguredQuark {
                                        id: desc_inner.id.clone(),
                                        transport: "acp".to_string(),
                                        state: state_inner.clone(),
                                    });

                                    // An ACP seat, and it carries the command the wizard
                                    // just proved boots — so the daemon reaches this agent
                                    // over the same transport the human tested it on.
                                    this.team.quarks.push(hadron_lattice::Seat {
                                        id: hadron_lattice::QuarkId::new(&desc_inner.id),
                                        display_name: None,
                                        provider: desc_inner.id.clone(),
                                        model: model_inner.clone(),
                                        flavor: hadron_lattice::Flavor::Worker, // default flavor
                                        transport: hadron_lattice::Transport::Acp,
                                        command: Some(hadron_lattice::AcpCommand {
                                            program: desc_inner.command.clone(),
                                            args: desc_inner.args.clone(),
                                        }),
                                        // A seat the human just proved and saved is on.
                                        enabled: true,
                                        effort: None,
                                        mode_config: None,
                                    });
                                    let repo_root = crate::vcs::repo_root_of(&this.path).to_path_buf();
                                    let team_path = hadron_lattice::team_for_field(&this.path)
                                        .unwrap_or_else(|| repo_root.join(".hadron").join("team.json"));
                                    let _ = hadron_lattice::save_team(&team_path, &this.team);

                                    this.wizard_state = WizardState::None;
                                    cx.notify();
                                }),
                            ))
                            .into_any_element()
                    }
                    ProviderState::Failed(err) => div()
                        .text_color(theme::text_secondary())
                        .child(err.clone())
                        .into_any_element(),
                };

                v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-presets", "← Back").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.wizard_state = WizardState::PickPreset;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child(format!("Connecting to {}", desc.name)),
                    )
                    .child(state_ui)
            }
        }
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
/// The app menu behind the 3-line icon.
///
/// Every item here does something real. "Open Folder / Recent Projects" is
/// deliberately absent: the daemon is bound to one workspace at boot, so the chamber
/// alone cannot repoint the swarm at another one — an item that opened a folder the
/// quarks could not see would be a lie with a file dialog attached.
fn menu_button(chamber: &Entity<Chamber>) -> impl IntoElement {
    let view = chamber.clone();
    Button::new("app-menu")
        .ghost()
        .icon(Icon::new(IconName::Menu).small())
        .dropdown_menu(move |menu, _, _| {
            let palette = view.clone();
            let settings = view.clone();
            let folder = view.clone();
            let about = view.clone();
            menu.item(
                PopupMenuItem::new("Command Palette…").on_click(move |_, window, cx| {
                    palette.update(cx, |this, cx| {
                        this.on_toggle_palette(&TogglePalette, window, cx)
                    });
                }),
            )
            .item(
                PopupMenuItem::new("Settings…").on_click(move |_, window, cx| {
                    settings.update(cx, |this, cx| this.open_settings(window, cx));
                }),
            )
            .separator()
            .item(
                PopupMenuItem::new("Reveal Workspace in File Manager").on_click(
                    move |_, _, cx| {
                        folder.update(cx, |this, cx| {
                            this.handle_context_menu_action(
                                ContextMenuAction::OpenInFolder(String::from(".")),
                                cx,
                            );
                        });
                    },
                ),
            )
            .separator()
            .item(
                PopupMenuItem::new("About Hadron").on_click(move |_, _, cx| {
                    about.update(cx, |this, cx| {
                        this.about_open = true;
                        cx.notify();
                    });
                }),
            )
            .separator()
            .item(PopupMenuItem::new("Quit Hadron").on_click(|_, _, cx| cx.quit()))
        })
}

/// A circular window-control button (min / max / close) with circular hover.
fn control_button(id: &'static str, icon: IconName, is_close: bool) -> impl IntoElement {
    let hover_bg = if is_close {
        theme::danger()
    } else {
        theme::bg_surface_raised()
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
fn roster_row(id: &ResolvedIdentity, r: &RosterRow, mode_el: gpui::AnyElement) -> impl IntoElement {
    let name = id.name.clone();
    let label = theme::presence_label(r.state);
    let tip: SharedString = format!("{name} — {label}").into();

    // Legibility line: "provider · model" when the seat is in team.json, else
    // the presence label alone.
    let cap = |s: &str| {
        let mut c = s.chars();
        match c.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        }
    };

    let tokens = r.tokens;
    let tokens_str = if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}k", tokens / 1_000)
    } else {
        format!("{}", tokens)
    };

    let flavor_str = match &r.flavor {
        Some(hadron_lattice::Flavor::Orchestrator) => "Orchestrator",
        Some(hadron_lattice::Flavor::Worker) => "Worker",
        None => "",
    };

    let detail_1: SharedString = if r.provider.is_empty() && r.model.is_empty() {
        label.into()
    } else if r.model.is_empty() {
        cap(&r.provider).into()
    } else {
        format!("{} · {}", cap(&r.provider), cap(&r.model)).into()
    };

    let unknown_str = if r.unknown_turns > 0 {
        format!(" (+{} turns of unknown spend)", r.unknown_turns)
    } else {
        "".to_string()
    };

    let detail_2: gpui::AnyElement = if flavor_str.is_empty() {
        div().font_family("Cascadia Code").child(format!("{} tokens{}", tokens_str, unknown_str)).into_any_element()
    } else {
        h_flex().gap_1()
            .child(flavor_str)
            .child("·")
            .child(div().font_family("Cascadia Code").child(format!("{} tokens{}", tokens_str, unknown_str)))
            .into_any_element()
    };

    let is_excited = r.state == hadron_lattice::QuarkState::Excited;
    let dot_color = theme::presence(r.state);

    let dot = div()
        .absolute()
        .bottom_0()
        .right_0()
        .size(px(10.0))
        .rounded_full()
        .bg(dot_color)
        .border_2()
        .border_color(theme::bg_elevated());

    let dot = if is_excited {
        dot.with_animation(
            "pulse",
            gpui::Animation::new(std::time::Duration::from_millis(1500)).repeat(),
            move |div, delta| {
                let v: f32 = 0.3 + (delta * std::f32::consts::PI * 2.0).sin() * 0.7;
                div.opacity(v.max(0.3_f32))
            },
        )
        .into_any_element()
    } else {
        dot.into_any_element()
    };

    h_flex()
        .id(SharedString::from(format!("quark-{}", r.id)))
        .items_start()
        .gap_2p5()
        .px_2()
        .py_1p5()
        .rounded_md()
        .hover(|s| s.bg(theme::bg_surface()))
        .child(
            div()
                .relative()
                .mt_1()
                .child(identity_avatar(id, 28.0))
                .child(dot),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
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
                        .text_color(theme::text())
                        .opacity(0.8)
                        .truncate()
                        .child(detail_1),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text())
                        .opacity(0.7)
                        .truncate()
                        .child(detail_2),
                ),
        )
        // Effective permission mode (click to cycle a per-quark override).
        .child(mode_el)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
}

/// A labeled row in the Settings card: a muted caption above its control.
fn settings_field(label: &'static str, content: gpui::AnyElement) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(div().text_xs().text_color(theme::text_muted()).child(label))
        .child(content)
}

/// A small, subtle text button for secondary actions (caller attaches on_click).
fn text_button(
    id: impl Into<gpui::SharedString>,
    label: impl Into<gpui::SharedString>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px_2()
        .py_1()
        .rounded_md()
        .text_sm()
        .text_color(theme::text_secondary())
        .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
        .child(label.into())
}

/// The next mode in the ladder, cycling Ask → Write → Auto → Bypass → Ask.
fn next_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Ask => Mode::Write,
        Mode::Write => Mode::Auto,
        Mode::Auto => Mode::Bypass,
        Mode::Bypass => Mode::Ask,
    }
}

/// A permission-mode badge. Variant carries the risk temperature (Ask muted →
/// Bypass danger). A per-quark override renders solid; an inherited/global mode
/// renders outlined.
fn mode_tag(mode: Mode, is_override: bool) -> gpui::AnyElement {
    if !is_override {
        return div().into_any_element();
    }
    let (tag, label): (Tag, &'static str) = match mode {
        Mode::Ask => (Tag::secondary(), "ASK"),
        Mode::Write => (Tag::info(), "WRITE"),
        Mode::Auto => (Tag::warning(), "AUTO"),
        Mode::Bypass => (Tag::danger(), "BYPASS"),
    };
    tag.small()
        .outline()
        .child(div().child(label.to_string()))
        .into_any_element()
}

/// An overall swarm-status badge for the status bar. Priority: a blocked/error
/// quark, then a pending permission, then any active quark, else "ready".
fn swarm_status_tag(view: &ChamberView) -> impl IntoElement {
    let (tag, label): (Tag, &'static str) = if view
        .roster
        .iter()
        .any(|r| matches!(r.state, QuarkState::Error | QuarkState::Blocked))
    {
        (Tag::danger(), "error")
    } else if view.pending_permission.is_some() {
        (Tag::warning(), "waiting")
    } else if view
        .roster
        .iter()
        .any(|r| matches!(r.state, QuarkState::Excited | QuarkState::Thinking))
    {
        (Tag::info(), "working")
    } else {
        (Tag::success(), "ready")
    };
    tag.small()
        .outline()
        .child(div().font_family("Inter").child(label))
}

/// A muted placeholder line shown when a tab view has nothing to render.
fn empty_hint(text: &'static str) -> impl IntoElement {
    div().text_sm().text_color(theme::text_muted()).child(text)
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

fn markdown_style() -> gpui_component::text::TextViewStyle {
    let mut style = gpui_component::text::TextViewStyle::default();
    style.highlight_theme = gpui_component::highlighter::HighlightTheme::default_dark();
    style.table = {
        let mut s = gpui::StyleRefinement::default();
        s.overflow.x = Some(gpui::Overflow::Scroll);
        s
    };
    style
}

/// Mentions render as coloured, bold text: pink for a quark, purple for a file.
///
/// `<span style="color: …">` is honoured by `TextMark::color`, which exists only in
/// our fork of `gpui-component` (see the `[patch]` in the workspace `Cargo.toml`).
/// Upstream's `TextMark` can express a highlight — a *background* — but no foreground
/// colour, so before the fork a mention could only ever be a tinted block.
const MENTION_QUARK_OPEN: &str = "<span style=\"color: pink-400\"><strong>";
const MENTION_FILE_OPEN: &str = "<span style=\"color: purple-400\"><strong>";
const MENTION_CLOSE: &str = "</strong></span>";

/// Routing names that address the swarm rather than one quark. They are mentions of
/// *us*, so they read as quarks even though no roster row carries the id.
const SWARM_MENTIONS: [&str; 2] = ["team", "orchestrator"];

fn color_mentions(body: &str, roster: &[crate::model::RosterRow]) -> String {
    let mut out = String::with_capacity(body.len() + 100);
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' {
            let mut name = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '.' || nc == '/' || nc == '-' || nc == '_' || nc == ' ' || nc == '(' || nc == ')' {
                    name.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            
            // Because we greedily consumed spaces and parens, the name might have trailing characters
            // that aren't actually part of a mention (e.g., "@Agy said hi"). 
            // We need to find the longest matching display name or id.
            let mut matched_quark = false;
            let mut best_match_len = 0;
            
            for q in roster {
                let q_id = &q.id;
                let q_name = q.display_name.as_ref().unwrap_or(q_id);
                if name.starts_with(q_name) && q_name.len() > best_match_len {
                    best_match_len = q_name.len();
                    matched_quark = true;
                } else if name.starts_with(q_id) && q_id.len() > best_match_len {
                    best_match_len = q_id.len();
                    matched_quark = true;
                }
            }
            
            let is_swarm = SWARM_MENTIONS.iter().find(|&&m| name.starts_with(m));
            if let Some(m) = is_swarm {
                if m.len() > best_match_len {
                    best_match_len = m.len();
                    matched_quark = true;
                }
            }
            
            if matched_quark {
                let matched_name = name[..best_match_len].to_string();
                let remainder = name[best_match_len..].to_string();
                out.push_str(&format!("{}@{}{}", MENTION_QUARK_OPEN, matched_name, MENTION_CLOSE));
                out.push_str(&remainder);
            } else {
                // If it's a file, we shouldn't have consumed spaces/parens. 
                // We fallback to the old behavior for files: break at first space/paren.
                let mut file_name = String::new();
                for c in name.chars() {
                    if c.is_alphanumeric() || c == '.' || c == '/' || c == '-' || c == '_' {
                        file_name.push(c);
                    } else {
                        break;
                    }
                }
                if file_name.is_empty() {
                    out.push('@');
                    out.push_str(&name);
                } else {
                    let remainder = name[file_name.len()..].to_string();
                    out.push_str(&format!("{}@{}{}", MENTION_FILE_OPEN, file_name, MENTION_CLOSE));
                    out.push_str(&remainder);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Render a message body as Markdown under an element id unique to `(view, ix)`.
///
/// The id is load-bearing, not decoration. `gpui_component::text::markdown()`
/// derives its `ElementId` from `Location::caller()`, so every row rendered from
/// one call site would share a single id — and the `TextView`'s parsed state is
/// keyed on that id. All messages would then share one state, whose `set_text`
/// would see different text on every message and re-parse (and re-highlight) the
/// Markdown for every row, every frame. Distinct ids give each row its own state,
/// so `set_text` early-returns and the parse happens once per body.
///
/// Keying on the positional `ix` is sound only because the field is append-only and
/// rendered oldest-first, so a given message keeps its index for the window's life.
/// If rows ever get reordered or filtered, key on a stable message id instead — the
/// cache would silently stop helping, and no test would catch the regression.
impl Chamber {
    fn markdown_body(
        &self,
        view: &'static str,
        ix: usize,
        body: &str,
        roster: &[crate::model::RosterRow],
    ) -> impl IntoElement {
        let mut cache = self.parsed_markdown.borrow_mut();
        let html = cache
            .entry(ix)
            .or_insert_with(|| {
                let options = markdown::Options {
                    compile: markdown::CompileOptions {
                        allow_dangerous_html: true,
                        ..markdown::CompileOptions::default()
                    },
                    parse: markdown::ParseOptions::gfm(),
                };
                markdown::to_html_with_options(&color_mentions(body, roster), &options)
                    .unwrap_or_default()
            })
            .clone();

        div().text_size(px(13.65)).child(
            gpui_component::text::TextView::html((view, ix), html)
                .selectable(true)
                .style(markdown_style()),
        )
    }

    fn chat_message_row(
        &self,
        id: &ResolvedIdentity,
        m: &MessageRow,
        ix: usize,
        roster: &[crate::model::RosterRow],
    ) -> impl IntoElement {
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
                            .child(div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(id.color).child(id.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(crate::model::format_clock(m.ts.with_timezone(&chrono::Local))),
                            )
                            .when_some(m.to.clone(), |this, to| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(format!("→ {to}")),
                                )
                            })
                            .when_some(m.usage.as_ref(), |this, u| {
                                let mut parts = Vec::new();
                                if let Some(ctx) = &u.context {
                                    parts.push(format!("ctx: {:.1}%", ctx.used_percentage));
                                }
                                if !u.spend.is_empty() {
                                    let fresh = u.spend.fresh().unwrap_or(0);
                                    let cached = u.spend.cached().unwrap_or(0);
                                    let cost_str = if let Some(c) = u.cost_usd() { format!(" (${:.2})", c) } else { "".to_string() };
                                    if cached > 0 {
                                        parts.push(format!(
                                            "spent: {} fresh, {} cached{}",
                                            fresh, cached, cost_str
                                        ));
                                    } else {
                                        parts.push(format!("spent: {} fresh{}", fresh, cost_str));
                                    }
                                }
                                if parts.is_empty() {
                                    this
                                } else {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(format!("({})", parts.join(" | "))),
                                    )
                                }
                            }),
                    )
                    .child(self.markdown_body("chat-md", ix, &m.body, roster)),
            )
    }

    fn message_row(
        &self,
        m: &MessageRow,
        ix: usize,
        roster: &[crate::model::RosterRow],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_expanded = self.log_expanded_ixs.contains(&ix);
        
        let mut header_row = h_flex()
            .gap_2()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _window, _cx| {
                if this.log_expanded_ixs.contains(&ix) {
                    this.log_expanded_ixs.remove(&ix);
                } else {
                    this.log_expanded_ixs.insert(ix);
                }
            }))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::actor_hue(&m.from))
                            .child(if is_expanded { format!("▼ {}", m.from) } else { format!("▶ {}", m.from) }),
                    )
                    .when_some(m.to.clone(), |this, to| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(format!("→ {}", to)),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(crate::model::format_clock(m.ts.with_timezone(&chrono::Local))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(format!("· {}", m.kind_label)),
                    ),
            );
            
        if let Some(u) = m.usage.as_ref() {
            let mut parts = Vec::new();
            if let Some(ctx) = &u.context {
                parts.push(format!("ctx: {:.1}%", ctx.used_percentage));
            }
            if !u.spend.is_empty() {
                let fresh = u.spend.fresh().unwrap_or(0);
                let cached = u.spend.cached().unwrap_or(0);
                let cost_str = if let Some(c) = u.cost_usd() { format!(" (${:.2})", c) } else { "".to_string() };
                if cached > 0 {
                    parts.push(format!("spent: {} fresh, {} cached{}", fresh, cached, cost_str));
                } else {
                    parts.push(format!("spent: {} fresh{}", fresh, cost_str));
                }
            }
            if !parts.is_empty() {
                header_row = header_row.child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(format!("({})", parts.join(" | "))),
                );
            }
        }
        
        let mut row = v_flex().gap_1().child(header_row);
        
        if is_expanded {
            row = row.child(self.markdown_body("log-md", ix, &m.body, roster));
        } else {
            let snippet = m.body.lines().next().unwrap_or("").chars().take(80).collect::<String>();
            let suffix = if m.body.len() > snippet.len() { "..." } else { "" };
            row = row.child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("{}{}", snippet, suffix))
            );
        }
        
        row
    }
}

/// Launch the chamber window against a field file path.
pub fn run(field_path: Option<String>) {
    let Some(path) = field_path else {
        eprintln!("usage: hadron-chamber <field.jsonl>");
        return;
    };
    let field_path = PathBuf::from(&path);
    // Load the SAME team the daemon seated for this field: the project
    // `.hadron/team.json` beside the field, else the global `~/.hadron/team.json`.
    let team = hadron_lattice::team_for_field(&field_path)
        .map(|p| load_team(&p))
        .unwrap_or_default();
    let events = io::read_events(&field_path).unwrap_or_default();
    let view = model::project_with_team(&events, &team);
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
            t.popover = theme::popover().into();
            // Borderless: the resize handle paints `border` when idle, so match
            // it to the sidebar and it disappears into the unified space; while
            // dragging it paints `drag_border` — keep that on-brand pink so the
            // drag still shows feedback. (This also softens gpui-component's own
            // hairlines, which suits the borderless surfaces.)
            t.border = rgb(0x191a1b).into();
            t.drag_border = rgb(0xec4899).into();
            // Markdown inline code blocks use `accent` for background in gpui-component.
            // Override it to a very soft white overlay so it's slightly brighter than the background.
            t.accent = gpui::rgba(0xffffff20).into();
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
            t.font_family =
                "Inter, Segoe UI, sans-serif, Noto Color Emoji, Apple Color Emoji, Segoe UI Emoji".into();
        }
        cx.bind_keys([
            KeyBinding::new("ctrl-shift-p", TogglePalette, Some(KEY_CONTEXT)),
            KeyBinding::new("cmd-shift-p", TogglePalette, Some(KEY_CONTEXT)),
            KeyBinding::new("shift-tab", CycleMode, Some(KEY_CONTEXT)),
        ]);

        // Build window options here (needs `&App`, not the async cx below).
        //
        // Size and origin are restored *independently*, which matters: the first
        // cut treated them as one unit, so a saved origin the compositor wouldn't
        // vouch for threw the saved size away too and the window reopened at the
        // default. Under Wayland (WSLg) that fired every launch — a client there
        // can't position itself, and `displays()` may enumerate nothing — so the
        // origin check must not be able to veto the size.
        let default_size = gpui::size(px(1440.0), px(900.0));
        let mut bounds = WindowBounds::centered(default_size, cx);
        if let Some(wb) = &prefs.window_bounds {
            let size = gpui::size(px(wb.width), px(wb.height));
            let origin = gpui::point(px(wb.x), px(wb.y));

            // Keep the saved size regardless — centered is the honest fallback for
            // a position we can't place.
            bounds = WindowBounds::centered(size, cx);

            // Only place the window at its saved origin if a display actually
            // covers it, so an unplugged monitor can't strand it offscreen.
            let displays = cx.displays();
            let on_screen = displays.iter().any(|d| d.bounds().contains(&origin));
            if on_screen {
                bounds = WindowBounds::Windowed(gpui::Bounds { origin, size });
            }
        }

        let window_options = WindowOptions {
            titlebar: Some(TitleBar::title_bar_options()),
            window_decorations: Some(WindowDecorations::Client),
            // Transparent so the client-side shadow + rounded frame composite
            // over the desktop (Zed's approach). On WSLg this is the open
            // question — if the compositor ignores it, the inset shows black.
            window_background: WindowBackgroundAppearance::Transparent,
            window_bounds: Some(bounds),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, move |window, cx| {
                let chamber = cx.new(|cx| {
                    Chamber::new(
                        view.clone(),
                        prefs.clone(),
                        team.clone(),
                        field_path.clone(),
                        window,
                        cx,
                    )
                });
                cx.new(|cx| Root::new(chamber, window, cx).bordered(false))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RosterRow;
    use hadron_lattice::QuarkState;

    #[test]
    fn test_mentions_parse_as_raw_html() {
        let body = "Hello @opus!";
        let roster = vec![RosterRow {
            id: "opus".to_string(),
            state: QuarkState::Excited,
            mode: hadron_lattice::Mode::Ask,
            mode_is_override: false,
            provider: "anthropic".to_string(),
            model: "Claude Opus 4.6".to_string(),
            flavor: Some(hadron_lattice::Flavor::Worker),
            transport: hadron_lattice::Transport::Cli,
            enabled: true,
            tokens: 0,
            unknown_turns: 0,
        }];

        let colored = color_mentions(body, &roster);
        assert_eq!(
            colored,
            "Hello <span style=\"color: pink-400\"><strong>@opus</strong></span>!"
        );

        let options = markdown::Options {
            compile: markdown::CompileOptions {
                allow_dangerous_html: true,
                ..markdown::CompileOptions::default()
            },
            parse: markdown::ParseOptions::gfm(),
        };
        let html = markdown::to_html_with_options(&colored, &options).unwrap();
        // The HTML should literally contain the span, meaning it wasn't escaped
        assert!(html.contains("<span style=\"color: pink-400\"><strong>@opus</strong></span>"));
    }

    /// `@team` and `@orchestrator` route to the swarm rather than to a roster row, so
    /// nothing in the roster carries those ids — they must still read as quarks, not
    /// as filenames.
    #[test]
    fn swarm_mentions_colour_as_quarks_not_files() {
        let colored = color_mentions("@team and @orchestrator and @src/main.rs", &[]);
        assert!(colored.contains(&format!("{MENTION_QUARK_OPEN}@team")));
        assert!(colored.contains(&format!("{MENTION_QUARK_OPEN}@orchestrator")));
        assert!(colored.contains(&format!("{MENTION_FILE_OPEN}@src/main.rs")));
    }

    /// An unparseable colour is not an error in gpui-component — it is silently
    /// dropped. And `TextMark::color` exists only in our fork (see the `[patch]` in
    /// the workspace Cargo.toml): if that patch ever stops applying, we fall back to
    /// upstream, mentions render as plain text, and nothing else would notice.
    #[test]
    fn mention_colours_are_ones_the_forked_renderer_can_apply() {
        for markup in [MENTION_QUARK_OPEN, MENTION_FILE_OPEN] {
            let color = markup
                .split_once("color: ")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(c, _)| c)
                .expect("mention markup carries a color declaration");
            let parsed = gpui_component::try_parse_color(color)
                .unwrap_or_else(|e| panic!("{color} is not a colour gpui-component accepts: {e}"));

            // Fails to compile against upstream gpui-component, which has no `color`.
            let mark = gpui_component::text::TextMark::default().color(parsed);
            assert_eq!(mark.color, Some(parsed));
        }
    }
}
