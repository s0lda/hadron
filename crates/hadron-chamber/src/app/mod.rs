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
    FocusHandle, Focusable, Hsla, KeyBinding, MouseButton, Pixels, Render, Rgba, ScrollHandle,
    SharedString, Subscription, Window, WindowBackgroundAppearance, WindowBounds,
    WindowControlArea, WindowDecorations, WindowOptions,
};
use gpui_component::avatar::Avatar;
// badge removed
use gpui_component::chart::AreaChart;
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::scroll::{ScrollableElement, Scrollbar, ScrollbarShow};
use gpui_component::stepper::{Stepper, StepperItem};
use gpui_component::switch::Switch;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::tag::Tag;
use gpui_component::tooltip::Tooltip;

// table imports removed
use gpui_component::{
    h_flex, v_flex, Icon, IconName, Root, Sizable, Size, Theme, ThemeMode, TitleBar,
};
use hadron_lattice::{
    io, load_team, resolve_team, Actor, Event, Kind, Mode, QuarkId, QuarkState, Seat, SeatOverride, Team,
};

use crate::config::{self, ChamberPrefs, Identity};
use crate::model::{self, ChamberView, MessageRow, RosterRow, StatsWindow};
use crate::theme;

mod mentions;
use mentions::{parse_plan_progress, parse_plan_tasks, resolve_mention_names};

mod identity;
use identity::{
    hsla_to_hex, identity_avatar, pack_rgb, parse_hex, ResolvedIdentity, IDENTITY_SWATCHES,
};

mod tabs;
use tabs::{ChatTab, GitSubtab, InfoTab, Rail, RightRailTab};

mod providers;
use providers::{
    cli_seat_from, configured_providers, custom_cli_vendor_is_valid, migrate_legacy_ids,
    migrate_repo_to_catalogue, parse_max_exchanges, prompt_channel_from, AcpModelProbe,
    AcpModelState, AgentDescriptor, CliChannelChoice, ConfiguredQuark, ProviderState,
    SettingsTarget, WizardState, DEFAULT_SECRET_VAR,
};

mod widgets;
use widgets::{
    active_quarks, control_button, drag_region, effective_presence_state, effort_tag, empty_hint,
    fallback_pick_image, format_num, frame_corner_radii, kind_icon, kv_row, log_row,
    markdown_style, menu_button, mode_color, mode_hint, mode_label, mode_tag, next_global_mode,
    next_mode, panel_eyebrow, progress_meter, roster_row, session_card, settings_field,
    settings_field_stacked, stat_tile, text_button,
};

mod actions;
mod settings;
mod render;

mod input;
#[cfg(test)]
use input::split_leading_commands;

mod terminal;
mod reload;
mod update;
pub use update::UpdateState;

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
        ToggleFocus,
        ToggleProcessManager,
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

/// Translate the terminal screen's measured pixel size into a PTY column/row
/// grid. The single source of truth for the cell → grid conversion, used both to
/// spawn the PTY and to resize it. Floored, and never below a 2×2 usable grid.
fn term_dims((w, h): (f32, f32)) -> (usize, usize) {
    (
        ((w / TERM_CELL_W).floor() as usize).max(2),
        ((h / TERM_CELL_H).floor() as usize).max(2),
    )
}

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
    /// Where per-seat secret env-var VALUES are written/read (the Settings API-key
    /// field). Real backend: `hadron_gluon::KeyringStore` over the OS credential
    /// store, constructed once in [`Chamber::new`]. Boxed as the trait so a test
    /// can inject a `MemoryStore` instead — the chamber itself never sees which.
    secret_store: Box<dyn hadron_lattice::secrets::SecretStore>,
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
    /// [`Self::reload_archives`].
    archived_messages: Vec<MessageRow>,
    /// The archived sessions the app menu's `Sessions` submenu offers to `/resume`,
    /// newest first. Cached rather than listed on menu-open: [`crate::model::list_sessions`]
    /// reads every archive's whole `field.jsonl`, which is not something to do on the
    /// frame that paints a menu. Rebuilt alongside `archived_messages`.
    sessions: Vec<crate::model::SessionInfo>,
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
    /// Cached branches/worktrees/log-graph for the Git rail, refreshed on tab entry
    /// (like `working_diff`) rather than every render — each is its own git subprocess.
    git_branch_fingerprint: Option<String>,
    git_branches: Option<Vec<crate::vcs::BranchInfo>>,
    git_worktrees: Option<Vec<crate::vcs::WorktreeInfo>>,
    git_log_graph: Option<String>,
    git_scroll: ScrollHandle,
    git_subtab: GitSubtab,
    /// The branch whose diff-against-`main` is expanded in the Branches subtab, with
    /// its cached diff and per-file open set (like `changes_open_ixs`).
    git_selected_branch: Option<String>,
    git_branch_diff: Option<Vec<crate::vcs::FileDiff>>,
    git_branch_open_ixs: std::collections::HashSet<usize>,
    pub git_selected_commit: Option<String>,
    pub git_commit_diff: Option<Vec<crate::vcs::FileDiff>>,
    pub git_commit_open_ixs: std::collections::HashSet<usize>,
    pub git_show_snapshots: bool,
    /// The Graph subtab's rows, already parsed, snapshot-filtered and connector-collapsed,
    /// plus the lane count their rail gutter is sized from. Derived state, rebuilt by
    /// [`Chamber::rebuild_graph_rows`] only when `git_log_graph` or `git_show_snapshots`
    /// changes — GPUI re-renders on every hover, and re-parsing the whole `git log` there
    /// is what made the tab lag.
    pub(super) git_graph_rows: Vec<crate::vcs::GraphRow>,
    pub(super) git_graph_max_lanes: usize,
    /// Virtual list state for the Graph subtab: without it every commit builds an element
    /// and a lane `canvas` on every frame, so an uncapped walk would be unaffordable.
    pub(super) git_graph_list: gpui::ListState,
    /// Scroll position of the Plan tracker pane.
    plan_scroll: ScrollHandle,
    pub(super) plan_collapsed_tasks: std::collections::HashSet<String>,
    pub(super) last_plan_path: Option<String>,
    pub(super) last_incomplete_task: Option<String>,
    /// Virtual list state for the Chat tab.
    chat_list_state: gpui::ListState,
    log_list_state: gpui::ListState,
    /// Log rows (by message index) the user has clicked to expand to their full body.
    log_expanded: std::collections::HashSet<usize>,
    #[allow(dead_code)] // only read by the superseded message_row
    log_expanded_ixs: std::collections::HashSet<usize>,
    /// Maps a virtual list item index to the message's true index in `view.messages`.
    chat_message_ixs: Vec<usize>,
    /// Scroll position for each of the three tabs.
    chat_scrolls: [ScrollHandle; 4],
    /// Cache of parsed Markdown to HTML, keyed by message index, storing (raw_body, resolved_content)
    parsed_markdown: std::cell::RefCell<std::collections::HashMap<usize, (String, String)>>,
    /// A debounced window-bounds save is already in flight, so a drag (which
    /// re-renders every frame) coalesces into one write instead of one per frame.
    bounds_save_pending: bool,
    /// Whether the Process Manager overlay is showing (pinned Roster rail button,
    /// above Settings).
    process_manager_open: bool,
    /// Whether the Settings overlay is showing, and which identity it edits.
    settings_open: bool,
    settings_target: SettingsTarget,
    /// Settings editor fields (display name + image path for the current target).
    settings_name: Entity<InputState>,
    settings_path: Entity<InputState>,
    settings_model: Entity<InputState>,
    settings_effort: Entity<InputState>,
    settings_mode_config: Entity<InputState>,
    settings_roles: Entity<InputState>,
    settings_new_role: Entity<InputState>,
    settings_deny_skills: Entity<InputState>,
    settings_energy_limit: Entity<InputState>,
    /// The team-wide "Max exchanges" field on the Providers panel (not per-identity, so
    /// it's loaded/committed unconditionally in `load_settings_inputs`/
    /// `commit_settings_inputs` rather than keyed off `settings_target`). Blank = clear
    /// the repo override; see `parse_max_exchanges`.
    settings_max_exchanges: Entity<InputState>,
    /// The per-quark secret env-var **name** to declare/set/clear (e.g.
    /// `"GEMINI_API_KEY"`), defaulted from the seat's existing `secret_env` (or
    /// [`providers::DEFAULT_SECRET_VAR`] when none is declared yet).
    settings_secret_var: Entity<InputState>,
    /// The masked secret **value** input — write-only: never populated from the
    /// store, always blank on load and cleared again after Set/Clear so the
    /// stored value is never rendered back into the UI.
    settings_secret_value: Entity<InputState>,
    /// Cached set / not-set / keychain-unavailable status for `settings_secret_var`,
    /// refreshed on load/Set/Clear (not read every render — a keychain lookup per frame
    /// would hammer the OS credential store, e.g. a D-Bus round trip to Secret Service).
    settings_secret_status: settings::SecretStatus,
    /// Whether the current Settings quark's provider needs a secret key at all —
    /// gates the API-key field so it isn't shown under every quark. Set on load.
    settings_secret_applies: bool,
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
    _lock_file: Option<std::fs::File>,
    git_statuses: std::collections::HashMap<String, crate::vcs::GitStatus>,
    completion_files: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    pub(super) update_state: UpdateState,
    file_tree_open: Option<(String, String)>,
    file_tree_expanded: std::collections::HashSet<String>,
    terminal: Option<crate::pty::PtyTerminal>,
    terminal_error: Option<String>,
    /// Keyboard focus for the terminal grid — keystrokes flow to the PTY only
    /// while this holds focus.
    terminal_focus: FocusHandle,
    /// The terminal screen's measured pixel size, written by a paint-time canvas
    /// probe and read by the pump loop to size the PTY to fit.
    /// The terminal grid's painted screen rect in window pixels: `(x, y, w, h)`.
    /// Width/height size the PTY; the origin maps a pointer position to a cell.
    terminal_px: std::rc::Rc<std::cell::Cell<Option<(f32, f32, f32, f32)>>>,
    /// Pump ticks left in which to force a repaint so the paint-time size probe
    /// re-measures. The measured size only refreshes on paint, and an idle
    /// terminal forces none — so without this the PTY stays stuck at whatever
    /// width the first (often transient-narrow) frame measured, leaving the shell
    /// prompt wrapped until the user types. Re-armed whenever the size moves; runs
    /// down to zero once it settles, then the pump goes quiet again.
    terminal_warmup: u8,
    info_panel: Option<String>,
    /// The About dialog, opened from the app menu.
    about_open: bool,
    file_tree_scroll: ScrollHandle,
    file_tree_open_scroll: ScrollHandle,
    completion_scroll: ScrollHandle,
    pub(super) last_live_activities: std::collections::HashMap<String, Option<hadron_lattice::live::Activity>>,
    /// The `gluon.lock` flock reading from the previous poll — compared against the
    /// fresh reading each tick so a toast fires only on the running→stopped edge,
    /// not on every tick gluon happens to still be down. Starts optimistic (running)
    /// since the chamber normally auto-spawns the daemon on launch.
    pub(super) last_gluon_running: bool,
    /// Whether the "gluon stopped" banner is currently shown. Set on the
    /// running→stopped edge, cleared on stopped→running or manual dismiss.
    /// Whether `.hadron/nucleus/index.md` exceeds the 32 KiB prompt limit
    pub(super) nucleus_over_budget: bool,
    pub(super) gluon_stopped_notice: bool,
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
        lock_file: Option<std::fs::File>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let repo_root = crate::vcs::repo_root_of(&path).to_path_buf();
        let files = crate::sys::list_workspace_files(&repo_root, &std::collections::HashSet::new());
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
                .auto_grow(1, 12)
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
        let settings_roles = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. architect, reviewer"));
        let settings_new_role = cx.new(|cx| InputState::new(window, cx).placeholder("Add custom role..."));
        let settings_deny_skills = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. writing-plans, brainstorming"));
        let settings_energy_limit = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. 500000 (blank = default)"));
        let settings_max_exchanges = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. 50 (blank = daemon default)"));
        let settings_secret_var = cx.new(|cx| InputState::new(window, cx).placeholder(DEFAULT_SECRET_VAR));
        // `.masked(true)`: a password field — the stored value is never rendered back
        // into this input, on load or after Set/Clear (see `load_settings_inputs`).
        let settings_secret_value =
            cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("value (write-only)"));
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

            let state = match this.update(cx, |chamber, _| {
                (
                    chamber.path.clone(),
                    chamber.file_tree_expanded.clone(),
                    chamber.right_rail_tab,
                )
            }) {
                Ok(s) => s,
                Err(_) => break,
            };

            let scan = cx
                .background_executor()
                .spawn(async move {
                    let root = crate::vcs::repo_root_of(&state.0);
                    reload::WorkspaceScan::gather(
                        root,
                        &state.1,
                        state.2 == RightRailTab::Changes,
                        state.2 == RightRailTab::Git,
                    )
                })
                .await;

            if this
                .update(cx, |chamber, cx| chamber.reload_if_changed(scan, cx))
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
        let chat_message_ixs = model::chat_message_indices(&view.messages);

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

        // The commit graph reads newest-first, so it anchors at the top — unlike the
        // chat/log lists, which follow the newest message at the bottom.
        // `measure_all` so the scrollbar thumb is honest: `ListState`'s content height
        // is the sum over *measured* items, and unmeasured ones count as zero — with
        // 2000+ rows and ~40 on screen the thumb would claim the list barely scrolls.
        // It measures once per `reset` (i.e. per `rebuild_graph_rows`), not per splice
        // or per frame, so the cost lands on Git-tab entry and nowhere else.
        let git_graph_list = gpui::ListState::new(0, gpui::ListAlignment::Top, px(400.))
            .measure_all();

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
        // Resolved before `team` moves into the struct literal below.
        let nucleus_index_budget_bytes = hadron_gluon::nucleus_status::resolve_budget_bytes(&team);

        // The real secret backend: the OS credential store, via the same
        // `KeyringStore` the daemon uses (`hadron_gluon::secrets`) — same service
        // name and account format, so a value set here in the chamber and a value
        // the daemon resolves at spawn are the SAME keychain entry.
        let secret_store: Box<dyn hadron_lattice::secrets::SecretStore> =
            Box::new(hadron_gluon::KeyringStore::new());

        // Load the archived sessions once — the history the wider Stats windows fold in,
        // and the rows the app menu's `Sessions` submenu offers. Rebuilt by
        // `Self::reload_archives` after `/clear` or `/resume` writes a new archive.
        let sessions_dir = path.parent().map(|p| p.join("sessions")).unwrap_or_default();
        let archived_messages = crate::model::load_archived_messages(&sessions_dir);
        let sessions = crate::model::list_sessions(&sessions_dir);

        let mut chamber = Chamber {
            view,
            prefs,
            team,
            global,
            path,
            secret_store,
            input,
            completion: None,
            focus_handle,
            chat_tab: ChatTab::Chat,
            info_tab: InfoTab::Identity,
            stats_window: StatsWindow::Current,
            archived_messages,
            sessions,
            right_rail_tab: RightRailTab::Terminal,
            selected_quark_ix: None,
            app_menu_open: false,
            working_diff: None,
            changes_open_ixs: std::collections::HashSet::new(),
            changes_scroll: ScrollHandle::new(),
            git_branch_fingerprint: None,
            git_branches: None,
            git_worktrees: None,
            git_log_graph: None,
            git_scroll: ScrollHandle::new(),
            git_subtab: GitSubtab::Branches,
            git_selected_branch: None,
            git_branch_diff: None,
            git_branch_open_ixs: Default::default(),
            git_selected_commit: None,
            git_commit_diff: None,
            git_commit_open_ixs: std::collections::HashSet::new(),
            git_show_snapshots: false,
            git_graph_rows: Vec::new(),
            git_graph_max_lanes: 1,
            git_graph_list,
            plan_scroll: ScrollHandle::new(),
            plan_collapsed_tasks: std::collections::HashSet::new(),
            last_plan_path: None,
            last_incomplete_task: None,
            chat_list_state,
            log_list_state,
            log_expanded: std::collections::HashSet::new(),
            log_expanded_ixs: std::collections::HashSet::new(),
            chat_message_ixs,
            chat_scrolls,
            parsed_markdown: std::cell::RefCell::new(std::collections::HashMap::new()),
            bounds_save_pending: false,
            process_manager_open: false,
            settings_open: false,
            settings_target: SettingsTarget::Human,
            settings_name,
            settings_path,
            settings_model,
            settings_effort,
            settings_mode_config,
            settings_roles,
            settings_new_role,
            settings_deny_skills,
            settings_energy_limit,
            settings_max_exchanges,
            settings_secret_var,
            settings_secret_value,
            settings_secret_status: settings::SecretStatus::NotSet,
            settings_secret_applies: false,
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
            _lock_file: lock_file,
            git_statuses: crate::vcs::get_git_statuses(&repo_root),
            completion_files,
            file_tree_open: None,
            file_tree_expanded: std::collections::HashSet::new(),
            terminal: None,
            terminal_error: None,
            terminal_focus: cx.focus_handle(),
            terminal_px: std::rc::Rc::new(std::cell::Cell::new(None)),
            terminal_warmup: 0,
            info_panel: None,
            about_open: false,
            file_tree_scroll: ScrollHandle::new(),
            file_tree_open_scroll: ScrollHandle::new(),
            completion_scroll: ScrollHandle::new(),
            last_live_activities: std::collections::HashMap::new(),
            last_gluon_running: true,
            nucleus_over_budget: hadron_gluon::nucleus_status::index_over_budget(
                &repo_root,
                nucleus_index_budget_bytes,
            ),
            gluon_stopped_notice: false,
            update_state: UpdateState::default(),
        };
        chamber.update_active_plan();
        chamber.check_for_updates(cx);
        chamber
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

/// Every chord the chamber binds at startup. A free function (not an inline
/// array inside `run`) so a test can assert a chord is actually bound without
/// opening a window.
fn default_key_bindings() -> Vec<KeyBinding> {
    vec![
        // Verified-free (was shift-tab, dead while typing — see above).
        KeyBinding::new("f6", CycleMode, Some(KEY_CONTEXT)),
        // Chat column tabs (Chat / Log / Stats).
        KeyBinding::new("alt-right", NextChatTab, None),
        KeyBinding::new("alt-left", PrevChatTab, None),
        // Right rail tabs (Terminal / Files / Changes / Plan).
        KeyBinding::new("alt-pagedown", NextInspectorTab, None),
        KeyBinding::new("alt-pageup", PrevInspectorTab, None),
        // Stats time window (Session / Week / Month / All time).
        KeyBinding::new("alt-down", NextStatsSubTab, None),
        KeyBinding::new("alt-up", PrevStatsSubTab, None),
        // Roster cursor (vim-style j/k) and open-selected.
        KeyBinding::new("ctrl-j", NextQuark, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-k", PrevQuark, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-alt-enter", ToggleSelectedQuark, Some(KEY_CONTEXT)),
        // App menu overlay — F10 (the conventional "focus the menu" key) plus
        // `ctrl-m` as a chord that reaches the menu without leaving the home row.
        // Both are scoped: the text input's key context claims neither.
        KeyBinding::new("f10", OpenMenu, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-m", OpenMenu, Some(KEY_CONTEXT)),
        // Chat input <-> terminal focus toggle (`ctrl-tab` / `ctrl-``).
        // Global so it fires from either side.
        KeyBinding::new("ctrl-tab", ToggleFocus, None),
        KeyBinding::new("ctrl-`", ToggleFocus, None),
    ]
}

/// The first UI font family whose **bold actually renders**.
///
/// GPUI has no CSS font-stack parsing. `Theme::font_family` is ONE literal family name,
/// matched exactly against the platform font database (`cosmic_text_system.rs`'s
/// `load_family` compares `*name == family.0`), so gpui-component's non-macOS default —
/// `"Inter, Segoe UI, DejaVu Sans, Liberation Sans, sans-serif"` — matches nothing at all.
///
/// The miss is **silent, and it eats bold specifically**: `TextSystem::resolve_font` falls
/// through to its own hardcoded fallback stack, and every entry in that stack is built with
/// `font(family)`, i.e. `FontWeight::default()`. Bold and regular then resolve to the SAME
/// regular face, so `**bold**` renders flat while everything else looks fine. Three fixes
/// were shipped against this symptom (`3001c67`, `59479e8`) by swapping one unresolvable
/// name for another — `.SystemUIFont` maps to "IBM Plex Sans" on Linux, which is not
/// installed on this box either.
///
/// `TextSystem::font_id` — the one that reports the miss — is private, so the probe uses
/// the observable consequence instead: resolve the family at bold and at regular and
/// compare the two `FontId`s. Different ids mean the family resolved AND has a real bold
/// face. Equal ids mean either the family missed (both landed on the same fallback) or it
/// has no bold face — both render flat, so both are rejected. This is the property we
/// actually care about, which is why it is checked rather than "is the name installed".
fn font_family_with_a_real_bold(cx: &App) -> SharedString {
    // Platform-typical UI faces first, then the ones a Linux desktop nearly always ships.
    const CANDIDATES: [&str; 7] = [
        ".SystemUIFont", // the real system face on macOS/Windows; "IBM Plex Sans" on Linux
        "Inter",
        "Segoe UI",
        "Ubuntu",
        "Cantarell",
        "Noto Sans",
        "DejaVu Sans",
    ];
    let text_system = cx.text_system();
    CANDIDATES
        .into_iter()
        .find(|name| {
            let regular = gpui::font(*name);
            text_system.resolve_font(&regular.clone().bold()) != text_system.resolve_font(&regular)
        })
        .map(SharedString::from)
        // Nothing here has a distinguishable bold. Leave gpui to its own fallback rather
        // than pinning a family we just proved does not work.
        .unwrap_or_else(|| ".SystemUIFont".into())
}

/// Launch the chamber window against a field file path.
pub fn run(field_path: Option<String>, chamber_lock_file: Option<std::fs::File>) {
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
        // A `file://` link in a chat message is a SOURCE FILE, not a document for the
        // desktop to guess about: without this it reaches `xdg-open` and lands in
        // whatever the system association says (Vim, here). Declining anything that
        // is not a local file keeps `https://` links going to the browser.
        //
        // The choice is re-read from disk per click rather than captured here: the
        // Settings picker writes `chamber.json` on click, so reading it at click time
        // keeps ONE home for the setting instead of a second cached copy that could
        // drift. A click is rare and the file is tiny.
        gpui_component::text::set_link_handler(|url| {
            let Some((path, line)) = crate::sys::file_url_target(url) else {
                return false;
            };
            match crate::sys::editor_argv(&config::load().editor, &path, line) {
                Some((program, args)) => std::process::Command::new(&program).args(&args).spawn().is_ok(),
                // `System` (or a blank custom command) has no opinion — let the
                // platform opener have it, which is the pre-existing behaviour.
                None => false,
            }
        });
        Theme::change(ThemeMode::Dark, None, cx);
        // Probed before the theme block below, which holds `cx` mutably. Logged because
        // this bug is invisible from inside the app — the only symptom is flat bold.
        let ui_font = font_family_with_a_real_bold(cx);
        eprintln!("chamber: UI font family {ui_font} (bold verified)");
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
            t.tokens.popover = gpui::Hsla::from(theme::popover()).into();
            // `border` is ONE shared token: every popup menu, dialog, tab, table and
            // input outline reads it, not just the chat/right-rail resize handle. It was
            // previously zeroed out here to hide that one handle at rest, which silently
            // killed borders everywhere else too — including context-menu edges, which
            // is why they read as bleeding into the field with no outline. Fixed at the
            // source instead: `resize_handle.rs` (the fork) now paints its own idle
            // handle transparent directly rather than reading this token, so `border`
            // can go back to a real, visible value for everyone else.
            t.border = theme::border().into();
            t.drag_border = rgb(0xec4899).into();
            // Stock dark theme's `input`/`selection`/`list_active` are deep blues
            // (`#1d4ed8`-family) that never got re-themed, which is the "blue tint" in
            // the Settings/Processes/File-Tree surfaces — `input` in particular sat only
            // two hex steps from `modal_surface()`, so a Settings text field was nearly
            // invisible against its own card. Re-anchor all three to the chamber's own
            // neutral/pink ramp so nothing on these panels still carries the fork's
            // stock accent.
            t.input = theme::border().into();
            t.selection = gpui::rgba(0xec489966).into();
            t.list_active = gpui::rgba(0xec489933).into();
            t.list_active_border = theme::accent().into();
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
            // Chat markdown links — light blue instead of the fork's stock
            // near-white, so a link reads as a link against chat prose.
            t.link = theme::link().into();
            t.link_hover = theme::link_hover().into();
            t.link_active = theme::link_active().into();
            // Scrollbar thumb theme tokens — frosted white over dark background
            // so scrollbars are clearly visible on hover or scrolling.
            t.scrollbar_show = ScrollbarShow::Hover;
            t.scrollbar = gpui::rgba(0x00000000).into();
            t.scrollbar_thumb = gpui::rgba(0xffffff40).into();
            t.scrollbar_thumb_hover = gpui::rgba(0xffffffa0).into();
            t.tokens.scrollbar_thumb = gpui::Hsla::from(gpui::rgba(0xffffff40)).into();
            t.tokens.scrollbar_thumb_hover = gpui::Hsla::from(gpui::rgba(0xffffffa0)).into();
            // Subtle dark window frame (Zed-style CSD border), matching the UI.
            t.window_border = rgb(0x2a2b2c).into();
            // Root paints this behind everything; transparent so our rounded
            // window frame (crate::window_frame) shows the shadow through the
            // corners instead of a square fill.
            t.tokens.background = gpui::hsla(0.0, 0.0, 0.0, 0.0).into();
            // ONE family, never a comma list — see `font_family_with_a_real_bold`.
            t.font_family = ui_font;
        }
        // Keyboard navigation. The chords still scoped to `KEY_CONTEXT` are ones
        // the text input's own key context (`gpui_component::input::state::CONTEXT`)
        // does NOT claim, so they fall through to this Chamber context even while
        // the chat box has focus. (That claim used to be taken on faith for
        // `shift-tab`: Input binds `shift-tab` for `OutdentInline`, so `CycleMode`
        // was silently dead while typing — hence `f6`. Every scoped chord below has
        // been grepped against `crates/gpui-component/crates/ui/src/input/state.rs`
        // and confirmed unclaimed.)
        //
        // The navigation chords are bound with `None` (global) so they dispatch
        // regardless of which sub-context holds focus — `content` (which carries
        // `KEY_CONTEXT`) is always their ancestor, so a global binding still reaches
        // its handler. Jake's requested scheme is modifier + arrows:
        //   ctrl-tab           chat input  <-> terminal (reuses `ToggleFocus`)
        //   alt-left/right     chat column tabs   (Chat / Log / Stats)
        //   alt-pageup/pagedn  right-rail tabs     (Terminal / Files / Changes / Plan)
        //   alt-up/down        Stats time window
        cx.bind_keys(default_key_bindings());

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
                        chamber_lock_file,
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

    /// `self.reproject(` is the raw, un-resynced path: it leaves `chat_list_state`,
    /// `log_list_state` and `chat_message_ixs` exactly as they were, which is what let
    /// `/rename`/`/reboot`/`/approve`/`/deny`/`answer_permission` under-count the Log tab
    /// (see `sync-view-log-list-state-ssot`). `Chamber::sync_view` is the one place
    /// allowed to call it; everything else must call `sync_view` instead. A hand-written
    /// allowlist of "already fixed" call sites would not catch a *new* one added later —
    /// exactly the class of bug this guards — so this scans the real source text of
    /// every file that used to hold a bare call, via `include_str!` (this file, `mod.rs`,
    /// is deliberately NOT one of them, so the search needle below can't match itself).
    #[test]
    fn every_reproject_call_goes_through_sync_view() {
        let files: &[(&str, &str)] = &[
            ("actions.rs", include_str!("actions.rs")),
            ("reload.rs", include_str!("reload.rs")),
            ("input.rs", include_str!("input.rs")),
        ];
        let needle = ["self", ".reproject("].concat();
        let total: usize = files.iter().map(|(_, text)| text.matches(&needle).count()).sum();
        assert_eq!(
            total, 1,
            "expected exactly one `self.reproject(` call (inside Chamber::sync_view) \
             across actions.rs/reload.rs/input.rs, found {total} — a new or migrated call \
             site must go through `sync_view` instead"
        );
    }

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

        // reboot swallows the rest of the line as its argument.
        let (cmds, body) = split_leading_commands("/reboot @acp-claude");
        assert_eq!(cmds, vec![("reboot".to_string(), "@acp-claude".to_string())]);
        assert_eq!(body, None);

        // approve swallows the rest of the line as its argument.
        let (cmds, body) = split_leading_commands("/approve @acp-claude remember");
        assert_eq!(cmds, vec![("approve".to_string(), "@acp-claude remember".to_string())]);
        assert_eq!(body, None);

        // toggle swallows the rest of the line as its argument — and must NOT be
        // shadowed by `/toggle-roster`, which shares its prefix. A `/toggle` that the
        // parser did not know would be swept into `body` and POSTED as a chat message,
        // silently, however good the handler arm is.
        let (cmds, body) = split_leading_commands("/toggle @Sonnet");
        assert_eq!(cmds, vec![("toggle".to_string(), "@Sonnet".to_string())]);
        assert_eq!(body, None);
        let (cmds, body) = split_leading_commands("/toggle-roster hello");
        assert_eq!(cmds, vec![("toggle-roster".to_string(), String::new())]);
        assert_eq!(body.as_deref(), Some("hello"), "the longer name still wins its own arm");

        // deny swallows the rest of the line as its argument.
        let (cmds, body) = split_leading_commands("/deny @acp-claude");
        assert_eq!(cmds, vec![("deny".to_string(), "@acp-claude".to_string())]);
        assert_eq!(body, None);

        // limit swallows the rest of the line as its argument.
        let (cmds, body) = split_leading_commands("/limit @acp-claude 1000000");
        assert_eq!(cmds, vec![("limit".to_string(), "@acp-claude 1000000".to_string())]);
        assert_eq!(body, None);

        // reset-energy swallows the rest of the line as its argument.
        let (cmds, body) = split_leading_commands("/reset-energy @Claude");
        assert_eq!(cmds, vec![("reset-energy".to_string(), "@Claude".to_string())]);
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

    #[test]
    fn the_app_menu_answers_to_both_f10_and_ctrl_m() {
        // `KeyBinding` exposes neither its keystrokes nor its action, so Debug is
        // the only handle a headless test has on what was actually bound.
        let bound: Vec<String> = super::default_key_bindings()
            .iter()
            .map(|b| format!("{:?}", b))
            .filter(|b| b.contains("OpenMenu"))
            .collect();
        // Debug prints the *parsed* keystroke (`control: true`, `key: "m"`), never
        // the source string "ctrl-m" — asserting on the literal chord passes only
        // by accident of it never running.
        assert_eq!(bound.len(), 2, "expected exactly two OpenMenu chords: {bound:?}");
        assert!(
            bound.iter().any(|b| b.contains(r#"key: "f10""#)),
            "f10 lost: {bound:?}"
        );
        assert!(
            bound
                .iter()
                .any(|b| b.contains("control: true") && b.contains(r#"key: "m""#)),
            "ctrl-m missing: {bound:?}"
        );
    }
}
