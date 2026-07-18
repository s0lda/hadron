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
    io, load_team, resolve_team, Actor, Event, Kind, Mode, QuarkId, SeatOverride, Team,
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
    cli_seat_from, configured_providers, custom_cli_vendor_is_valid, migrate_legacy_ids,
    migrate_repo_to_catalogue, prompt_channel_from, AcpModelProbe, AcpModelState,
    AgentDescriptor, CliChannelChoice, ConfiguredQuark, ProviderState, SettingsTarget,
    WizardState,
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
mod render;

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
    /// The "Custom CLI" wizard form fields (`WizardState::CustomCli`): a generic
    /// `Transport::Cli` seat built by hand rather than probed from an ACP preset. Live
    /// here (not in the `WizardState` variant itself) so the enum stays a plain, cheaply
    /// `Clone + PartialEq`-able value — matching `settings_name`/`settings_model` etc.
    custom_cli_vendor: Entity<InputState>,
    custom_cli_program: Entity<InputState>,
    custom_cli_args: Entity<InputState>,
    custom_cli_model: Entity<InputState>,
    /// The argv flag when `custom_cli_channel` is `Arg` (e.g. `--prompt`); blank means a
    /// bare positional argument (`PromptChannel::Arg { flag: None }`).
    custom_cli_flag: Entity<InputState>,
    /// Which of the two prompt-delivery channels the toggle currently selects.
    custom_cli_channel: CliChannelChoice,
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
    _settings_subs: [Subscription; 6],
    providers: Vec<ConfiguredQuark>,
    wizard_state: WizardState,
    /// Offered-model probe for the ACP quark whose Settings are open — drives the model
    /// dropdown. `None` for a non-ACP target or before the first probe. See `providers`.
    acp_model_probe: Option<AcpModelProbe>,
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
        let custom_cli_vendor = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. ollama"));
        let custom_cli_program = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. ollama or /usr/local/bin/mytool"));
        let custom_cli_args = cx.new(|cx| InputState::new(window, cx).placeholder("space-separated, e.g. run llama3"));
        let custom_cli_model = cx.new(|cx| InputState::new(window, cx).placeholder("model name (optional)"));
        let custom_cli_flag = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. --prompt (blank = positional)"));
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
            // Same reason: the custom-CLI form's "Save" button is only wired up once
            // vendor + program are non-empty (`can_save` in `providers_view`), so typing
            // into either must repaint the wizard for the button to light up live.
            cx.subscribe_in(&custom_cli_vendor, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
            cx.subscribe_in(&custom_cli_program, window, |_, _, _: &InputEvent, _, cx| {
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
            custom_cli_vendor,
            custom_cli_program,
            custom_cli_args,
            custom_cli_model,
            custom_cli_flag,
            custom_cli_channel: CliChannelChoice::default(),
            color_picker,
            pending_image_pick: None,
            _input_sub,
            _settings_subs,
            providers,
            wizard_state: WizardState::None,
            acp_model_probe: None,
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

    /// Re-read the field; if it grew, re-project and repaint. Comparing event
    /// count to the current row count is a cheap change check (projection emits
    /// exactly one row per event), so an unchanged field costs only a read.
    fn reload_if_changed(&mut self, cx: &mut Context<Self>) {
        // Only reproject on a successful read — a transient read error must not
        // blank the current view (which would flash to empty, then repopulate).
        if let Ok(events) = io::read_events(&self.path) {
            let mut changed = false;

            // The team files are edited out-of-band, not only through this window: the
            // daemon re-seats from team.json, and a quark is adopted/removed by writing
            // team.json (or the shared catalogue) directly. Poll them the same dumb way
            // the field is polled, so an externally added/removed quark shows in the
            // roster + Settings at once — instead of only after it first authors an
            // event (event-seeding), which is why a new quark used to "appear after
            // mention". A no-op after this window's own save: the reload matches what
            // was just written, so `!=` is false and nothing reprojects.
            //
            // STRICT read on purpose: `load_team` degrades a missing/half-written file to
            // an EMPTY team. Assigning that would blank the roster for a frame and — if a
            // settings save landed in the next ~400ms tick — persist an empty team.json,
            // which the polling daemon would read as "unseat the whole swarm". So parse
            // with the error kept and, on ANY read/parse failure, leave the in-memory team
            // untouched (mirrors the events guard above; `save_team`'s atomic rename is
            // what protects concurrent *writers*, this protects concurrent *readers*).
            let read_strict = |path: &std::path::Path| -> Option<Team> {
                std::fs::read_to_string(path)
                    .ok()
                    .and_then(|t| hadron_lattice::parse_team(&t).ok())
            };
            let repo_team = read_strict(&self.repo_team_path());
            let global_team = match hadron_lattice::team_config_path() {
                Some(p) => read_strict(&p),
                None => Some(Team::default()), // no catalogue configured → genuinely empty
            };
            let mut team_changed = false;
            if let (Some(repo_team), Some(global_team)) = (repo_team, global_team) {
                if repo_team != self.team || global_team != self.global {
                    self.team = repo_team;
                    self.global = global_team;
                    self.providers =
                        configured_providers(&resolve_team(&self.team, &self.global));
                    team_changed = true;
                    changed = true;
                }
            }

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
            } else if team_changed {
                // The message list is unchanged (same events) but the resolved team is
                // not — refresh the view so the new roster/Settings render. No
                // message-count change, so the virtualized chat/log lists need no splice.
                self.reproject(&events);
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
        let full = input.read(cx).value().trim().to_string();
        if full.is_empty() {
            return;
        }

        // A line may begin with chained UI commands, then a normal message, e.g.
        // "/toggle-roster /clear ping the team". `split_leading_commands` peels the
        // leading `/command` tokens; the returned body is the untouched remainder, so a
        // multi-line message (Shift+Enter, a markdown list) keeps its newlines.
        let (cmds, body) = split_leading_commands(&full);
        for (cmd, args) in &cmds {
            self.handle_chat_command(cmd, args, window, cx);
        }
        let text = match body {
            Some(body) => body,
            None => {
                // Only recognised commands were present (`body` is `None` only when at
                // least one command ran): clear the box and stop before posting nothing.
                input.update(cx, |state, cx| state.set_value("", window, cx));
                return;
            }
        };

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

}

/// Launch the chamber window against a field file path.
/// Peel the leading `/command` tokens off a submitted chat line. Returns the commands
/// to run in order — each as `(name, args)` — and the leftover message body.
///
/// Only *leading* commands are recognised: the first token that is not a known command
/// ends parsing, and everything from there is the message (so a body can contain a
/// literal "/foo"). Zero-arg commands (`toggle-roster` / `toggle-inspector` / `clear`)
/// chain; `/team-brainstorm` takes the rest of the line as its argument. The body is a
/// slice of the ORIGINAL input, so internal newlines survive — it is never rebuilt from
/// whitespace-split tokens. `body` is `None` only when at least one command ran and no
/// message text remains, which is how the caller knows to clear the box and post nothing.
fn split_leading_commands(full: &str) -> (Vec<(String, String)>, Option<String>) {
    const ZERO_ARG_CMDS: [&str; 3] = ["toggle-roster", "toggle-inspector", "clear"];
    let mut cmds = Vec::new();
    let mut rest = full;

    loop {
        let head = rest.trim_start();
        let tok_end = head.find(char::is_whitespace).unwrap_or(head.len());
        let token = &head[..tok_end];
        match token.strip_prefix('/').filter(|c| !c.is_empty()) {
            Some(cmd) if ZERO_ARG_CMDS.contains(&cmd) => {
                cmds.push((cmd.to_string(), String::new()));
                rest = &head[tok_end..];
            }
            Some("team-brainstorm") => {
                cmds.push(("team-brainstorm".to_string(), head[tok_end..].trim().to_string()));
                // team-brainstorm consumes the rest of the line, so nothing is left to post.
                return (cmds, None);
            }
            // First non-command token: the untouched remainder is the message body.
            _ => break,
        }
    }

    let remaining = rest.trim();
    let body = (!remaining.is_empty()).then(|| remaining.to_string());
    (cmds, body)
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

    // Load prefs before either migration below, so the id-rename can move ChamberPrefs'
    // per-quark identity (colour/name/avatar) onto the renamed keys and persist it.
    let mut prefs = config::load();

    // One-shot migration to the global-catalogue split: if the repo file still carries
    // legacy full seats and there is a separate catalogue, move each definition into the
    // catalogue and rewrite the repo file as role/state overrides. Idempotent (a repo
    // with no legacy seats is a no-op), and the resolved seats are byte-identical to the
    // originals, so the running daemon reconciles to a no-op re-seat.
    if let (Some(rp), Some(gp)) = (repo_path.as_deref(), global_path.as_deref()) {
        migrate_repo_to_catalogue(rp, gp);
        // One-shot: rename legacy ids (`agy` → `cli-agy`, `opus` → `cli-claude`) to the
        // `<transport>-<vendor>` convention in both team files, and move the chamber's own
        // per-quark identity onto the new keys so a rename doesn't reset a quark's
        // appearance. Idempotent — safe on every launch.
        migrate_legacy_ids(rp, gp, &mut prefs);
        let _ = config::save(&prefs);
    }

    // Load the SAME repo team the daemon seated for this field, plus the catalogue.
    let team = repo_path.as_deref().map(load_team).unwrap_or_default();
    let global = global_path.as_deref().map(load_team).unwrap_or_default();
    let events = io::read_events(&field_path).unwrap_or_default();
    let view = model::project_with_team(&events, &resolve_team(&team, &global), &global);

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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_commands_peel_and_preserve_message_newlines() {
        // No command: the whole body is returned verbatim, newlines intact.
        let (cmds, body) = split_leading_commands("line one\nline two");
        assert!(cmds.is_empty());
        assert_eq!(body.as_deref(), Some("line one\nline two"));

        // A leading command runs; the multi-line remainder keeps its newline (this is the
        // regression guard — the old tokenizer flattened it to "line one line two").
        let (cmds, body) = split_leading_commands("/clear line one\nline two");
        assert_eq!(cmds, vec![("clear".to_string(), String::new())]);
        assert_eq!(body.as_deref(), Some("line one\nline two"));

        // Chained zero-arg commands, no message left → body None (caller clears the box).
        let (cmds, body) = split_leading_commands("/toggle-roster /clear");
        assert_eq!(
            cmds,
            vec![
                ("toggle-roster".to_string(), String::new()),
                ("clear".to_string(), String::new()),
            ]
        );
        assert_eq!(body, None);

        // team-brainstorm swallows the rest of the line as its argument.
        let (cmds, body) = split_leading_commands("/team-brainstorm ship the release");
        assert_eq!(cmds, vec![("team-brainstorm".to_string(), "ship the release".to_string())]);
        assert_eq!(body, None);

        // A "/command" that is NOT leading stays literal text in the message body.
        let (cmds, body) = split_leading_commands("please run /clear later");
        assert!(cmds.is_empty());
        assert_eq!(body.as_deref(), Some("please run /clear later"));

        // A bare slash is not a command.
        let (cmds, body) = split_leading_commands("/ and /clear");
        assert!(cmds.is_empty());
        assert_eq!(body.as_deref(), Some("/ and /clear"));
    }
}
