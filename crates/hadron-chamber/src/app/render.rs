//! The chamber's view layer: the `impl Render for Chamber` entry point plus every
//! panel/overlay/row builder it composes (titlebar, body, rails, roster, chat, log,
//! timeline, stats, terminal, toasts, About/menu overlays, mode picker, and the
//! markdown/message rows). These borrow `&self`/`&mut self` and paint from live state.

use super::*;

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
    pub(super) fn info_panel_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            hadron_lattice::Transport::Sdk => "reserved (not yet implemented)".to_string(),
        };
        let model_str = if roster_row.model.is_empty() {
            "—".to_string()
        } else {
            roster_row.model.clone()
        };
        let transport_str = match roster_row.transport {
            hadron_lattice::Transport::Cli => "CLI (one-shot)",
            hadron_lattice::Transport::Acp => "ACP (resident)",
            hadron_lattice::Transport::Sdk => "SDK (reserved)",
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

    /// The completion card: rows floating just above the message box, spanning the
    /// input's full width. It is a normal render-tree descendant — `.absolute()`
    /// with `.bottom(100%)` inside the input area's `.relative()` wrapper — so it
    /// draws *upward* and stays inside the window, unlike the fork's `deferred()`
    /// menu that painted off the bottom edge (`completion-menu-draws-out-of-bounds`).
    pub(super) fn completion_card_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(super) fn titlebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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

    /// A collapsed rail: a fixed vertical strip with just the expand affordance
    /// (and, on the Quarks rail, the pinned Settings button).
    pub(super) fn rail_strip(&self, rail: Rail, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn settings_button(&self, cx: &mut Context<Self>, icon_only: bool) -> impl IntoElement {
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

    pub(super) fn roster_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn chat_pane(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn chat_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn log_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn timeline_view(&self) -> impl IntoElement {
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
    pub(super) fn stats_window_tabs(&self, id: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn stats_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn terminal_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn permission_toast(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
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

    /// The About dialog. Every value here is read from the build, not typed in: the
    /// version comes from the crate's own manifest, so it cannot drift from what
    /// shipped.
    pub(super) fn about_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn mode_select(&self, id: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
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
    pub(super) fn app_menu_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn markdown_body(
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

    pub(super) fn chat_message_row(
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

    pub(super) fn message_row(
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
