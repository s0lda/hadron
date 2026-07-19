//! The chamber's view layer: the `impl Render for Chamber` entry point plus every
//! panel/overlay/row builder it composes (titlebar, body, rails, roster, chat, log,
//! timeline, stats, terminal, toasts, About/menu overlays, mode picker, and the
//! markdown/message rows). These borrow `&self`/`&mut self` and paint from live state.

use super::*;

mod titlebar;
mod roster;
mod chat;
mod terminal;
mod stats;
mod overlays;

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
            .on_action(cx.listener(|this, _: &ToggleFocus, window, cx| this.toggle_focus(window, cx)))
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
                    // The background photo (Jake's asset), replacing the two-colour
                    // gradient wash. Baked absolute at compile time so it resolves
                    // regardless of the runtime cwd; loaded via `Resource::Path`
                    // (fs::read, decoded once and cached by gpui), so it costs a
                    // texture composite per repaint, not a re-decode. `Cover` fills
                    // the field, cropping overflow; rounded to the housing radius so
                    // the corners sit under the window's rounded frame.
                    .child(
                        gpui::img(std::path::PathBuf::from(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/../../assets/background.jpeg"
                        )))
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover)
                        .rounded_tl(top_radius)
                        .rounded_tr(top_radius)
                        .rounded_bl(bottom_radius)
                        .rounded_br(bottom_radius),
                    )
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
    /// The body: the left roster ("friends list") at a locked width, then the
    /// resizable chat + terminal group. The roster sits *outside* the group so
    /// dragging the terminal never disturbs it — only the terminal is draggable,
    /// and the chat flexes to fill whatever's left (so a window resize reflows
    /// into the chat instead of stranding a stored width). A collapsed rail is a
    /// thin strip. The group is re-keyed on the terminal's presence so a fresh
    /// sizing state seeds its width from prefs; `on_resize` persists it back.
    pub(super) fn body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
