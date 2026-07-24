use super::*;

impl super::Chamber {
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
        let live_dir = hadron_lattice::live::live_dir(&self.path);
        for (ix, r) in self.view.roster.iter().enumerate() {
            let is_selected = self.selected_quark_ix == Some(ix);
            let activity = hadron_lattice::live::read(&live_dir, &hadron_lattice::QuarkId::new(&r.id), chrono::Utc::now());
            // The per-quark mode tag is clickable → cycle this quark's override.
            let qid = r.id.clone();
            let mode_el = div()
                .id(SharedString::from(format!("mode-{}", r.id)))
                .cursor_pointer()
                .flex_none()
                .on_click(cx.listener(move |this, _, _, cx| this.cycle_quark_mode(&qid, cx)))
                // A quark with no per-quark override shows a grey "Default" chip;
                // an override shows the actual mode. (`is_default = !override`.)
                .child(mode_tag(r.mode, !r.mode_is_override))
                .into_any_element();

            // Restart is meaningful for any resident (ACP) seat — a one-shot CLI quark
            // holds nothing between turns. NOT gated on `adopted`: the daemon seats
            // resident quarks straight from the global catalogue (adopted=false in this
            // repo, but very much live), and `reset_session` is idempotent, so a click
            // on a seat with no live session is a harmless no-op.
            let is_acp = matches!(r.transport, hadron_lattice::Transport::Acp);

            // Trailing controls, right-aligned: effort tag (only when the seat carries an
            // explicit effort) and the mode tag (always shown now — solid for a per-quark
            // override, outlined for the inherited/global mode; click to cycle an override).
            // Restart lives in the right-click context menu now (below), not as a row glyph.
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
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected_quark_ix = Some(ix);
                    cx.notify();
                }))
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
                                hadron_lattice::Flavor::Orchestrator => {}
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
                .child(roster_row(&self.resolve_identity(&r.id), r, activity, controls));
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
            // Processes pinned directly above Settings at the bottom of the rail.
            .child(self.processes_button(cx, false))
            // Settings pinned to the bottom of the rail.
            .child(self.settings_button(cx, false));

        v_flex().w_full().h_full().min_h_0().p_2().child(card)
    }

    /// The Process Manager entry pinned directly above [`Self::settings_button`] at
    /// the foot of the Quarks rail — opens the process control overlay.
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
