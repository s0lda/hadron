//! The chamber's view layer: the `impl Render for Chamber` entry point plus every
//! panel/overlay/row builder it composes (titlebar, body, rails, roster, chat, log,
//! timeline, stats, terminal, toasts, About/menu overlays, mode picker, and the
//! markdown/message rows). These borrow `&self`/`&mut self` and paint from live state.

use super::*;

mod titlebar;
mod roster;
// `pub(crate)` only so `widgets::task_row` can reuse `chat::format_duration` rather
// than growing a second copy of it.
pub(crate) mod chat;
mod terminal;
mod git;
mod stats;
mod overlays;
mod visualizer;
mod breadcrumb;
mod repl_overlay;
mod dag_visualizer;
mod attention_hud;

impl Render for Chamber {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame_start =
            std::env::var_os("HADRON_FRAME_TIMING").is_some().then(std::time::Instant::now);
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
        let body = self.body(window, cx);
        let settings = self.settings_open.then(|| self.settings_overlay(window, cx));
        let info = self
            .info_panel
            .is_some()
            .then(|| self.info_panel_overlay(cx));
        let about = self.about_open.then(|| self.about_overlay(cx));
        let changelog = self.changelog_open.then(|| self.changelog_overlay(cx));
        let app_menu = self.app_menu_open.then(|| self.app_menu_overlay(cx));
        let processes = self
            .process_manager_open
            .then(|| self.process_overlay(cx));
        let toasts = self.render_toasts(cx);
        let repl = self.repl_overlay_open.then(|| self.repl_overlay(window, cx));

        let content = v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ToggleReplOverlay, window, cx| this.toggle_repl_overlay(window, cx)))
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
            .on_action(cx.listener(|this, _: &ToggleFocus, window, cx| this.toggle_focus(window, cx)))
            .on_action(cx.listener(|this, _: &FocusChat, window, cx| this.focus_chat_input(window, cx)))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.handle_escape_dismiss(window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleProcessManager, _, cx| this.toggle_process_manager(cx)),
            )
            .on_action(cx.listener(|this, _: &NewTerminalTab, _, cx| this.add_terminal(cx)))
            .on_action(cx.listener(|this, _: &CloseTerminalTab, _, cx| {
                let active = this.active_terminal_index;
                this.close_terminal(active, cx);
            }))
            .on_action(cx.listener(|this, _: &NextTerminalTab, _, cx| this.next_terminal_tab(cx)))
            .on_action(cx.listener(|this, _: &PrevTerminalTab, _, cx| this.prev_terminal_tab(cx)))
            .on_action(cx.listener(|this, _: &NextGitSubtab, _, cx| this.cycle_git_subtab(1, cx)))
            .on_action(cx.listener(|this, _: &PrevGitSubtab, _, cx| this.cycle_git_subtab(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectGitBranches, _, cx| this.select_git_subtab(GitSubtab::Branches, cx)))
            .on_action(cx.listener(|this, _: &SelectGitWorktrees, _, cx| this.select_git_subtab(GitSubtab::Worktrees, cx)))
            .on_action(cx.listener(|this, _: &SelectGitGraph, _, cx| this.select_git_subtab(GitSubtab::Graph, cx)))
            .on_action(cx.listener(|this, _: &SelectGitDelegation, _, cx| this.select_git_subtab(GitSubtab::Delegation, cx)))
            .on_action(cx.listener(|this, _: &NextGitItem, _, cx| this.move_git_selection(1, cx)))
            .on_action(cx.listener(|this, _: &PrevGitItem, _, cx| this.move_git_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &OpenGitItem, _, cx| this.open_git_selection(cx)))
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

            .child(titlebar)
            .child(body)
            .children(settings)
            .children(info)
            .children(about)
            .children(changelog)
            .children(app_menu)
            .children(processes)
            .children(repl)
            .children(toasts);

        let wrapped_content = crate::window_frame::window_frame(window, cx, content);

        let res = div().size_full().child(wrapped_content).into_any_element();
        if let Some(start) = frame_start {
            hadron_lattice::term::info(
                hadron_lattice::term::Source::Chamber,
                &format!("frame render total: {:?}", start.elapsed()),
            );
        }
        res
    }
}

impl Chamber {
    /// The body: the left roster ("friends list") at a locked width, then the
    /// resizable chat + terminal group. The roster sits *outside* the group so
    /// dragging the terminal never disturbs it — only the terminal is draggable,
    /// and the chat flexes to fill whatever's left (so a window resize reflows
    /// into the chat instead of stranding a stored width). A collapsed rail is a
    /// thin strip. The group is re-keyed on the terminal's presence so a fresh
    /// sizing state seeds its width from prefs; `on_resize` persists it back.
    pub(super) fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .child(self.chat_pane(window, cx)),
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
}
