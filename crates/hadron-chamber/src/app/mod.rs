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
    actions, div, linear_color_stop, linear_gradient, prelude::*, px, rgb, rgba, App, Context,
    Decorations, Entity,
    FocusHandle, Hsla, KeyBinding, MouseButton, Pixels, Render, Rgba, ScrollHandle, SharedString,
    Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowControlArea,
    WindowDecorations, WindowOptions,
};
use gpui_component::avatar::Avatar;
// badge removed
use gpui_component::chart::AreaChart;
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
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
use hadron_lattice::{
    io, load_team, resolve_team, Actor, Event, Kind, Mode, QuarkId, QuarkState, SeatOverride, Team,
};

use crate::config::{self, ChamberPrefs, Identity};
use crate::model::{self, ChamberView, MessageRow, RosterRow, StatsWindow};
use crate::theme;

mod mentions;
use mentions::{color_mentions, parse_plan_progress};

mod identity;
use identity::{
    hsla_to_hex, identity_avatar, pack_rgb, parse_hex, ResolvedIdentity, IDENTITY_SWATCHES,
};

mod tabs;
use tabs::{ChatTab, InfoTab, Rail, RightRailTab};

mod providers;
use providers::{
    configured_providers, migrate_repo_to_catalogue, AgentDescriptor, ConfiguredQuark,
    ProviderState, SettingsTarget, WizardState,
};

mod widgets;
use widgets::{
    control_button, drag_region, effort_tag, empty_hint, fallback_pick_image, format_num,
    frame_corner_radii, glow_layer, kind_icon, kv_row, log_row, markdown_style, menu_button,
    mode_color, mode_hint, mode_label, mode_tag, next_mode, panel_eyebrow, progress_meter,
    roster_row, session_card, settings_field, stat_tile, text_button, wash_layer,
};

mod actions;
mod settings;

actions!(
    chamber,
    [
        CycleMode,
        NextChatTab,
        PrevChatTab,
        NextInspectorTab,
        PrevInspectorTab,
        NextStatsSubTab,
        PrevStatsSubTab,
        NextQuark,
        PrevQuark,
        ToggleSelectedQuark,
        OpenMenu,
    ]
);

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

/// Terminal grid metrics: the render font size and one cell's width/height for
/// Cascadia Code at that size. The pump loop divides the measured screen by
/// these to pick the PTY's columns/rows, so they must track the values used in
/// the grid render. (Cascadia's advance is ~0.6em; line height ~1.3em.)
const TERM_FONT: f32 = 13.0;
const TERM_CELL_W: f32 = 7.8;
const TERM_CELL_H: f32 = 17.0;

/// The live completion card for the chat box: the rows it offers and which one
/// Enter accepts. Held as `Option<CompletionCard>` on the chamber — `None` is
/// "no card", so the open-flag and the rows can never disagree (there is no
/// separate bool to fall out of sync). Rebuilt on every `InputEvent::Change`.
struct CompletionCard {
    /// Byte offset of the trigger char in the input; the accept replaces
    /// `input[start..cursor]` with the chosen row's `new_text`.
    start: usize,
    candidates: Vec<crate::text::Candidate>,
    /// The highlighted row (Up/Down move it, Enter/click accept it).
    selected: usize,
}

struct Chamber {
    view: ChamberView,
    prefs: ChamberPrefs,
    /// The **repo** team file (`.hadron/team.json`): legacy full seats plus per-repo
    /// role/state overrides. This is what the chamber edits and writes. The roster is
    /// projected from `resolve_team(&self.team, &self.global)`, not this directly.
    team: Team,
    /// The **global catalogue** (`~/.hadron/team.json`): every quark's definition. A
    /// repo override names one of these; catalogue quarks not adopted here show greyed.
    global: Team,
    /// The field file this chamber reads from and steers into.
    path: PathBuf,
    /// The human's message box at the foot of the chat column.
    input: Entity<InputState>,
    /// The completion card floating above the message box, or `None` when no
    /// `@`/`:`/`/` query is live. Our own overlay, not the fork's LSP menu.
    completion: Option<CompletionCard>,
    /// Root focus target, so Ctrl+Shift+P dispatches regardless of what's focused.
    focus_handle: FocusHandle,
    /// Which view the chat column's segmented tabs are showing.
    chat_tab: ChatTab,
    /// Which section the quark info panel is showing.
    info_tab: InfoTab,
    /// The time window the Stats views (chat Stats tab + info Stats tab) aggregate over.
    /// Shared: the info panel is a modal overlay, never on-screen with the chat Stats tab.
    stats_window: StatsWindow,
    /// Projected messages of every archived session (`sessions/*/field.jsonl`), the
    /// history the wider [`StatsWindow`]s fold in. Loaded once at startup and rebuilt by
    /// the `/clear` handler (the only thing that writes a new archive in this process).
    archived_messages: Vec<MessageRow>,
    /// Which view the right rail's segmented tabs are showing. The right rail is
    /// independent of the chat column: changing the chat tab must not move it.
    right_rail_tab: RightRailTab,
    /// Keyboard cursor over the roster (index into `view.roster`), moved by the
    /// quark-nav keys and drawn as a highlighted row. `None` = nothing selected.
    selected_quark_ix: Option<usize>,
    /// Whether the keyboard-triggered app menu overlay is open (mirrors the click
    /// dropdown behind the hamburger button, but reachable without the mouse).
    app_menu_open: bool,
    /// Cached diff string for the Changes rail
    working_diff: Option<Vec<crate::vcs::FileDiff>>,
    changes_open_ixs: std::collections::HashSet<usize>,
    changes_scroll: ScrollHandle,
    /// Scroll position of the Plan tracker pane.
    plan_scroll: ScrollHandle,
    /// Virtual list state for the Chat tab.
    chat_list_state: gpui::ListState,
    log_list_state: gpui::ListState,
    /// Log rows (by message index) the user has clicked to expand to their full body.
    log_expanded: std::collections::HashSet<usize>,
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
    /// Whether the Settings overlay is showing, and which identity it edits.
    settings_open: bool,
    settings_target: SettingsTarget,
    /// Settings editor fields (display name + image path for the current target).
    settings_name: Entity<InputState>,
    settings_path: Entity<InputState>,
    settings_model: Entity<InputState>,
    settings_effort: Entity<InputState>,
    settings_mode_config: Entity<InputState>,
    /// Live filter for the add-quark preset catalogue (~37 entries): a case-insensitive
    /// substring match on preset name + command, so the list is searchable instead of a
    /// long scroll.
    preset_filter: Entity<InputState>,
    /// Arbitrary-colour picker for the current Settings identity, beside the preset
    /// swatches. Its `Change` events write the identity's colour (see `new`).
    color_picker: Entity<ColorPickerState>,
    /// A path chosen from the native file picker (avatar image), parked here by the
    /// async picker task and drained into `settings_path` at the next `render` — the
    /// picker returns without a `Window`, but `set_value` needs one, so `render`
    /// (which has the window) applies it.
    pending_image_pick: Option<String>,
    /// Keep the input subscriptions alive for the window's lifetime. The last
    /// two repaint the Settings overlay so its live preview tracks typing.
    _input_sub: Subscription,
    _settings_subs: [Subscription; 4],
    providers: Vec<ConfiguredQuark>,
    wizard_state: WizardState,
    /// Every workspace entry with its ignored flag; drives the file tree. Gitignored
    /// entries are flagged `true` (rendered muted) and wholly-ignored dirs are collapsed.
    file_tree_paths: Vec<(String, bool)>,
    completion_files: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    file_tree_open: Option<(String, String)>,
    file_tree_expanded: std::collections::HashSet<String>,
    terminal: Option<crate::pty::PtyTerminal>,
    /// Keyboard focus for the terminal grid — keystrokes flow to the PTY only
    /// while this holds focus.
    terminal_focus: FocusHandle,
    /// The terminal screen's measured pixel size, written by a paint-time canvas
    /// probe and read by the pump loop to size the PTY to fit.
    terminal_px: std::rc::Rc<std::cell::Cell<Option<(f32, f32)>>>,
    info_panel: Option<String>,
    /// The About dialog, opened from the app menu.
    about_open: bool,
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
    /// Adopt a catalogue quark into this repo (available → participating).
    AdoptQuark(String),
    /// Force-restart a resident quark's session (reap the subprocess, keep it seated).
    /// Offered only for `Transport::Acp` quarks — a one-shot CLI quark holds nothing
    /// resident to kill.
    RestartQuark(String),
}

impl Chamber {
    fn new(
        view: ChamberView,
        prefs: ChamberPrefs,
        team: Team,
        global: Team,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let repo_root = crate::vcs::repo_root_of(&path);
        let files = crate::sys::list_workspace_files(&repo_root);
        // `@`-mention autocomplete only offers real, editable files — never the muted
        // gitignored entries (collapsed build dirs etc.), so filter them out here.
        let completion_files = std::rc::Rc::new(std::cell::RefCell::new(
            files
                .iter()
                .filter(|(_, ignored)| !ignored)
                .map(|(p, _)| p.clone())
                .collect::<Vec<String>>(),
        ));

        // No `completion_provider`: the fork's LSP menu is drawn with `deferred()`
        // and paints off the bottom of the window (seven fixes could not move it —
        // see `completion-menu-draws-out-of-bounds`). The chat completions are our
        // own card instead (`completion_card_overlay`), driven from `InputEvent`.
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 4)
                .submit_on_enter(true)
                .placeholder("Type @quark a message…  (Enter to send · Shift+Enter for newline)")
        });
        let _input_sub = cx.subscribe_in(&input, window, Self::on_input_submit);

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        let settings_name = cx.new(|cx| InputState::new(window, cx).placeholder("Display name"));
        let settings_path = cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/image.png"));
        let settings_model = cx.new(|cx| InputState::new(window, cx).placeholder("inherit catalogue default"));
        let settings_effort = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. low, standard, high"));
        let settings_mode_config = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. architect, code, ask"));
        let preset_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search providers…"));
        let color_picker = cx.new(|cx| ColorPickerState::new(window, cx));
        // Repaint the Settings overlay on every edit so its preview is live.
        let _settings_subs = [
            cx.subscribe_in(&settings_name, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
            cx.subscribe_in(&settings_path, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
            // Re-render the add-quark wizard on every keystroke so the preset list
            // re-filters live as the search box is typed into.
            cx.subscribe_in(&preset_filter, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
            // A colour chosen in the picker writes the current Settings identity's colour.
            cx.subscribe_in(
                &color_picker,
                window,
                |this, _, event: &ColorPickerEvent, _, cx| {
                    let ColorPickerEvent::Change(Some(hsla)) = event else {
                        return;
                    };
                    this.set_settings_color(hsla_to_hex(*hsla), cx);
                },
            ),
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

        // Terminal pump: while the Terminal tab holds a live PTY, size it to the
        // measured screen and repaint when new output arrives. Capped at ~10fps: a
        // repaint here re-renders the WHOLE window in software (llvmpipe, no GPU), so a
        // live TUI that emits every frame (an agent CLI's spinner/token counter) drove
        // a full-window raster ~30x/sec. A text terminal reads fine at 10fps, and
        // keystrokes still echo immediately (on_terminal_key notifies directly). Does
        // nothing (forces no frame) when the terminal is closed or idle.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            if this
                .update(cx, |chamber, cx| chamber.pump_terminal(cx))
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
        // The Configured Providers list is every ADOPTED quark (resolved), so a
        // migrated repo whose seats are now overrides still lists them.
        let providers = configured_providers(&resolve_team(&team, &global));

        // Load the archived sessions once — the history the wider Stats windows fold in.
        // Rebuilt only by `/clear` (the sole writer of a new archive in this process).
        let archived_messages = path
            .parent()
            .map(|p| crate::model::load_archived_messages(&p.join("sessions")))
            .unwrap_or_default();

        Chamber {
            view,
            prefs,
            team,
            global,
            path,
            input,
            completion: None,
            focus_handle,
            chat_tab: ChatTab::Chat,
            info_tab: InfoTab::Identity,
            stats_window: StatsWindow::Session,
            archived_messages,
            right_rail_tab: RightRailTab::Terminal,
            selected_quark_ix: None,
            app_menu_open: false,
            working_diff: None,
            changes_open_ixs: std::collections::HashSet::new(),
            changes_scroll: ScrollHandle::new(),
            plan_scroll: ScrollHandle::new(),
            chat_list_state,
            log_list_state,
            log_expanded: std::collections::HashSet::new(),
            log_expanded_ixs: std::collections::HashSet::new(),
            chat_message_ixs,
            chat_scrolls,
            parsed_markdown: std::cell::RefCell::new(std::collections::HashMap::new()),
            bounds_save_pending: false,
            settings_open: false,
            settings_target: SettingsTarget::Human,
            settings_name,
            settings_path,
            settings_model,
            settings_effort,
            settings_mode_config,
            preset_filter,
            color_picker,
            pending_image_pick: None,
            _input_sub,
            _settings_subs,
            providers,
            wizard_state: WizardState::None,
            file_tree_paths: files,
            completion_files,
            file_tree_open: None,
            file_tree_expanded: std::collections::HashSet::new(),
            terminal: None,
            terminal_focus: cx.focus_handle(),
            terminal_px: std::rc::Rc::new(std::cell::Cell::new(None)),
            info_panel: None,
            about_open: false,
            file_tree_scroll: ScrollHandle::new(),
            file_tree_open_scroll: ScrollHandle::new(),
        }
    }

    /// Re-project the field into the roster/log/session view, resolving the repo team
    /// against the global catalogue first so adopted quarks carry their full defs and
    /// available-but-not-adopted catalogue quarks show greyed. The one place the view
    /// is rebuilt, so every mutation path routes through the same resolve.
    fn reproject(&mut self, events: &[Event]) {
        let resolved = resolve_team(&self.team, &self.global);
        self.view = model::project_with_team(events, &resolved, &self.global);
    }

    /// Drive the live terminal each tick: spawn the PTY lazily when the Terminal
    /// tab is open, size it to the measured screen, and repaint only when the
    /// child has produced new output (an idle terminal forces no frames).
    fn pump_terminal(&mut self, cx: &mut Context<Self>) {
        if self.right_rail_tab != RightRailTab::Terminal || self.prefs.inspector_collapsed {
            return;
        }
        // Translate the last painted screen size into columns/rows (default until
        // the first frame has measured it).
        let (cols, rows) = match self.terminal_px.get() {
            Some((w, h)) => (
                ((w / TERM_CELL_W).floor() as usize).max(2),
                ((h / TERM_CELL_H).floor() as usize).max(2),
            ),
            None => (80, 24),
        };
        if self.terminal.is_none() {
            let root = crate::vcs::repo_root_of(&self.path).to_path_buf();
            if let Ok(term) = crate::pty::PtyTerminal::new(&root, cols, rows) {
                self.terminal = Some(term);
                cx.notify();
            }
            return;
        }
        if let Some(term) = &mut self.terminal {
            term.resize(cols, rows);
            if term.take_dirty() {
                cx.notify();
            }
        }
    }

    /// Translate a keystroke into the bytes a TTY expects and stream them to the
    /// child. Covers the printable range, the essential control keys, Ctrl+letter
    /// control codes, and the arrow/nav escape sequences. (Function keys, mouse
    /// reporting, and the kitty keyboard protocol are not wired yet.)
    fn on_terminal_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(term) = &mut self.terminal else {
            return;
        };
        let ks = &event.keystroke;
        let m = &ks.modifiers;
        let bytes: Vec<u8> = match ks.key.as_str() {
            "enter" => vec![b'\r'],
            "backspace" => vec![0x7f],
            "tab" => vec![b'\t'],
            "escape" => vec![0x1b],
            "space" => vec![b' '],
            "up" => vec![0x1b, b'[', b'A'],
            "down" => vec![0x1b, b'[', b'B'],
            "right" => vec![0x1b, b'[', b'C'],
            "left" => vec![0x1b, b'[', b'D'],
            "home" => vec![0x1b, b'[', b'H'],
            "end" => vec![0x1b, b'[', b'F'],
            "delete" => vec![0x1b, b'[', b'3', b'~'],
            _ => {
                if m.control && ks.key.len() == 1 {
                    // Ctrl+letter → its control byte (Ctrl-C = 0x03, Ctrl-D = 0x04…).
                    let c = ks.key.as_bytes()[0].to_ascii_lowercase();
                    if c.is_ascii_lowercase() {
                        vec![c - b'a' + 1]
                    } else {
                        return;
                    }
                } else if !m.control && !m.alt {
                    // A printable key: prefer the platform-resolved character
                    // (handles Shift and dead keys), else the single-char key.
                    if let Some(ch) = &ks.key_char {
                        ch.clone().into_bytes()
                    } else if ks.key.chars().count() == 1 {
                        ks.key.clone().into_bytes()
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        };
        term.send_input(&bytes);
        cx.notify();
    }



    fn info_panel_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let qid = self.info_panel.as_ref().unwrap().clone();
        let roster_row = self.view.roster.iter().find(|r| r.id == qid).unwrap();
        let q_color = self.color_for(&qid);
        let resolved = self.resolve_identity(&qid);

        let stats =
            self.view
                .stats_for(&self.archived_messages, self.stats_window, chrono::Utc::now());
        let q_stats = stats
            .per_quark
            .into_iter()
            .find(|(id, _)| id == &qid)
            .map(|(_, s)| s)
            .unwrap_or_default();

        // Effort + session mode live on the resolved seat, not the roster row.
        let seat = resolve_team(&self.team, &self.global)
            .quarks
            .into_iter()
            .find(|s| s.id.as_str() == qid);
        let effort = seat.as_ref().and_then(|s| s.effort.clone());

        let flavor_str = match &roster_row.flavor {
            Some(hadron_lattice::Flavor::Orchestrator) => "Orchestrator",
            Some(hadron_lattice::Flavor::Worker) => "Worker",
            None => "—",
        };
        // For ACP the "Agent" is the boot command the daemon runs (genuinely more info
        // than repeating the provider); an absent command means "resolve the default from
        // the provider". CLI seats are driven by the in-process adapter.
        let agent_str = match roster_row.transport {
            hadron_lattice::Transport::Acp => seat
                .as_ref()
                .and_then(|s| s.command.as_ref())
                .map(|c| {
                    if c.args.is_empty() {
                        c.program.clone()
                    } else {
                        format!("{} {}", c.program, c.args.join(" "))
                    }
                })
                .unwrap_or_else(|| format!("default ({})", roster_row.provider)),
            hadron_lattice::Transport::Cli => "hadron-adapter".to_string(),
        };
        let model_str = if roster_row.model.is_empty() {
            "—".to_string()
        } else {
            roster_row.model.clone()
        };
        let transport_str = match roster_row.transport {
            hadron_lattice::Transport::Cli => "CLI (one-shot)",
            hadron_lattice::Transport::Acp => "ACP (resident)",
        };

        // Presence: a live (adopted + enabled) quark shows its state colour; otherwise
        // it is greyed, distinguishing "available here but not adopted" from "disabled".
        let live = roster_row.adopted && roster_row.enabled;
        let (dot_color, presence_txt) = if live {
            (
                theme::presence(roster_row.state),
                theme::presence_label(roster_row.state).to_string(),
            )
        } else if !roster_row.adopted {
            (theme::presence_disabled(), "available — not adopted here".to_string())
        } else {
            (theme::presence_disabled(), "disabled".to_string())
        };

        // Header: avatar + display name + a live presence line.
        let header = h_flex()
            .gap_3()
            .items_center()
            .child(identity_avatar(&resolved, 46.0))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(q_color)
                            .child(resolved.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(div().size(px(8.0)).rounded_full().bg(dot_color))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(presence_txt),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(format!("· {qid}")),
                            ),
                    ),
            );

        // A coloured permission chip (always shown, unlike the roster's override-only tag).
        let pm = roster_row.mode;
        let perm_chip = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .bg(mode_color(pm).opacity(0.18))
                    .border_1()
                    .border_color(mode_color(pm).opacity(0.5))
                    .text_color(mode_color(pm))
                    .child(mode_label(pm)),
            )
            .child(div().text_xs().text_color(theme::text_muted()).child(
                if roster_row.mode_is_override { "override" } else { "global default" },
            ));

        // Force-restart action — only for a resident (ACP) seat, which is the only kind
        // that holds a live subprocess to reap; a one-shot CLI seat has nothing between
        // turns. Reaps the session (aborting any in-flight turn); it re-boots fresh on
        // its next mention. This is the human's manual override for a wedged agent. Lives
        // in the Identity tab (it acts on *this* quark, not on its wiring).
        let restart_action: Option<gpui::AnyElement> =
            matches!(roster_row.transport, hadron_lattice::Transport::Acp).then(|| {
                let rid = qid.clone();
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_4()
                    .items_center()
                    .text_sm()
                    .child(div().flex_none().text_color(theme::text_muted()).child("Session"))
                    .child(
                        h_flex()
                            .id("info-restart")
                            .cursor_pointer()
                            .items_center()
                            .gap_1p5()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .text_color(theme::text())
                            .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
                            .child("⟳")
                            .child("Restart agent")
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.reboot_quark(&rid, cx)),
                            ),
                    )
                    .into_any_element()
            });

        let identity_section = v_flex()
            .gap_1p5()
            .child(panel_eyebrow("IDENTITY"))
            .child(kv_row("Role", flavor_str))
            .child(kv_row(
                "State",
                if roster_row.enabled { "enabled" } else { "disabled" },
            ))
            .child(kv_row(
                "Adoption",
                if roster_row.adopted { "adopted in this repo" } else { "available (catalogue)" },
            ))
            // Restart lives here (Identity), acting on this quark; ACP-only, else None.
            .children(restart_action);

        let mut config_section = v_flex()
            .gap_1p5()
            .child(panel_eyebrow("CONFIGURATION"))
            .child(kv_row("Provider", roster_row.provider.clone()))
            .child(kv_row("Agent", agent_str))
            .child(kv_row("Model", model_str))
            .child(kv_row("Transport", transport_str));
        // Always shown, even when the seat inherits (unset) — an empty row read as a
        // missing feature ("I can't see the effort tag"); "inherited" says it explicitly.
        config_section = config_section.child(kv_row(
            "Effort",
            effort.clone().unwrap_or_else(|| "inherited".to_string()),
        ));
        // The Permission chip below is the single authority control (it replaced the
        // Claude-specific ACP `mode_config`), so `mode_config` is deliberately not shown
        // here — showing both would just relocate the duplication it was meant to remove.
        config_section = config_section.child(
            h_flex()
                .w_full()
                .justify_between()
                .gap_4()
                .items_center()
                .text_sm()
                .child(div().flex_none().text_color(theme::text_muted()).child("Permission"))
                .child(perm_chip),
        );

        // --- Session stats ---
        let avg = if q_stats.turns > 0 { q_stats.fresh / q_stats.turns } else { 0 };
        let first_seen_str = q_stats
            .first_seen
            .map(|ts| ts.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "never".to_string());
        let last_active_str = q_stats
            .last_active
            .map(|ts| ts.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "never".to_string());

        let mut stats_block = v_flex()
            .gap_1p5()
            .child(kv_row("Turns", q_stats.turns.to_string()))
            .child(kv_row(
                "Fresh spent",
                format!("{} ({}/turn)", format_num(q_stats.fresh), format_num(avg)),
            ))
            .child(kv_row("Cached", format_num(q_stats.cached)));
        // `unknown_turns` is a live-field aggregate, not windowed — only honest to show
        // it alongside the live Session numbers, so it is hidden in the archived windows
        // rather than displayed as if it were a Week/Month/All-time count.
        if roster_row.unknown_turns > 0 && self.stats_window == StatsWindow::Session {
            stats_block = stats_block
                .child(kv_row("Unmeasured", format!("+{} turns", roster_row.unknown_turns)));
        }
        stats_block = stats_block
            .child(kv_row("First seen", first_seen_str))
            .child(kv_row("Last active", last_active_str));

        if let Some(ctx) = q_stats.context.as_ref() {
            stats_block = stats_block.child(kv_row(
                "Context",
                format!(
                    "{:.1}% ({} / {})",
                    ctx.used_percentage,
                    format_num(ctx.used_tokens),
                    format_num(ctx.context_window_size)
                ),
            ));
            // Context occupancy is a proportion, not a series — a progress bar reads it
            // better than a two-bar chart. Fill in the quark's colour.
            let frac = (ctx.used_percentage as f32 / 100.0).clamp(0.0, 1.0);
            stats_block = stats_block.child(div().mt_1().child(progress_meter(frac, q_color)));
        }
        if !q_stats.spend_history.is_empty() {
            // Fresh-spend over turns as an area under the curve: the quark's hue stroke
            // over a vertical gradient of the same hue fading to transparent, so the
            // trend reads as a filled shape, not a thin line. `linear_gradient` angle 0
            // points up, so the strong stop sits at position 1.0 (top, at the curve) and
            // fades toward the baseline.
            stats_block = stats_block.child(
                div().h(px(96.0)).w_full().mt_1().child(
                    AreaChart::new(q_stats.spend_history.clone())
                        .id(format!("info-spend-chart-{qid}"))
                        .name("Fresh Spent")
                        .x(|d| format!("T{}", d.turn))
                        .y(|d| d.fresh as f64)
                        .stroke(q_color)
                        .fill(linear_gradient(
                            0.0,
                            linear_color_stop(q_color.opacity(0.35), 1.0),
                            linear_color_stop(q_color.opacity(0.02), 0.0),
                        ))
                        .natural(),
                ),
            );
        }
        for bucket in q_stats.quota {
            stats_block = stats_block.child(kv_row(
                "Quota",
                format!("{}: {:.0}% left", bucket.key, bucket.remaining_fraction * 100.0),
            ));
        }

        // Section tabs keep the panel short: the header stays pinned (you always see
        // whose panel this is), and one section shows at a time below it.
        let info_selected = self.info_tab;
        let info_tabs = TabBar::new("info-tabs")
            .segmented()
            .selected_index(info_selected.index())
            .children(InfoTab::ALL.map(|t| {
                if t.index() == info_selected.index() {
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
                this.info_tab = InfoTab::from_index(*ix);
                cx.notify();
            }));

        let body = match info_selected {
            InfoTab::Identity => identity_section.into_any_element(),
            InfoTab::Config => config_section.into_any_element(),
            InfoTab::Stats => v_flex()
                .gap_3()
                .child(self.stats_window_tabs("info-stats-window-tabs", cx))
                .child(stats_block)
                .into_any_element(),
        };

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.info_panel = None;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("quark-info-panel")
                    .occlude()
                    .w(px(560.0))
                    .max_h(px(660.0))
                    .overflow_y_scroll()
                    // Opaque: a focused info panel must not let the bright field bleed
                    // through (glass_surface read as too transparent). Solid, like Settings.
                    .bg(theme::modal_surface())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .rounded(INNER_RADIUS)
                    .p_5()
                    .gap_4()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {}) // swallow inner clicks
                    .child(header)
                    .child(info_tabs)
                    .child(body),
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
                self.reproject(&events);

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
            // Rescan files unconditionally so autocomplete mentions are always live,
            // regardless of which right rail tab is active.
            let root = crate::vcs::repo_root_of(&self.path);
            let files = crate::sys::list_workspace_files(root);
            if files != self.file_tree_paths {
                // Autocomplete offers only real, editable files — never muted gitignored
                // entries — mirroring the filter in `new`.
                *self.completion_files.borrow_mut() = files
                    .iter()
                    .filter(|(_, ignored)| !ignored)
                    .map(|(p, _)| p.clone())
                    .collect();
                self.file_tree_paths = files;
                changed = true;
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
        // Typing rebuilds the completion card from the live text; it is our own
        // overlay, not the fork's LSP menu, so we drive it from the edit stream.
        if let InputEvent::Change = event {
            self.recompute_completion(cx);
            cx.notify();
            return;
        }
        let InputEvent::PressEnter { shift, .. } = event else {
            return;
        };
        // A live card claims Enter: it accepts the highlighted row instead of
        // sending the message (Shift+Enter always means newline, never accept).
        if !*shift && self.completion.is_some() {
            self.accept_completion(window, cx);
            return;
        }
        if *shift {
            let selected_idx = self.chat_tab.index();
            let scroll = self.chat_scrolls[selected_idx].clone();
            cx.on_next_frame(window, move |_, _, cx: &mut Context<Self>| {
                scroll.scroll_to_bottom();
                cx.notify();
            });
            return;
        }
        let mut text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }

        // A single line may chain several UI commands and end with a normal message,
        // e.g. "/toggle-roster /clear ping the team". Zero-arg UI commands run in
        // order as they appear; `/team-brainstorm` consumes the rest of the line as
        // its argument; whatever text is left over falls through to be posted as one
        // human message via the normal path below.
        {
            const ZERO_ARG_CMDS: [&str; 3] = ["toggle-roster", "toggle-inspector", "clear"];
            let words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
            let mut remaining_words: Vec<String> = Vec::new();
            let mut brainstorm_args: Option<Vec<String>> = None;
            let mut ran_ui_cmd = false;

            for word in words {
                // Once the rest-of-line command (team-brainstorm) is open, everything
                // trailing becomes its argument.
                if let Some(args) = brainstorm_args.as_mut() {
                    args.push(word);
                    continue;
                }
                if let Some(cmd) = word.strip_prefix('/').filter(|c| !c.is_empty()) {
                    if ZERO_ARG_CMDS.contains(&cmd) {
                        self.handle_chat_command(cmd, "", window, cx);
                        ran_ui_cmd = true;
                        continue;
                    }
                    if cmd == "team-brainstorm" {
                        brainstorm_args = Some(Vec::new());
                        continue;
                    }
                }
                remaining_words.push(word);
            }

            if let Some(args) = brainstorm_args {
                self.handle_chat_command("team-brainstorm", &args.join(" "), window, cx);
                ran_ui_cmd = true;
            }

            let remaining_text = remaining_words.join(" ");
            if remaining_text.is_empty() {
                // Pure command line (or lines with only recognised commands): clear the
                // box if we actually ran something, and stop before posting an empty message.
                if ran_ui_cmd {
                    input.update(cx, |state, cx| state.set_value("", window, cx));
                }
                return;
            }
            // Leftover, non-command text is posted as a human message below.
            text = remaining_text;
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
        self.reproject(&events);

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
    /// Rebuild the completion card from the input's current text and cursor.
    /// Sets `self.completion` to `None` when no `@`/`:`/`/` query is live.
    fn recompute_completion(&mut self, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let text = state.value().to_string();
        let cursor = state.cursor();
        let quarks: Vec<(String, Option<String>)> = self
            .team
            .quarks
            .iter()
            .map(|q| (q.id.0.clone(), q.display_name.clone()))
            .collect();
        let files = self.completion_files.borrow();
        let result = crate::text::completion_candidates(&text, cursor, &quarks, files.as_slice());
        drop(files);
        self.completion = result.map(|c| CompletionCard {
            start: c.start,
            candidates: c.candidates,
            selected: 0,
        });
    }

    /// Move the card's highlight by `delta`, clamped to the list. No-op with no card.
    fn move_completion_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(card) = &mut self.completion {
            let len = card.candidates.len();
            if len == 0 {
                return;
            }
            let max = len as isize - 1;
            card.selected = (card.selected as isize + delta).clamp(0, max) as usize;
            cx.notify();
        }
    }

    /// Accept the highlighted row: splice its `new_text` over `input[start..cursor]`
    /// and put the caret just after it. Byte offsets throughout — `cursor()` and
    /// `set_selected_range` are both documented UTF-8, and the cursor is clamped to a
    /// char boundary first, so this cannot slice mid-character (the emoji crash class).
    fn accept_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.completion.take() else {
            return;
        };
        let Some(cand) = card.candidates.get(card.selected).or_else(|| card.candidates.first())
        else {
            cx.notify();
            return;
        };
        let new_text = cand.new_text.clone();
        let value = self.input.read(cx).value().to_string();
        let mut cursor = self.input.read(cx).cursor().min(value.len());
        while cursor > 0 && !value.is_char_boundary(cursor) {
            cursor -= 1;
        }
        let start = card.start.min(cursor);
        let new_value = format!("{}{}{}", &value[..start], new_text, &value[cursor..]);
        let new_cursor = start + new_text.len();
        self.input.update(cx, |state, cx| {
            state.set_value(new_value, window, cx);
            state.set_selected_range(new_cursor..new_cursor, cx);
        });
        cx.notify();
    }

    /// The completion card: rows floating just above the message box, spanning the
    /// input's full width. It is a normal render-tree descendant — `.absolute()`
    /// with `.bottom(100%)` inside the input area's `.relative()` wrapper — so it
    /// draws *upward* and stays inside the window, unlike the fork's `deferred()`
    /// menu that painted off the bottom edge (`completion-menu-draws-out-of-bounds`).
    fn completion_card_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let card = self.completion.as_ref();
        let mut list = v_flex()
            .id("completion-card")
            .absolute()
            .bottom(gpui::relative(1.0))
            .left_0()
            .right_0()
            .mb_2()
            .occlude()
            .max_h(px(280.0))
            .overflow_y_scroll()
            .p_1()
            .gap_1()
            .rounded_lg()
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::border());

        if let Some(card) = card {
            let sel = card.selected.min(card.candidates.len().saturating_sub(1));
            for (i, cand) in card.candidates.iter().enumerate() {
                let selected = i == sel;
                let label = cand.label.clone();
                let detail = cand.detail.clone();
                list = list.child(
                    div()
                        .id(("completion-row", i))
                        .flex()
                        .justify_between()
                        .items_center()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .when(selected, |s| s.bg(theme::bg_surface_raised()))
                        .hover(|s| s.bg(theme::bg_surface_raised()))
                        .child(div().text_sm().text_color(theme::text()).child(label))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(detail),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(c) = this.completion.as_mut() {
                                c.selected = i;
                            }
                            this.accept_completion(window, cx);
                        })),
                );
            }
        }
        list
    }


    // ── Keyboard navigation ──────────────────────────────────────────────
    // These are driven by actions bound at the Chamber key context. Only keys the
    // focused text input does NOT claim (see `crate::input::CONTEXT`) bubble up to
    // the Chamber context, so the bound chords are deliberately chosen to avoid the
    // input's editing chords — that is what lets tab navigation work *while* the
    // chat box has focus, instead of being silently swallowed by it.






}

impl Render for Chamber {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Drain a path chosen from the native avatar picker. The picker task runs
        // without a `Window`, so it parks the path here; `render` has the window and
        // is the first place `set_value` can apply it. Committing persists it so the
        // avatar sticks without a separate Done click.
        if let Some(path) = self.pending_image_pick.take() {
            self.settings_path
                .update(cx, |s, cx| s.set_value(path, window, cx));
            self.commit_settings_inputs(cx);
        }
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
        let settings = self.settings_open.then(|| self.settings_overlay(cx));
        let info = self
            .info_panel
            .is_some()
            .then(|| self.info_panel_overlay(cx));
        let about = self.about_open.then(|| self.about_overlay(cx));
        let app_menu = self.app_menu_open.then(|| self.app_menu_overlay(cx));

        let content = v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &CycleMode, _, cx| this.cycle_global_mode(cx)))
            .on_action(cx.listener(|this, _: &NextChatTab, _, cx| this.cycle_chat_tab(1, cx)))
            .on_action(cx.listener(|this, _: &PrevChatTab, _, cx| this.cycle_chat_tab(-1, cx)))
            .on_action(
                cx.listener(|this, _: &NextInspectorTab, _, cx| this.cycle_inspector_tab(1, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PrevInspectorTab, _, cx| this.cycle_inspector_tab(-1, cx)),
            )
            .on_action(cx.listener(|this, _: &NextStatsSubTab, _, cx| this.cycle_stats_window(1, cx)))
            .on_action(
                cx.listener(|this, _: &PrevStatsSubTab, _, cx| this.cycle_stats_window(-1, cx)),
            )
            .on_action(cx.listener(|this, _: &NextQuark, _, cx| this.move_quark_selection(1, cx)))
            .on_action(cx.listener(|this, _: &PrevQuark, _, cx| this.move_quark_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &ToggleSelectedQuark, _, cx| this.open_selected_quark(cx)))
            .on_action(cx.listener(|this, _: &OpenMenu, _, cx| {
                this.app_menu_open = !this.app_menu_open;
                cx.notify();
            }))
            .relative()
            .size_full()
            .overflow_hidden()
            // The opaque housing tone; the ambient quark-state field (below) washes over
            // it, and the translucent panels let it glow through.
            .bg(theme::window_glint())
            .rounded_tl(top_radius)
            .rounded_tr(top_radius)
            .rounded_bl(bottom_radius)
            .rounded_br(bottom_radius)
            .text_color(theme::text())
            // The ambient field: a bright blue-violet glow, painted first so every panel
            // floats over it. A base wash (bright top -> deep bottom) plus soft bright glows
            // down the two side edges give the "Built"-style lit surround with a darker
            // centre behind the panels. Static gradients only (no blur / no animation), so
            // it costs only per-repaint — tune the angles/tones freely.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .overflow_hidden()
                    .child(wash_layer(
                        180.0,
                        theme::field_bright(),
                        theme::field_deep(),
                        top_radius,
                        bottom_radius,
                    ))
                    // Quark-state hues, one per corner (angle points at the OPPOSITE corner,
                    // so the hue sits bright in the named corner and fades across).
                    .child(glow_layer(135.0, theme::glow_blue(), top_radius, bottom_radius)) // working — top-left
                    .child(glow_layer(225.0, theme::glow_pink(), top_radius, bottom_radius)) // thinking — top-right
                    .child(glow_layer(45.0, theme::glow_green(), top_radius, bottom_radius)), // available — bottom-left
            )
            .child(titlebar)
            .child(body)
            .children(settings)
            .children(info)
            .children(about)
            .children(app_menu);

        let wrapped_content = crate::window_frame::window_frame(window, cx, content);

        div().size_full().child(wrapped_content).into_any_element()
    }
}

impl Chamber {
    fn titlebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(drag_region("drag-c"))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .h_full()
                    .items_center()
                    .justify_end()
                    .child(controls),
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
        // A folded rail is a rounded smoked-glass pill, matching the expanded panels — a
        // square bar here broke the window's rounded corners at the edge. It fills a p_2
        // gutter (added at the return) so it floats with the same edge gap and the same
        // height as an expanded panel, rather than sticking to the edge and running taller.
        let mut col = v_flex()
            .id(id)
            .h_full()
            .w_full()
            .py_2()
            .items_center()
            .gap_2()
            .rounded(INNER_RADIUS)
            .bg(theme::glass_surface())
            .border_1()
            .border_color(theme::glass_highlight())
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
        // The p_2 gutter: same inset as the expanded panels, so collapsing a rail keeps the
        // edge gap and the height instead of snapping flush and taller.
        v_flex()
            .flex_none()
            .w(px(RAIL_STRIP))
            .h_full()
            .min_h_0()
            .p_2()
            .child(col)
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
        for (ix, r) in self.view.roster.iter().enumerate() {
            let is_selected = self.selected_quark_ix == Some(ix);
            // The per-quark mode tag is clickable → cycle this quark's override.
            let qid = r.id.clone();
            let mode_el = div()
                .id(SharedString::from(format!("mode-{}", r.id)))
                .cursor_pointer()
                .flex_none()
                .on_click(cx.listener(move |this, _, _, cx| this.cycle_quark_mode(&qid, cx)))
                .child(mode_tag(r.mode, r.mode_is_override))
                .into_any_element();

            // Restart is meaningful for any resident (ACP) seat — a one-shot CLI quark
            // holds nothing between turns. NOT gated on `adopted`: the daemon seats
            // resident quarks straight from the global catalogue (adopted=false in this
            // repo, but very much live), and `reset_session` is idempotent, so a click
            // on a seat with no live session is a harmless no-op.
            let is_acp = matches!(r.transport, hadron_lattice::Transport::Acp);

            // Trailing controls, right-aligned: effort tag (when set) and mode tag (click
            // to cycle a per-quark override). Each is added only when it has content, so
            // empty slots don't leave phantom gaps. Restart lives in the right-click
            // context menu now (below), not as an always-on row glyph.
            let mut controls = h_flex().flex_none().items_center().gap_1p5();
            if matches!(r.effort.as_deref(), Some(e) if !e.is_empty()) {
                controls = controls.child(effort_tag(&r.effort));
            }
            controls = controls.child(mode_el);
            let controls = controls.into_any_element();

            // The row needs a stable id: `ContextMenuExt` derives the popup's
            // ElementId from its parent's, and with no parent id it falls back to
            // a stack address — every row in the loop then shares one menu state.
            let row_el = div()
                .id(SharedString::from(format!("roster-row-{}", r.id)))
                .rounded(px(8.0))
                .border_1()
                // Keyboard-cursor cue: a fuchsia ring, matching the slash-command accent.
                // Transparent when unselected so rows don't shift by a border width.
                .border_color(if is_selected {
                    gpui::rgb(0xe879f9).into()
                } else {
                    gpui::transparent_black()
                })
                .context_menu({
                    let qid_str = r.id.clone();
                    let enable_str = if r.enabled { "Disable" } else { "Enable" };
                    let r_flavor = r.flavor.clone();
                    let is_adopted = r.adopted;
                    let menu_is_acp = is_acp;
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
                        // Restart is offered for any resident (ACP) seat — adopted or
                        // catalogue-seated (the daemon seats residents straight from the
                        // global catalogue, so a live quark can read adopted=false here).
                        // A one-shot CLI quark holds nothing resident, so it is omitted.
                        if menu_is_acp {
                            let qid_r = qid_str.clone();
                            let view_r = view.clone();
                            menu = menu.item(PopupMenuItem::new("Restart").on_click(
                                move |_, window, cx| {
                                    view_r.update(cx, |this, cx| {
                                        this.handle_context_menu_action(
                                            ContextMenuAction::RestartQuark(qid_r.clone()),
                                            cx,
                                        );
                                    });
                                    window.refresh();
                                },
                            ));
                        }
                        // A not-adopted (catalogue-only) quark offers just "Adopt";
                        // enable/disable and role changes only apply once it participates.
                        if !is_adopted {
                            let qid_a = qid_str.clone();
                            let view_a = view.clone();
                            menu = menu.item(PopupMenuItem::new("Adopt into repo").on_click(
                                move |_, window, cx| {
                                    view_a.update(cx, |this, cx| {
                                        this.handle_context_menu_action(
                                            ContextMenuAction::AdoptQuark(qid_a.clone()),
                                            cx,
                                        );
                                    });
                                    window.refresh();
                                },
                            ));
                            return menu;
                        }
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
                .child(roster_row(&self.resolve_identity(&r.id), r, controls));
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

        // The roster is a smoked-glass panel like the chat/terminal cards, so its quark
        // names stay legible over the bright field (a bare rail washed out). It floats in
        // a p_2 gutter that shows the field around it.
        let card = v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .p_2()
            .gap_2()
            .rounded(INNER_RADIUS)
            .bg(theme::glass_surface())
            .border_1()
            .border_color(theme::glass_highlight())
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
            .child(self.settings_button(cx, false));

        v_flex().w_full().h_full().min_h_0().p_2().child(card)
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
                        ChatTab::Stats => div()
                            .id("session-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.chat_scrolls[selected.index()])
                            .child(self.stats_view(cx))
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
                    // Anchor for the completion card, which is `.absolute()` above.
                    .relative()
                    // The focused Input binds Up/Down/Escape at the deepest node, so
                    // intercept those actions in the capture phase (ancestor-first)
                    // while a card is open — move the highlight / close it instead of
                    // moving the caret. Gated on `is_some()` so normal cursor movement
                    // is untouched when there is no card (advisor's trap #1).
                    .capture_action(cx.listener(|this, _: &MoveDown, _window, cx| {
                        if this.completion.is_some() {
                            this.move_completion_selection(1, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &MoveUp, _window, cx| {
                        if this.completion.is_some() {
                            this.move_completion_selection(-1, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &Escape, _window, cx| {
                        if this.completion.take().is_some() {
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }))
                    .when(self.completion.is_some(), |el| {
                        el.child(self.completion_card_overlay(cx))
                    })
                    .child(
                        h_flex()
                            .px_1()
                            .rounded_lg()
                            .bg(theme::input_bg())
                            // A hairline border lifts the field off the card behind it
                            // — the modern outlined-input look, using the shared token.
                            .border_1()
                            .border_color(theme::border())
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
            // Glass: a faint top sheen + a hairline top highlight, so the dark
            // layer reads as a lit panel rather than a flat black rectangle.
            .bg(theme::glass_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .child(header)
            .children(self.permission_toast(cx))
            .child(body)
            .children(input);

        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .p_2()
            // No fill here: the ambient field is the backdrop, so the card reads as a
            // single pane of glass floating on it. A second fill would stack with the
            // card's translucent glass and hide the field; the p_2 gutter shows it.
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
            .p_3()
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

                                let mut row = v_flex().w_full();
                                if add_divider {
                                    let label = crate::model::date_divider_label(
                                        m.ts.date_naive(),
                                        chrono::Local::now().date_naive(),
                                    );
                                    row = row.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .pt_3()
                                            .pb_2()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(theme::text_muted())
                                                    .child(label),
                                            ),
                                    );
                                }

                                let expanded = this.log_expanded.contains(&ix);
                                return row
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("log-row-{ix}")))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !this.log_expanded.remove(&ix) {
                                                    this.log_expanded.insert(ix);
                                                }
                                                cx.notify();
                                            }))
                                            .child(log_row(m, expanded, this.color_for(&m.from))),
                                    )
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

    /// The Session / Week / Month / All-time selector shared by the chat Stats tab and
    /// the info panel's Stats tab. `id` distinguishes the two (both can be in the tree at
    /// once — the info panel overlays the chat pane), so their element ids never collide.
    fn stats_window_tabs(&self, id: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.stats_window;
        let sel_ix = StatsWindow::ALL
            .iter()
            .position(|w| *w == selected)
            .unwrap_or(0);
        TabBar::new(id)
            .segmented()
            .selected_index(sel_ix)
            .children(StatsWindow::ALL.map(|w| {
                if w == selected {
                    Tab::new().child(
                        div()
                            .text_color(theme::accent())
                            .child(w.label().to_string()),
                    )
                } else {
                    Tab::new().label(w.label())
                }
            }))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.stats_window = StatsWindow::ALL
                    .get(*ix)
                    .copied()
                    .unwrap_or(StatsWindow::Session);
                cx.notify();
            }))
    }

    /// The chat column's Stats tab: team-wide telemetry over the selected window.
    fn stats_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let stats =
            self.view
                .stats_for(&self.archived_messages, self.stats_window, chrono::Utc::now());

        let mut col = v_flex().p_4().gap_4();
        col = col.child(self.stats_window_tabs("chat-stats-window-tabs", cx));
        // Session totals as a row of KPI tiles.
        col = col.child(
            h_flex()
                .w_full()
                .gap_3()
                .child(stat_tile(
                    "Turns",
                    stats.total_turns.to_string(),
                    theme::text(),
                ))
                .child(stat_tile(
                    "Fresh",
                    format_num(stats.total_fresh),
                    theme::accent(),
                ))
                .child(stat_tile(
                    "Cached",
                    format_num(stats.total_cached),
                    theme::accent_secondary(),
                ))
                .child(stat_tile(
                    "Cost",
                    stats
                        .total_cost_usd
                        .map(|c| format!("${:.2}", c))
                        .unwrap_or_else(|| "—".to_string()),
                    rgb(0x22c55e),
                )),
        );

        // Combined spend chart: cumulative fresh spend over turns, one translucent area
        // per quark (its colour) with the team total as a stroke-only line on top — being
        // the running sum it sits above every quark band without hiding them.
        let timeline =
            self.view
                .spend_timeline(&self.archived_messages, self.stats_window, chrono::Utc::now());
        if !timeline.points.is_empty() {
            let mut chart = AreaChart::new(timeline.points.clone())
                .id("session-spend-area")
                .x(|d| format!("T{}", d.step));
            for (i, q) in timeline.quarks.iter().enumerate() {
                let color = self.color_for(q);
                chart = chart
                    .y(move |d| d.per_quark[i])
                    .stroke(color)
                    .fill(linear_gradient(
                        0.0,
                        linear_color_stop(color.opacity(0.28), 1.0),
                        linear_color_stop(color.opacity(0.02), 0.0),
                    ))
                    .name(q.clone())
                    .natural();
            }
            // The team total: a bright accent line, transparent fill (a line, not a band).
            chart = chart
                .y(|d| d.team)
                .stroke(theme::accent())
                .fill(gpui::rgba(0x00000000))
                .name("Team")
                .natural();
            col = col.child(
                session_card()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text())
                            .child("Cumulative spend over turns"),
                    )
                    .child(div().h(px(180.0)).w_full().child(chart)),
            );
        }

        for (q, s) in &stats.per_quark {
            let q_color = self.color_for(q);
            let mut block = session_card().child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
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
                            .child(format!("{} turns", s.turns)),
                    ),
            );
            block = block.child(
                div()
                    .text_xs()
                    .text_color(theme::text_secondary())
                    .child(format!(
                        "{} fresh · {} cached{}",
                        format_num(s.fresh),
                        format_num(s.cached),
                        s.cost_usd
                            .map(|c| format!(" · ${:.2}", c))
                            .unwrap_or_default(),
                    )),
            );

            if let Some(ctx) = &s.context {
                let frac = (ctx.used_percentage as f32 / 100.0).clamp(0.0, 1.0);
                block = block
                    .child(
                        div().text_xs().text_color(theme::text_muted()).child(format!(
                            "Context {:.0}% · {} / {}",
                            ctx.used_percentage,
                            format_num(ctx.used_tokens),
                            format_num(ctx.context_window_size),
                        )),
                    )
                    .child(progress_meter(frac, q_color));
            }
            // An empty quota list means the provider has no quota concept — not that the
            // quota is spent. Say nothing rather than render a zero.
            for bucket in &s.quota {
                block = block.child(div().text_xs().text_color(theme::text_muted()).child(
                    format!(
                        "Quota [{}]: {:.0}% left",
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
                // The live grid: one styled row per terminal line, each line a
                // few coalesced same-colour runs (not one element per cell — this
                // box CPU-rasterises every frame). The block cursor is an inverted
                // cell baked into the snapshot.
                let grid: gpui::AnyElement = if let Some(term) = &self.terminal {
                    let snap = term.snapshot();
                    let mut rows = v_flex()
                        .flex_1()
                        .min_h_0()
                        .p_2()
                        .font_family("Cascadia Code")
                        .text_size(px(TERM_FONT))
                        .line_height(px(TERM_CELL_H));
                    for line in &snap.lines {
                        let mut row = h_flex().h(px(TERM_CELL_H));
                        for run in &line.runs {
                            row = row.child(
                                div()
                                    .text_color(rgb(pack_rgb(run.fg)))
                                    .bg(rgb(pack_rgb(run.bg)))
                                    .child(run.text.clone()),
                            );
                        }
                        rows = rows.child(row);
                    }
                    rows.into_any_element()
                } else {
                    div()
                        .flex_1()
                        .p_3()
                        .font_family("Cascadia Code")
                        .text_size(px(TERM_FONT))
                        .text_color(theme::text_muted())
                        .child("starting shell…")
                        .into_any_element()
                };

                // A paint-time probe: report the screen's pixel bounds so the pump
                // loop can size the PTY to fit. It paints nothing.
                let px_cell = self.terminal_px.clone();
                let size_probe = gpui::canvas(
                    move |bounds, _, _| {
                        px_cell.set(Some((
                            f32::from(bounds.size.width),
                            f32::from(bounds.size.height),
                        )));
                    },
                    |_, _: (), _, _| {},
                )
                .absolute()
                .size_full();

                // The terminal "screen": a focusable dark surface. Clicking focuses
                // it; while focused, keystrokes stream to the PTY (`on_terminal_key`).
                let screen = div()
                    .id("terminal-screen")
                    .track_focus(&self.terminal_focus)
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .rounded_md()
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::border())
                    .bg(theme::term_bg())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| window.focus(&this.terminal_focus, cx)),
                    )
                    .on_key_down(cx.listener(Self::on_terminal_key))
                    .child(size_probe)
                    .child(grid);

                v_flex()
                    .flex_1()
                    // Without min-height:0 this flex child grows to the terminal grid's
                    // content height and spills past the container's bottom edge.
                    .min_h_0()
                    .p_3()
                    .child(screen)
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
                                        this.parsed_markdown.borrow_mut().remove(&usize::MAX);
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
                        is_ignored: bool,
                        full_path: String,
                    }
                    impl FileTreeNode {
                        /// `is_dir_leaf` marks a path that is itself a directory (a
                        /// collapsed gitignored dir, kept with a trailing `/` by
                        /// `list_workspace_files`) — its last component is a folder, not
                        /// a file. Interior directories start un-ignored; `resolve_ignores`
                        /// computes their flag from their children afterwards.
                        fn insert(&mut self, path: &str, is_ignored: bool, is_dir_leaf: bool) {
                            let parts: Vec<&str> =
                                path.split('/').filter(|p| !p.is_empty()).collect();
                            if parts.is_empty() {
                                return;
                            }
                            let full = path.trim_end_matches('/');
                            let mut current = self;
                            for (i, part) in parts.iter().enumerate() {
                                let last = i == parts.len() - 1;
                                let is_file = last && !is_dir_leaf;
                                current =
                                    current.children.entry(part.to_string()).or_insert_with(|| {
                                        FileTreeNode {
                                            children: std::collections::BTreeMap::new(),
                                            is_file,
                                            is_ignored: false,
                                            full_path: String::new(),
                                        }
                                    });
                                if last {
                                    current.is_file = is_file;
                                    current.is_ignored = is_ignored;
                                    if current.full_path.is_empty() {
                                        current.full_path = full.to_string();
                                    }
                                }
                            }
                        }

                        /// Bottom-up: a file/collapsed-dir keeps its own flag; a directory
                        /// with children is ignored only when **every** child is. Returns
                        /// this node's resolved ignored state so the parent can fold it in.
                        fn resolve_ignores(&mut self) -> bool {
                            if self.is_file || self.children.is_empty() {
                                return self.is_ignored;
                            }
                            let mut all_ignored = true;
                            for child in self.children.values_mut() {
                                if !child.resolve_ignores() {
                                    all_ignored = false;
                                }
                            }
                            self.is_ignored = all_ignored;
                            all_ignored
                        }
                    }

                    let mut root_node = FileTreeNode::default();
                    for (file, is_ignored) in &self.file_tree_paths {
                        root_node.insert(file, *is_ignored, file.ends_with('/'));
                    }
                    root_node.resolve_ignores();

                    let repo_root =
                        crate::vcs::repo_root_of(std::path::Path::new(&self.path)).to_path_buf();

                    // Folders before files, alphabetical within each group — the
                    // convention every file explorer uses. Applied at every level.
                    fn sorted_children(node: &FileTreeNode) -> Vec<(&String, &FileTreeNode)> {
                        let mut children: Vec<(&String, &FileTreeNode)> =
                            node.children.iter().collect();
                        children.sort_by(|(a_name, a), (b_name, b)| {
                            match (a.is_file, b.is_file) {
                                (false, true) => std::cmp::Ordering::Less,
                                (true, false) => std::cmp::Ordering::Greater,
                                _ => a_name.cmp(b_name),
                            }
                        });
                        children
                    }

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
                            for (child_name, child_node) in sorted_children(node) {
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
                            // Gitignored entries read as present-but-inactive: muted text.
                            .text_color(if node.is_ignored {
                                theme::text_muted()
                            } else {
                                theme::text()
                            })
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
                                            this.parsed_markdown.borrow_mut().remove(&usize::MAX);
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
                                for (child_name, child_node) in sorted_children(node) {
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
            RightRailTab::Plan => {
                let repo = crate::vcs::repo_root_of(&self.path).to_path_buf();

                // The active plan is the most-recently-mentioned plan file in the field:
                // scan message bodies newest-first for a `plans/….md` reference. This
                // covers the orchestrator's assignment ("execute docs/…/plan.md") and any
                // later re-reference. The projection has no `task` field, so the field's
                // messages are the source of truth.
                let active_plan_path = self
                    .view
                    .messages
                    .iter()
                    .rev()
                    .find_map(|m| hadron_gluon::skills::plan_ref(&m.body));

                // Resolve the referenced plan to its on-disk content in one step; either
                // the reference or the file may be absent (a plan can be named before it
                // is written, or removed after).
                let resolved = active_plan_path.and_then(|rel_path| {
                    crate::sys::read_workspace_file(&repo, &rel_path)
                        .map(|content| (rel_path, content))
                });

                let plan_element = match resolved {
                    Some((rel_path, content)) => {
                        let (total, completed, tasks) = parse_plan_progress(&content);
                        let frac = if total > 0 {
                            completed as f32 / total as f32
                        } else {
                            0.0
                        };
                        let pct = (frac * 100.0).round() as usize;

                        let mut list = v_flex().gap_2().p_3().w_full();
                        list = list.child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_sm()
                                .child(format!("Active Plan: {rel_path}")),
                        );
                        list = list.child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(format!("{completed}/{total} steps complete ({pct}%)")),
                        );
                        list = list.child(progress_meter(frac, gpui::rgb(0x34d399)));

                        for (task_desc, done) in tasks {
                            let marker = if done {
                                Icon::new(IconName::CircleCheck)
                                    .small()
                                    .text_color(gpui::rgb(0x34d399))
                                    .into_any_element()
                            } else {
                                // No hollow-circle glyph ships in the icon set, so draw one:
                                // a small ringed dot reads as an empty checkbox.
                                div()
                                    .size(px(14.0))
                                    .flex_shrink_0()
                                    .mt(px(2.0))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(theme::text_muted())
                                    .into_any_element()
                            };
                            list = list.child(
                                h_flex().gap_2().items_start().child(marker).child(
                                    div()
                                        .text_sm()
                                        .text_color(if done {
                                            theme::text_muted()
                                        } else {
                                            theme::text()
                                        })
                                        .child(task_desc),
                                ),
                            );
                        }
                        list.into_any_element()
                    }
                    None => div()
                        .p_4()
                        .text_color(theme::text_muted())
                        .child("No active implementation plan referenced in the field yet.")
                        .into_any_element(),
                };

                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("plan-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.plan_scroll)
                            .text_sm()
                            .text_color(theme::text())
                            .child(plan_element),
                    )
                    .child(
                        div().absolute().top_0().bottom_0().right_0().child(
                            Scrollbar::vertical(&self.plan_scroll)
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
            // Glass, matching the chat card: faint sheen + hairline top highlight.
            .bg(theme::glass_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .child(header)
            .child(content);

        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .p_2()
            // No fill here: the ambient field is the backdrop, so the card reads as a
            // single pane of glass floating on it. A second fill would stack with the
            // card's translucent glass and hide the field; the p_2 gutter shows it.
            .child(card)
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

        let adopted = self.view.roster.iter().filter(|r| r.adopted).count();
        let available = self.view.roster.len().saturating_sub(adopted);
        let workspace = crate::vcs::repo_root_of(&self.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| crate::vcs::repo_root_of(&self.path).to_string_lossy().to_string());

        // Signature brand motif: the four quark energies as a small constellation of dots,
        // echoing the field's corner glows.
        let quark_dots = h_flex().gap_1p5().items_center().children(
            [0x38bdf8u32, 0xec4899, 0x34d399, 0xfbbf24]
                .into_iter()
                .map(|c| div().size(px(9.0)).rounded_full().bg(rgb(c)).into_any_element()),
        );

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.about_open = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .occlude()
                    .w(px(420.0))
                    .p_5()
                    .gap_4()
                    .rounded(INNER_RADIUS)
                    // Opaque, like the info panel and Settings: a focused dialog must not
                    // let the bright field bleed through (glass_surface read as too
                    // transparent). One shared modal token so every dialog matches.
                    .bg(theme::modal_surface())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {}) // swallow inner clicks
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(quark_dots)
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text())
                                    .child("Hadron"),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_secondary())
                            .child("A multi-agent operating system. Quarks take turns in one shared workspace, on one shared field."),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(panel_eyebrow("BUILD"))
                            .child(kv_row("Version", env!("CARGO_PKG_VERSION")))
                            .child(kv_row("Licence", "Apache-2.0"))
                            .child(kv_row("Workspace", workspace))
                            .child(kv_row(
                                "Quarks",
                                format!("{adopted} adopted · {available} available"),
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child("Built on GPUI (Zed) and gpui-component (Longbridge), and speaks the Agent Client Protocol."),
                    )
                    .child(
                        div()
                            .id("about-close")
                            .self_end()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(theme::bg_surface_raised())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::glass_highlight()))
                            .text_sm()
                            .text_color(theme::text())
                            .child("Close")
                            .on_click(close),
                    ),
            )
    }








    /// The per-quark permission ladder (Ask / Write / Auto / Bypass) as an explicit
    /// segmented picker for Settings. Unlike the roster's cycle-on-click tag, each rung is
    /// directly selectable, the current resolved mode is highlighted on its risk colour,
    /// and a gloss explains what the choice delegates. The leading **Default** rung clears
    /// any override (`ModeClear`) so the quark follows the global default; the four posture
    /// rungs each pin a per-quark `ModeSet` override. The daemon honours it next tick.
    fn mode_select(&self, id: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (current, is_override) = self
            .view
            .roster
            .iter()
            .find(|r| r.id == id)
            .map(|r| (r.mode, r.mode_is_override))
            .unwrap_or((self.view.global_mode, false));

        // The "Default" rung is inheriting the global default; a concrete rung pins a
        // per-quark override. So Default is selected exactly when there is no override,
        // and a posture rung highlights only when it is the *pinned* one — otherwise a
        // quark inheriting a global "Write" would look identical to one pinned to Write.
        let mut row = h_flex().gap_1p5().flex_wrap();
        {
            let id_str = id.to_string();
            let selected = !is_override;
            row = row.child(
                div()
                    .id(SharedString::from(format!("mode-{id}-default")))
                    .px_2p5()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .text_sm()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme::bg_surface_raised())
                            .border_color(theme::text_secondary())
                            .text_color(theme::text())
                    })
                    .when(!selected, |d| {
                        d.bg(theme::bg_surface())
                            .border_color(theme::border())
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                    })
                    .child("Default")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clear_quark_mode(&id_str, cx);
                        cx.notify();
                    })),
            );
        }
        for m in [Mode::Ask, Mode::Write, Mode::Auto, Mode::Bypass] {
            let selected = is_override && m == current;
            let id_str = id.to_string();
            row = row.child(
                div()
                    .id(SharedString::from(format!("mode-{id}-{}", mode_label(m))))
                    .px_2p5()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .text_sm()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(mode_color(m)).border_color(mode_color(m)).text_color(theme::text())
                    })
                    .when(!selected, |d| {
                        d.bg(theme::bg_surface())
                            .border_color(theme::border())
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                    })
                    .child(mode_label(m))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_quark_mode(&id_str, m, cx);
                        cx.notify();
                    })),
            );
        }

        v_flex()
            .gap_1p5()
            .child(row)
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(mode_hint(current).to_string()),
            )
            .child(div().text_xs().text_color(theme::text_muted()).child(if is_override {
                format!("Pinned for this quark ({}) — the global default no longer moves it.", mode_label(current))
            } else {
                format!("Default — following the global setting ({}).", mode_label(current))
            }))
            .into_any_element()
    }






    /// The Settings overlay: a dim backdrop (click to dismiss) behind a card
    /// that edits one identity — an avatar switcher, a live preview, a display
    /// name, a color swatch row, and an image path (image wins over color).
    /// The keyboard-triggered app menu (F10): the same actions as the hamburger
    /// dropdown, but reachable without the mouse. A full-bleed backdrop dismisses on
    /// any outside click (and swallows it); the panel sits under the top-left button.
    fn app_menu_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        fn item(
            id: &'static str,
            label: &'static str,
            on_click: impl Fn(&mut Chamber, &mut Window, &mut Context<Chamber>) + 'static,
            cx: &mut Context<Chamber>,
        ) -> gpui::AnyElement {
            div()
                .id(id)
                .w_full()
                .px_2()
                .py_1p5()
                .rounded(px(6.0))
                .cursor_pointer()
                .text_sm()
                .text_color(theme::text())
                .hover(|s| s.bg(theme::bg_surface_raised()))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.app_menu_open = false;
                    on_click(this, window, cx);
                    cx.notify();
                }))
                .child(label)
                .into_any_element()
        }

        let sep = || div().h(px(1.0)).w_full().bg(theme::border());

        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.app_menu_open = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .occlude()
                    .absolute()
                    .top(px(44.0))
                    .left(px(12.0))
                    .w(px(280.0))
                    .p_2()
                    .gap_0p5()
                    .rounded(INNER_RADIUS)
                    .bg(theme::modal_surface())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    // Swallow clicks inside the panel so they don't hit the dismiss backdrop.
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {})
                    .child(item(
                        "menu-settings",
                        "Settings…",
                        |this, window, cx| this.open_settings(window, cx),
                        cx,
                    ))
                    .child(sep())
                    .child(item(
                        "menu-reveal",
                        "Reveal Workspace in File Manager",
                        |this, _w, cx| {
                            this.handle_context_menu_action(
                                ContextMenuAction::OpenInFolder(String::from(".")),
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(sep())
                    .child(item(
                        "menu-about",
                        "About Hadron",
                        |this, _w, _cx| this.about_open = true,
                        cx,
                    ))
                    .child(sep())
                    .child(item("menu-quit", "Quit Hadron", |_t, _w, cx| cx.quit(), cx)),
            )
    }







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
    // The repo team file (`.hadron/team.json`) and the separate global catalogue
    // (`~/.hadron/team.json`, skipped when it IS the repo file).
    let repo_path = hadron_lattice::team_for_field(&field_path);
    let global_path = hadron_lattice::team_config_path()
        .filter(|g| Some(g.as_path()) != repo_path.as_deref());

    // One-shot migration to the global-catalogue split: if the repo file still carries
    // legacy full seats and there is a separate catalogue, move each definition into the
    // catalogue and rewrite the repo file as role/state overrides. Idempotent (a repo
    // with no legacy seats is a no-op), and the resolved seats are byte-identical to the
    // originals, so the running daemon reconciles to a no-op re-seat.
    if let (Some(rp), Some(gp)) = (repo_path.as_deref(), global_path.as_deref()) {
        migrate_repo_to_catalogue(rp, gp);
    }

    // Load the SAME repo team the daemon seated for this field, plus the catalogue.
    let team = repo_path.as_deref().map(load_team).unwrap_or_default();
    let global = global_path.as_deref().map(load_team).unwrap_or_default();
    let events = io::read_events(&field_path).unwrap_or_default();
    let view = model::project_with_team(&events, &resolve_team(&team, &global), &global);
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
            // Context menus, dropdown menus and tooltips paint from `tokens.popover`, which
            // is computed once at theme construction and does NOT re-derive from the mutated
            // `colors.popover` above — so without this line they stay the stock-dark theme
            // colour (near-black) instead of our surface. Same gotcha as `tokens.title_bar`.
            // A neutral gray with a touch of translucency — deliberately NOT the violet
            // glass tone, so a menu opened over a glass panel reads as a distinct surface.
            t.tokens.popover = gpui::Hsla::from(gpui::rgba(0x2c2c33e0)).into();
            // now a bright field, so a dark line stood out between the chat and the right
            // pane — make the idle border fully transparent so the handle vanishes at rest.
            // Dragging still paints `drag_border` (on-brand pink) for feedback. This also
            // drops gpui-component's own idle hairlines, which suits the glass surfaces.
            t.border = gpui::rgba(0x00000000).into();
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
        // Keyboard navigation. Every chord here is one the text input's own key
        // context (`crate::input::CONTEXT`) does NOT claim — verified against its
        // KeyBinding set (which takes ctrl/cmd arrows, ctrl-shift arrows, tab,
        // shift-tab, escape, brackets, …). A key the input doesn't bind falls
        // through to this Chamber context even while the chat box has focus, so
        // navigation works mid-typing instead of being swallowed. ctrl-based
        // (not alt/super) to dodge the WM's own workspace chords on Linux/WSL.
        cx.bind_keys([
            KeyBinding::new("shift-tab", CycleMode, Some(KEY_CONTEXT)),
            // Chat column tabs (Chat / Log / Stats) — the universal tab chord.
            KeyBinding::new("ctrl-tab", NextChatTab, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-shift-tab", PrevChatTab, Some(KEY_CONTEXT)),
            // Right rail tabs (Terminal / Files / Changes / Plan) — browser-style.
            KeyBinding::new("ctrl-pagedown", NextInspectorTab, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-pageup", PrevInspectorTab, Some(KEY_CONTEXT)),
            // Stats time window, only while the Stats tab is up.
            KeyBinding::new("ctrl-alt-pagedown", NextStatsSubTab, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-alt-pageup", PrevStatsSubTab, Some(KEY_CONTEXT)),
            // Roster cursor (vim-style j/k) and open-selected.
            KeyBinding::new("ctrl-j", NextQuark, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-k", PrevQuark, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-alt-enter", ToggleSelectedQuark, Some(KEY_CONTEXT)),
            // App menu overlay — F10, the conventional "focus the menu" key.
            KeyBinding::new("f10", OpenMenu, Some(KEY_CONTEXT)),
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
            // Transparent so the rounded corner cut-outs composite over the desktop
            // (see window_frame). The old fear — "transparent recomposites every repaint
            // and pegs the CPU" — was misdiagnosed: the CPU sink was *continuous repaints*
            // (a pulsing animation + a 30fps terminal pump), now fixed. Repaints are rare,
            // so the alpha channel costs almost nothing at rest.
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
                        global.clone(),
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

