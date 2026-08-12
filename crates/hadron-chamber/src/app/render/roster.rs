use super::*;
use hadron_lattice::QuarkState;

impl super::Chamber {
    pub(super) fn roster_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = v_flex().w_full().gap_2();
        let live_dir = hadron_lattice::live::live_dir(&self.path);

        for (ix, r) in self.view.roster.iter().enumerate() {
            let is_selected = self.selected_quark_ix == Some(ix);
            let activity = hadron_lattice::live::read(
                &live_dir,
                &hadron_lattice::QuarkId::new(&r.id),
                chrono::Utc::now(),
            );
            rows = rows.child(self.render_quark_card(r, ix, is_selected, activity, cx));
        }

        if self.view.roster.is_empty() {
            rows = rows.child(
                div()
                    .text_sm()
                    .text_color(theme::text_muted())
                    .p_3()
                    .child("no quarks yet"),
            );
        }

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
            .child(self.render_worktree_selector(cx))
            .child(
                div()
                    .id("roster-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(rows),
            )
            .child(self.render_security_posture_pill(cx))
            .child(self.processes_button(cx, false))
            .child(self.settings_button(cx, false));

        v_flex().w_full().h_full().min_h_0().p_2().child(card)
    }

    /// Renders the floating active worktree selector chip header displaying workspace dir & git branch.
    pub(super) fn render_worktree_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repo_dir = self
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("hadron");
        let branch = hadron_gluon::worktree::current_branch(&self.path)
            .unwrap_or_else(|| "main".to_string());

        let mut title_child = h_flex()
            .items_center()
            .gap_1p5()
            .child(Icon::new(IconName::Folder).small())
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::text())
                    .child(repo_dir.to_string()),
            )
            .child(div().text_color(theme::text_muted()).child("/"))
            .child(Icon::new(IconName::FolderOpen).small())
            .child(
                div()
                    .text_color(theme::text_muted())
                    .child(branch),
            );

        if self.nucleus_over_budget {
            title_child = title_child.child(
                div()
                    .id("nucleus-over-budget-warning")
                    .text_xs()
                    .text_color(theme::quark_state(QuarkState::Waiting))
                    .child("⚠ over budget")
                    .tooltip(|window, cx| {
                        Tooltip::new(
                            ".hadron/nucleus/index.md exceeds 32 KiB and is truncated in prompts",
                        )
                        .build(window, cx)
                    }),
            );
        }

        h_flex()
            .id("worktree-selector-chip")
            .w_full()
            .justify_between()
            .items_center()
            .px_3()
            .py_2()
            .text_xs()
            .bg(theme::glass_card())
            .rounded_lg()
            .border_1()
            .border_color(theme::glass_highlight())
            .child(title_child)
            .child(
                div()
                    .id("roster-toggle")
                    .cursor_pointer()
                    .active(|s| s.opacity(0.6))
                    .child(Icon::new(IconName::PanelLeftClose).small())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_rail(Rail::Roster, window, cx)
                    })),
            )
    }

    /// Renders a single borderless Quark fleet card with status halo, avatar, telemetry metrics, and context menu.
    pub(super) fn render_quark_card(
        &self,
        r: &RosterRow,
        ix: usize,
        is_selected: bool,
        activity: Option<hadron_lattice::live::Activity>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let identity = self.resolve_identity(&r.id);
        let qid = r.id.clone();
        let is_acp = matches!(r.transport, hadron_lattice::Transport::Acp);

        let effective_state = effective_presence_state(
            r.state,
            r.adopted,
            r.enabled,
            activity.is_some(),
        );

        let halo_dot_el = self.render_halo_dot(effective_state, r.enabled);

        let mode_el = div()
            .id(SharedString::from(format!("mode-{}", r.id)))
            .cursor_pointer()
            .flex_none()
            .on_click(cx.listener(move |this, _, _, cx| this.cycle_quark_mode(&qid, cx)))
            .child(mode_tag(r.mode, !r.mode_is_override));

        // Header line: Halo + Avatar + Quark Name + Transport Badge + Model Tag
        let header_row = h_flex()
            .items_center()
            .justify_between()
            .w_full()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .child(identity_avatar(&identity, 24.0))
                                    .map(|el| if r.enabled { el } else { el.opacity(0.6) }),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .right_0()
                                    .child(halo_dot_el),
                            ),
                    )
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_sm()
                            .text_color(if r.enabled { theme::text() } else { theme::text_muted() })
                            .truncate()
                            .child(identity.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .px_1p5()
                            .py_0p5()
                            .rounded_md()
                            .bg(theme::bg_surface())
                            .text_color(theme::text_muted())
                            .child(r.transport.code()),
                    )
                    .when(!r.model_label().is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .px_1p5()
                                .py_0p5()
                                .rounded_md()
                                .bg(theme::bg_surface())
                                .text_color(theme::text_secondary())
                                .truncate()
                                .child(r.model_label().to_string()),
                        )
                    }),
            );

        // Telemetry line: excited_time (time_per_task)  (larger gap)  context/context_window (%)
        let now = chrono::Utc::now();
        let latest_task = self.view.tasks.iter().find(|t| t.to == r.id);
        let excited_secs = latest_task.map(|t| t.elapsed_secs(now)).unwrap_or(0);

        fn format_dur(secs: i64) -> String {
            if secs < 60 {
                format!("{}s", secs)
            } else if secs < 3600 {
                format!("{}m {}s", secs / 60, secs % 60)
            } else {
                format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
            }
        }

        let excited_time_str = format_dur(excited_secs);

        let time_part = if let Some(act) = &activity {
            let task_secs = (now - act.at).num_seconds().max(0);
            if excited_secs > task_secs && task_secs > 0 {
                format!("{} ({})", excited_time_str, format_dur(task_secs))
            } else {
                excited_time_str
            }
        } else {
            excited_time_str
        };

        let ctx_opt = self.latest_context(&r.id);
        let context_str = if let Some(ctx) = ctx_opt {
            format!(
                "{} / {} ({:.0}%)",
                format_num(ctx.used_tokens as u64),
                format_num(ctx.context_window_size as u64),
                ctx.used_percentage,
            )
        } else if r.tokens > 0 {
            format!("{} tok", format_num(r.tokens as u64))
        } else {
            "0 tok".to_string()
        };

        let telemetry_row = h_flex()
            .items_center()
            .justify_between()
            .w_full()
            .text_xs()
            .text_color(theme::text_muted())
            .child(
                div()
                    .truncate()
                    .child(time_part),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().truncate().child(context_str))
                    .when(matches!(r.effort.as_deref(), Some(e) if !e.is_empty()), |this| {
                        this.child(effort_tag(&r.effort))
                    })
                    .child(mode_el),
            );

        div()
            .id(SharedString::from(format!("roster-row-{}", r.id)))
            .w_full()
            .p_2p5()
            .gap_1p5()
            .rounded_lg()
            .bg(theme::glass_card())
            .border_1()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_quark_ix = Some(ix);
                cx.notify();
            }))
            .border_color(if is_selected {
                gpui::rgba(0xffffff38).into()
            } else {
                theme::glass_highlight()
            })
            .context_menu({
                let qid_str = r.id.clone();
                let r_flavor = r.flavor.clone();
                let is_adopted = r.adopted;
                let is_enabled = r.enabled;
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
                    if is_acp {
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
                    let qid2 = qid_str.clone();
                    let view2 = view.clone();
                    let toggle_lbl = if is_enabled { "Disable" } else { "Enable" };
                    menu = menu.item(PopupMenuItem::new(toggle_lbl).on_click(move |_, window, cx| {
                        view2.update(cx, |this, cx| {
                            this.handle_context_menu_action(
                                ContextMenuAction::ToggleQuark(qid2.clone()),
                                cx,
                            );
                        });
                        window.refresh();
                    }));
                    if !is_adopted {
                        let qid3 = qid_str.clone();
                        let view3 = view.clone();
                        menu = menu.item(PopupMenuItem::new("Adopt").on_click(
                            move |_, window, cx| {
                                view3.update(cx, |this, cx| {
                                    this.handle_context_menu_action(
                                        ContextMenuAction::AdoptQuark(qid3.clone()),
                                        cx,
                                    );
                                });
                                window.refresh();
                            },
                        ));
                    }
                    if let Some(hadron_lattice::Flavor::Worker) = r_flavor {
                        let qid4 = qid_str.clone();
                        let view4 = view.clone();
                        menu = menu.item(PopupMenuItem::new("Make Orchestrator").on_click(
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
                    menu
                }
            })
            .child(v_flex().w_full().gap_1p5().child(header_row).child(telemetry_row))
    }

    /// Renders an 8px vector GPU-native status halo indicator dot.
    pub(crate) fn render_halo_dot(&self, state: QuarkState, enabled: bool) -> impl IntoElement {
        let color = if enabled {
            theme::halo_dot(state)
        } else {
            gpui::rgb(0x71717a).into()
        };
        div()
            .w(px(8.0))
            .h(px(8.0))
            .rounded_full()
            .bg(color)
            .flex_none()
    }

    /// Renders the bottom F6 security posture toggle pill (ASK / WRITE / AUTO / BYPASS).
    pub(super) fn render_security_posture_pill(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.view.global_mode;
        let mode_lbl = mode_label(mode);
        let mode_clr = mode_color(mode);
        let mode_hnt = mode_hint(mode);

        div()
            .id("security-posture-pill")
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_1p5()
            .rounded_md()
            .bg(theme::glass_card())
            .border_1()
            .border_color(theme::glass_highlight())
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.cycle_global_mode(cx)))
            .tooltip(move |window, cx| Tooltip::new(mode_hnt).build(window, cx))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(IconName::Info).small())
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text())
                            .child("F6 Posture"),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(mode_clr)
                    .text_color(mode_clr)
                    .child(mode_lbl),
            )
    }

    /// The Process Manager entry pinned directly above [`Self::settings_button`] at the foot of the Quarks rail.
    pub(super) fn processes_button(&self, cx: &mut Context<Self>, icon_only: bool) -> impl IntoElement {
        div()
            .id("processes")
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
            .child(Icon::new(IconName::Cpu).small())
            .when(!icon_only, |this| this.child("Processes"))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_process_manager(cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{QuarkState, Mode};

    #[test]
    fn test_quark_status_halo_resolution() {
        assert_eq!(theme::halo_dot(QuarkState::Ground), theme::halo_idle());
        assert_eq!(theme::halo_dot(QuarkState::Excited), theme::halo_active());
        assert_eq!(theme::halo_dot(QuarkState::Thinking), theme::halo_reasoning());
        assert_eq!(theme::halo_dot(QuarkState::Waiting), theme::halo_idle());
        assert_eq!(theme::halo_dot(QuarkState::Blocked), theme::halo_error());
        assert_eq!(theme::halo_dot(QuarkState::Error), theme::halo_error());
    }

    #[test]
    fn test_security_posture_mode_cycling() {
        assert_eq!(next_global_mode(Mode::Ask), Mode::Write);
        assert_eq!(next_global_mode(Mode::Write), Mode::Auto);
        assert_eq!(next_global_mode(Mode::Auto), Mode::Bypass);
        assert_eq!(next_global_mode(Mode::Bypass), Mode::Ask);
    }

    #[test]
    fn test_worktree_selector_branch_fallback() {
        let temp_dir = tempfile::tempdir().unwrap();
        let branch = hadron_gluon::worktree::current_branch(temp_dir.path())
            .unwrap_or_else(|| "main".to_string());
        assert_eq!(branch, "main");
    }

    #[test]
    fn test_nucleus_budget_warning_chip_logic() {
        let temp_dir = tempfile::tempdir().unwrap();
        assert!(!hadron_gluon::nucleus_status::index_over_budget(
            temp_dir.path(),
            32 * 1024
        ));

        let nucleus_dir = temp_dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus_dir).unwrap();
        std::fs::write(nucleus_dir.join("index.md"), vec![b'a'; 32 * 1024 + 1]).unwrap();

        assert!(hadron_gluon::nucleus_status::index_over_budget(
            temp_dir.path(),
            32 * 1024
        ));
    }

    #[test]
    fn test_context_menu_item_conditions() {
        use hadron_lattice::{Transport, Flavor};

        let resolve_items = |transport: Transport, adopted: bool, enabled: bool, flavor: Option<Flavor>| -> Vec<&'static str> {
            let is_acp = matches!(transport, Transport::Acp);
            let enable_str = if enabled { "Disable" } else { "Enable" };
            let mut items = vec!["Info"];
            if is_acp {
                items.push("Restart");
            }
            if !adopted {
                items.push("Adopt into repo");
                return items;
            }
            items.push(enable_str);
            if let Some(Flavor::Worker) = flavor {
                items.push("Make Orchestrator");
            }
            items
        };

        // 1. Unadopted ACP Worker (Enabled) -> Info, Restart, Adopt into repo
        assert_eq!(
            resolve_items(Transport::Acp, false, true, Some(Flavor::Worker)),
            vec!["Info", "Restart", "Adopt into repo"]
        );

        // 2. Unadopted CLI Worker (Enabled) -> Info, Adopt into repo (No Restart for non-ACP)
        assert_eq!(
            resolve_items(Transport::Cli, false, true, Some(Flavor::Worker)),
            vec!["Info", "Adopt into repo"]
        );

        // 3. Adopted ACP Worker (Enabled) -> Info, Restart, Disable, Make Orchestrator
        assert_eq!(
            resolve_items(Transport::Acp, true, true, Some(Flavor::Worker)),
            vec!["Info", "Restart", "Disable", "Make Orchestrator"]
        );

        // 4. Adopted ACP Worker (Disabled) -> Info, Restart, Enable, Make Orchestrator
        assert_eq!(
            resolve_items(Transport::Acp, true, false, Some(Flavor::Worker)),
            vec!["Info", "Restart", "Enable", "Make Orchestrator"]
        );

        // 5. Adopted ACP Orchestrator (Enabled) -> Info, Restart, Disable (No Make Orchestrator)
        assert_eq!(
            resolve_items(Transport::Acp, true, true, Some(Flavor::Orchestrator)),
            vec!["Info", "Restart", "Disable"]
        );

        // 6. Adopted SDK Worker (Enabled) -> Info, Disable, Make Orchestrator (No Restart for SDK)
        assert_eq!(
            resolve_items(Transport::Sdk, true, true, Some(Flavor::Worker)),
            vec!["Info", "Disable", "Make Orchestrator"]
        );
    }
}


