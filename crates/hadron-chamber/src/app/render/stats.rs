use super::*;

impl super::Chamber {
    /// The most recent context reading published by `qid` in the live field: the last
    /// message that actually carries `usage.context`, not merely the last message. A
    /// quark's trailing rows are usually turn-less status pings with no usage, so a plain
    /// `rfind(last row)` lands on one of those and reports "no context" even when a
    /// reading exists earlier this window — which is why the Current gauge showed for
    /// some quarks and not others.
    pub(super) fn latest_context(&self, qid: &str) -> Option<&hadron_lattice::ContextUsage> {
        latest_context(&self.view.messages, qid)
    }

    /// The most recent *live* quota buckets `qid` published in the live field. Like
    /// [`Self::latest_context`], this is a live gauge, not a window-summed quantity —
    /// a bucket only changes when the provider sends a fresh reading, which may predate
    /// the window's cutoff (in particular [`StatsWindow::Current`]'s "since the last
    /// human message" truncation), so it reads identically regardless of which stats
    /// window tab is selected rather than going missing on some of them. A bucket whose
    /// `reset_time` has already passed is spent history, not current state, and is
    /// dropped — see [`quota_is_live`].
    fn latest_quota(&self, qid: &str) -> Vec<hadron_lattice::QuotaBucket> {
        latest_quota(&self.view.messages, qid, chrono::Utc::now())
    }

    /// [`Self::latest_quota`], but when `qid` has none of its own, falls back to the
    /// newest live reading from a same-`vendor` peer: a subscription quota is billed
    /// per account, not per seat (`acp-claude` and `acp-claude-2` both draw on one
    /// claude.ai plan), so a seat that never reported quota itself can still show the
    /// account's real number instead of nothing. The returned `bool` marks a fallback
    /// reading so the renderer can label it "account-shared" rather than implying
    /// `qid` reported it. **Must run after `latest_quota`'s staleness filter** — a peer
    /// fallback shipped before that filter would copy a stale reading onto every seat.
    fn quota_for_display(&self, qid: &str) -> (Vec<hadron_lattice::QuotaBucket>, bool) {
        let vendor = self
            .view
            .roster
            .iter()
            .find(|r| r.id == qid)
            .map(|r| r.vendor.clone())
            .filter(|v| !v.is_empty());
        let Some(vendor) = vendor else {
            return (self.latest_quota(qid), false);
        };
        let roster = &self.view.roster;
        quota_with_fallback(
            &self.view.messages,
            |from| roster.iter().any(|r| r.id == from && r.vendor == vendor),
            qid,
            chrono::Utc::now(),
        )
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
            hadron_lattice::Transport::Http => seat
                .as_ref()
                .and_then(|s| s.http_base_url.clone())
                .unwrap_or_else(|| format!("default ({})", roster_row.vendor)),
        };
        let model_str = match roster_row.model_label() {
            label if label.is_empty() => "—".to_string(),
            label => label,
        };
        let transport_str = match roster_row.transport {
            hadron_lattice::Transport::Cli => "CLI (one-shot)",
            hadron_lattice::Transport::Acp => "ACP (resident)",
            hadron_lattice::Transport::Sdk => "SDK (unsupported)",
            hadron_lattice::Transport::Http => "HTTP (local server)",
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
                        format_num(ctx.used_tokens as u64),
                        format_num(ctx.context_window_size as u64)
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
        let (quota, quota_shared) = self.quota_for_display(&qid);
        let now = chrono::Utc::now();
        for bucket in quota {
            stats_block = stats_block.child(kv_row(
                "Quota",
                format!(
                    "{}: {:.0}% left{}",
                    quota_tag(&bucket.key, quota_shared),
                    bucket.remaining_fraction * 100.0,
                    quota_countdown_suffix(&bucket, now),
                ),
            ));
        }

        // Section tabs keep the panel short: the header stays pinned (you always see
        // whose panel this is), and one section shows at a time below it.
        let info_selected = self.info_tab;
        let info_tabs = h_flex()
            .id("info-capsule-tabs")
            .items_center()
            .gap_1()
            .p_1()
            .rounded_full()
            .bg(theme::glass_card())
            .border_1()
            .border_color(theme::glass_highlight())
            .max_w_full()
            .overflow_x_scroll()
            .children(InfoTab::ALL.map(|t| {
                let is_selected = t.index() == info_selected.index();
                let label = t.label();
                let ix = t.index();
                div()
                    .id(("info-tab-pill", ix))
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .cursor_pointer()
                    .when(is_selected, |s| {
                        s.bg(theme::glass_highlight())
                            .text_color(theme::accent())
                            .font_weight(gpui::FontWeight::BOLD)
                    })
                    .when(!is_selected, |s| {
                        s.text_color(theme::text_muted())
                            .hover(|h| h.text_color(theme::text()))
                    })
                    .text_xs()
                    .child(label)
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.info_tab = InfoTab::from_index(ix);
                        cx.notify();
                    }))
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
                    .bg(theme::glass_card())
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
        h_flex().child(
            h_flex()
                .id(id)
                .items_center()
                .gap_1()
                .p_1()
                .rounded_full()
                .bg(theme::glass_card())
                .border_1()
                .border_color(theme::glass_highlight())
                .overflow_x_scrollbar()
                .children(StatsWindow::ALL.map(|w| {
                    let is_selected = w == selected;
                    let label = w.label();
                    let target_window = w;
                    div()
                        .id(SharedString::from(format!("{id}-pill-{}", w.label())))
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .cursor_pointer()
                        .when(is_selected, |s| {
                            s.bg(theme::glass_highlight())
                                .text_color(theme::accent())
                                .font_weight(gpui::FontWeight::BOLD)
                        })
                        .when(!is_selected, |s| {
                            s.text_color(theme::text_muted())
                                .hover(|h| h.text_color(theme::text()))
                        })
                        .text_xs()
                        .child(label)
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.stats_window = target_window;
                            cx.notify();
                        }))
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
                                format_num(ctx.used_tokens as u64),
                                format_num(ctx.context_window_size as u64),
                            )),
                        )
                        .child(progress_meter(frac, q_color));
                }
            }
            // An empty quota list means the provider has no quota concept — not that the
            // quota is spent. Say nothing rather than render a zero. Read live (unwindowed,
            // like `latest_context`) rather than off the windowed fold `s.quota`, so it
            // does not go missing on windows whose cutoff excludes the last quota report
            // (Current's "since the last human message" truncation, in particular). Falls
            // back to a same-vendor peer's reading when `q` has none of its own.
            let (quota, quota_shared) = self.quota_for_display(q);
            let now = chrono::Utc::now();
            for bucket in quota {
                block = block.child(div().text_xs().text_color(theme::text_muted()).child(
                    format!(
                        "Quota [{}]: {:.0}% left{}",
                        quota_tag(&bucket.key, quota_shared),
                        bucket.remaining_fraction * 100.0,
                        quota_countdown_suffix(&bucket, now),
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
/// status-less rows" search, with any bucket whose `reset_time` has already passed
/// dropped (see [`quota_is_live`]). Empty when the quark never reported live quota.
fn latest_quota(
    messages: &[super::super::MessageRow],
    qid: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<hadron_lattice::QuotaBucket> {
    messages
        .iter()
        .rev()
        .filter(|m| m.from == qid)
        .find_map(|m| {
            let quota = &m.usage.as_ref()?.quota;
            (!quota.is_empty()).then(|| quota.clone())
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|b| quota_is_live(b, now))
        .collect()
}

/// [`latest_quota`], but when `qid` has no live buckets of its own, falls back to the
/// newest live reading from a peer for which `same_vendor_peer` returns `true` — a
/// subscription quota is billed per account, not per seat, so a seat with no reading
/// of its own can still show the account's real number. Returns `true` in the second
/// slot when the reading is such a fallback (the caller labels it "account-shared").
/// Pure over `(messages, same_vendor_peer, qid, now)`: membership in the vendor group
/// is the caller's job ([`Chamber::quota_for_display`] builds it from the roster),
/// which keeps this testable without a roster fixture.
fn quota_with_fallback(
    messages: &[super::super::MessageRow],
    same_vendor_peer: impl Fn(&str) -> bool,
    qid: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> (Vec<hadron_lattice::QuotaBucket>, bool) {
    let own = latest_quota(messages, qid, now);
    if !own.is_empty() {
        return (own, false);
    }
    let peer_bucket = messages
        .iter()
        .rev()
        .filter(|m| m.from != qid && same_vendor_peer(&m.from))
        .find_map(|m| {
            let quota = &m.usage.as_ref()?.quota;
            (!quota.is_empty()).then(|| quota.clone())
        })
        .map(|bucket| bucket.into_iter().filter(|b| quota_is_live(b, now)).collect::<Vec<_>>())
        .filter(|b| !b.is_empty())
        .unwrap_or_default();
    let shared = !peer_bucket.is_empty();
    (peer_bucket, shared)
}

/// Whether `bucket` still describes current state at `now`. A bucket with no
/// `reset_time` (the provider didn't say) is treated as live — there's nothing to
/// compare against. A bucket whose reset has already passed is spent history: the
/// window it described is over, so its old percentage would misrepresent a stale
/// reading as current (the "6% left" fossil from a session-limit error, read the next
/// day as if it were today's number).
fn quota_is_live(bucket: &hadron_lattice::QuotaBucket, now: chrono::DateTime<chrono::Utc>) -> bool {
    bucket.reset_time.is_none_or(|reset| reset > now)
}

/// The bucket key, tagged `", account"` when this reading is a same-vendor peer's
/// fallback rather than `qid`'s own — see [`Chamber::quota_for_display`].
fn quota_tag(key: &str, shared: bool) -> String {
    if shared {
        format!("{key}, account")
    } else {
        key.to_string()
    }
}

/// `" (resets in 2h 14m)"`, or empty when the provider gave no `reset_time`. Callers
/// only ever see live buckets ([`quota_is_live`] already dropped expired ones), so the
/// countdown here is always non-negative.
fn quota_countdown_suffix(bucket: &hadron_lattice::QuotaBucket, now: chrono::DateTime<chrono::Utc>) -> String {
    match bucket.reset_time {
        Some(reset) => format!(" (resets in {})", quota_countdown(reset, now)),
        None => String::new(),
    }
}

/// `"2h 14m"` / `"38m"` countdown from `now` to `reset_time`.
fn quota_countdown(reset_time: chrono::DateTime<chrono::Utc>, now: chrono::DateTime<chrono::Utc>) -> String {
    let mins = (reset_time - now).num_minutes().max(0);
    if mins >= 60 {
        format!("{}h {}m", mins / 60, mins % 60)
    } else {
        format!("{mins}m")
    }
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
    use hadron_lattice::{ContextUsage, QuotaBucket, Usage};

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
            severity: None,
        }
    }

    fn bucket(key: &str, remaining_fraction: f64, reset_time: Option<chrono::DateTime<chrono::Utc>>) -> QuotaBucket {
        QuotaBucket { key: key.into(), remaining_fraction, reset_time }
    }

    fn quota_row(from: &str, buckets: Vec<QuotaBucket>) -> MessageRow {
        MessageRow {
            from: from.into(),
            to: None,
            body: String::new(),
            kind_label: "status",
            usage: Some(Usage { quota: buckets, ..Default::default() }),
            ts: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            legacy_used_tokens: None,
            turn: None,
            severity: None,
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

    #[test]
    fn quota_is_live_drops_expired_buckets_only() {
        use super::quota_is_live;
        let now = chrono::DateTime::from_timestamp(1000, 0).unwrap();
        assert!(quota_is_live(&bucket("claude-five_hour", 0.06, Some(now + chrono::Duration::hours(1))), now));
        assert!(!quota_is_live(&bucket("claude-five_hour", 0.06, Some(now - chrono::Duration::hours(1))), now));
        // No reset_time at all: the provider didn't say, so there's nothing to have expired.
        assert!(quota_is_live(&bucket("claude-five_hour", 0.06, None), now));
    }

    #[test]
    fn quota_countdown_formats_hours_and_minutes() {
        use super::quota_countdown;
        let now = chrono::DateTime::from_timestamp(0, 0).unwrap();
        assert_eq!(quota_countdown(now + chrono::Duration::minutes(134), now), "2h 14m");
        assert_eq!(quota_countdown(now + chrono::Duration::minutes(38), now), "38m");
        // A reset time in the past never renders a negative countdown (defensive floor;
        // `latest_quota`/`quota_with_fallback` already filter these out beforehand).
        assert_eq!(quota_countdown(now - chrono::Duration::minutes(5), now), "0m");
    }

    #[test]
    fn latest_quota_drops_a_reading_whose_reset_time_has_passed() {
        use super::latest_quota;
        let now = chrono::DateTime::from_timestamp(10_000, 0).unwrap();
        // The fossil: yesterday's reset_time, asserted as current the next day.
        let msgs = vec![quota_row(
            "acp-claude-2",
            vec![bucket("claude-five_hour", 0.06, Some(now - chrono::Duration::hours(1)))],
        )];
        assert_eq!(latest_quota(&msgs, "acp-claude-2", now), Vec::<QuotaBucket>::new());
        // A live bucket survives.
        let msgs = vec![quota_row(
            "acp-claude-2",
            vec![bucket("claude-five_hour", 0.06, Some(now + chrono::Duration::hours(1)))],
        )];
        assert_eq!(latest_quota(&msgs, "acp-claude-2", now).len(), 1);
    }

    #[test]
    fn quota_with_fallback_prefers_own_reading_over_a_peers() {
        use super::quota_with_fallback;
        let now = chrono::DateTime::from_timestamp(10_000, 0).unwrap();
        let live = Some(now + chrono::Duration::hours(1));
        let msgs = vec![
            quota_row("acp-claude", vec![bucket("claude-five_hour", 0.40, live)]),
            quota_row("acp-claude-2", vec![bucket("claude-five_hour", 0.06, live)]),
        ];
        let same_vendor = |id: &str| id == "acp-claude" || id == "acp-claude-2";
        let (own, shared) = quota_with_fallback(&msgs, same_vendor, "acp-claude-2", now);
        assert_eq!(own.first().map(|b| b.remaining_fraction), Some(0.06));
        assert!(!shared);
    }

    #[test]
    fn quota_with_fallback_uses_a_same_vendor_peer_when_qid_has_none() {
        use super::quota_with_fallback;
        let now = chrono::DateTime::from_timestamp(10_000, 0).unwrap();
        let live = Some(now + chrono::Duration::hours(1));
        // Only "acp-claude" ever reported quota; "acp-claude-2" never has.
        let msgs = vec![quota_row("acp-claude", vec![bucket("claude-five_hour", 0.40, live)])];
        let same_vendor = |id: &str| id == "acp-claude" || id == "acp-claude-2";
        let (fallback, shared) = quota_with_fallback(&msgs, same_vendor, "acp-claude-2", now);
        assert_eq!(fallback.first().map(|b| b.remaining_fraction), Some(0.40));
        assert!(shared);
        // A quark with no same-vendor peers at all gets nothing, not a crash.
        let (none, shared) = quota_with_fallback(&msgs, |_| false, "acp-agy", now);
        assert!(none.is_empty());
        assert!(!shared);
    }

    #[test]
    fn quota_with_fallback_never_surfaces_a_peers_stale_reading() {
        use super::quota_with_fallback;
        let now = chrono::DateTime::from_timestamp(10_000, 0).unwrap();
        let expired = Some(now - chrono::Duration::hours(1));
        let msgs = vec![quota_row("acp-claude", vec![bucket("claude-five_hour", 0.40, expired)])];
        let same_vendor = |id: &str| id == "acp-claude" || id == "acp-claude-2";
        let (fallback, shared) = quota_with_fallback(&msgs, same_vendor, "acp-claude-2", now);
        assert!(fallback.is_empty());
        assert!(!shared);
    }
}
