use super::*;

impl super::Chamber {
    /// The most recent context reading published by `qid` in the live field: the last
    /// message that actually carries `usage.context`, not merely the last message. A
    /// quark's trailing rows are usually turn-less status pings with no usage, so a plain
    /// `rfind(last row)` lands on one of those and reports "no context" even when a
    /// reading exists earlier this window — which is why the Current gauge showed for
    /// some quarks and not others.
    fn latest_context(&self, qid: &str) -> Option<&hadron_lattice::ContextUsage> {
        latest_context(&self.view.messages, qid)
    }

    /// The most recent quota buckets `qid` published in the live field. Like
    /// [`Self::latest_context`], this is a live gauge, not a window-summed quantity —
    /// a bucket only changes when the provider sends a fresh reading, which may predate
    /// the window's cutoff (in particular [`StatsWindow::Current`]'s "since the last
    /// human message" truncation), so it reads identically regardless of which stats
    /// window tab is selected rather than going missing on some of them.
    fn latest_quota(&self, qid: &str) -> Vec<hadron_lattice::QuotaBucket> {
        latest_quota(&self.view.messages, qid)
    }

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
                .unwrap_or_else(|| format!("default ({})", roster_row.vendor)),
            hadron_lattice::Transport::Cli => "hadron-adapter".to_string(),
            hadron_lattice::Transport::Sdk => "unsupported — use ACP or CLI".to_string(),
        };
        let model_str = if roster_row.model.is_empty() {
            "—".to_string()
        } else {
            roster_row.model.clone()
        };
        let transport_str = match roster_row.transport {
            hadron_lattice::Transport::Cli => "CLI (one-shot)",
            hadron_lattice::Transport::Acp => "ACP (resident)",
            hadron_lattice::Transport::Sdk => "SDK (unsupported)",
        };

        // Presence: a live (adopted + enabled) quark shows its state colour, overridden
        // to Excited while it has fresh live activity — matching the roster row and rail
        // strip so the dot never disagrees with what the quark is actually doing.
        let live_dir = hadron_lattice::live::live_dir(&self.path);
        let activity = hadron_lattice::live::read(
            &live_dir,
            &hadron_lattice::QuarkId::new(&qid),
            chrono::Utc::now(),
        );
        let effective_state = effective_presence_state(
            roster_row.state,
            roster_row.adopted,
            roster_row.enabled,
            activity.is_some(),
        );
        let live = roster_row.adopted && roster_row.enabled;
        let (dot_color, presence_txt) = if live {
            (
                theme::presence(effective_state),
                theme::presence_label(effective_state).to_string(),
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
            .child(kv_row("Vendor", roster_row.vendor.clone()))
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

        if self.stats_window == StatsWindow::Current {
            if let Some(ctx) = self.latest_context(&qid) {
                stats_block = stats_block.child(kv_row(
                    "Context",
                    format!(
                        "{:.1}% ({} / {})",
                        ctx.used_percentage,
                        format_num(ctx.used_tokens),
                        format_num(ctx.context_window_size)
                    ),
                ));
                // Context occupancy is NOT a fixed proportion — it rises and falls as the
                // window fills and then compacts, so its trajectory (the "back and forth")
                // is the interesting part, which a single round meter can't show. Draw the
                // history as a line when we have one; fall back to a meter for a lone reading.
                let history = context_history(&self.view.messages, &qid);
                if history.len() >= 2 {
                    let points: Vec<(usize, f64)> = history.into_iter().enumerate().collect();
                    stats_block = stats_block.child(
                        div().h(px(96.0)).w_full().mt_1().child(
                            AreaChart::new(points)
                                .id(format!("info-context-chart-{qid}"))
                                .name("Context %")
                                .x(|d| format!("{}", d.0))
                                .y(|d| d.1)
                                .stroke(q_color)
                                .fill(linear_gradient(
                                    0.0,
                                    linear_color_stop(q_color.opacity(0.35), 1.0),
                                    linear_color_stop(q_color.opacity(0.02), 0.0),
                                ))
                                .linear(),
                        ),
                    );
                } else {
                    let frac = (ctx.used_percentage as f32 / 100.0).clamp(0.0, 1.0);
                    stats_block =
                        stats_block.child(div().mt_1().child(progress_meter(frac, q_color)));
                }
            }
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
                        // Straight segments between turns — the natural spline rounded the
                        // real values into blobs ("why is it round?"). Show the true shape.
                        .linear(),
                ),
            );
        }
        for bucket in self.latest_quota(&qid) {
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
            .bg(theme::field_base())
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
                    // Flat #101010 field colour — opaque, so the bright field can't
                    // bleed through, and matches the Quark Info panel to the solid
                    // background (Jake's request).
                    .bg(theme::field_base())
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

    /// The Timeline tab: a vertical [`Stepper`] over the run's milestones — the
    /// non-message activity (status changes, edits, commands, snapshots), most
    /// recent marked as the current step.
    #[allow(dead_code)] // built but not wired into the stats tab
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
        h_flex().child(
            TabBar::new(id)
                .segmented()
                .bg(theme::field_base())
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
                        .unwrap_or(StatsWindow::Current);
                    cx.notify();
                }))
        )
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
            let resolved = self.resolve_identity(q);
            let mut block = session_card().child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_baseline()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(q_color)
                                    .child(resolved.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(format!("({q})")),
                            ),
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

            if self.stats_window == StatsWindow::Current {
                if let Some(ctx) = self.latest_context(q) {
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
            }
            // An empty quota list means the provider has no quota concept — not that the
            // quota is spent. Say nothing rather than render a zero. Read live (unwindowed,
            // like `latest_context`) rather than off the windowed fold `s.quota`, so it
            // does not go missing on windows whose cutoff excludes the last quota report
            // (Current's "since the last human message" truncation, in particular).
            for bucket in self.latest_quota(q) {
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
}

/// The most recent context reading published by `qid`: the last message that actually
/// carries `usage.context`, not merely the last message from that quark. Context is
/// attached to a turn's rows, but a quark's trailing rows are usually turn-less status
/// pings with no usage — so scanning for the last *row* (rather than the last row with
/// context) reports "no context" whenever a status ping trails the turn, which is why
/// the Current gauge appeared for some quarks and not others.
fn latest_context<'a>(
    messages: &'a [super::super::MessageRow],
    qid: &str,
) -> Option<&'a hadron_lattice::ContextUsage> {
    messages
        .iter()
        .rev()
        .filter(|m| m.from == qid)
        .find_map(|m| m.usage.as_ref()?.context.as_ref())
}

/// The most recent quota buckets `qid` published: the last message from `qid` that
/// actually carries `usage.quota`, mirroring [`latest_context`]'s "skip the trailing
/// status-less rows" search. Empty when the quark never reported quota this field.
fn latest_quota(messages: &[super::super::MessageRow], qid: &str) -> Vec<hadron_lattice::QuotaBucket> {
    messages
        .iter()
        .rev()
        .filter(|m| m.from == qid)
        .find_map(|m| {
            let quota = &m.usage.as_ref()?.quota;
            (!quota.is_empty()).then(|| quota.clone())
        })
        .unwrap_or_default()
}

/// Every context-occupancy reading `qid` published this field, oldest first — the series
/// behind the "back and forth". Same row filter as [`latest_context`]: only rows from
/// `qid` that actually carry `usage.context` (turn-less status pings are skipped).
fn context_history(messages: &[super::super::MessageRow], qid: &str) -> Vec<f64> {
    messages
        .iter()
        .filter(|m| m.from == qid)
        .filter_map(|m| Some(m.usage.as_ref()?.context.as_ref()?.used_percentage))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::latest_context;
    use crate::model::MessageRow;
    use hadron_lattice::{ContextUsage, Usage};

    fn row(from: &str, ctx: Option<u32>) -> MessageRow {
        MessageRow {
            from: from.into(),
            to: None,
            body: String::new(),
            kind_label: "status",
            usage: ctx.map(|pct| Usage {
                context: Some(ContextUsage {
                    used_tokens: pct * 100,
                    context_window_size: 200_000,
                    used_percentage: pct as f64,
                }),
                ..Default::default()
            }),
            ts: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            legacy_used_tokens: None,
            turn: None,
        }
    }

    #[test]
    fn latest_context_skips_trailing_statusless_rows() {
        // A context-bearing turn row, then trailing status pings with no usage — the
        // exact real-field shape that hid Claude's gauge. The gauge must still find the
        // earlier reading rather than the last (context-less) row.
        let msgs = vec![
            row("acp-claude", Some(16)),
            row("acp-claude", None),
            row("acp-claude", None),
        ];
        assert_eq!(latest_context(&msgs, "acp-claude").map(|c| c.used_percentage), Some(16.0));
        // Most-recent reading wins when several exist.
        let msgs = vec![row("acp-claude", Some(9)), row("acp-claude", Some(16))];
        assert_eq!(latest_context(&msgs, "acp-claude").map(|c| c.used_percentage), Some(16.0));
        // A quark that never reported context has no gauge.
        assert!(latest_context(&[row("acp-claude", None)], "acp-claude").is_none());
    }

    #[test]
    fn context_history_collects_the_oscillating_series_in_order() {
        use super::context_history;
        // Real shape: context rises and falls, interleaved with status-less pings and
        // another quark's rows. Only this quark's context-bearing rows, oldest first.
        let msgs = vec![
            row("acp-claude", Some(5)),
            row("acp-agy", Some(40)),
            row("acp-claude", None),
            row("acp-claude", Some(8)),
            row("acp-claude", Some(5)),
        ];
        assert_eq!(context_history(&msgs, "acp-claude"), vec![5.0, 8.0, 5.0]);
        // Fewer than two points cannot draw a line (caller falls back to the meter).
        assert_eq!(context_history(&[row("acp-claude", Some(7))], "acp-claude"), vec![7.0]);
    }
}
