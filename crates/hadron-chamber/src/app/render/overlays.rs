use super::*;

impl super::Chamber {
    /// The completion card: rows floating just above the message box, spanning the
    /// input's full width. It is a normal render-tree descendant — `.absolute()`
    /// with `.bottom(100%)` inside the input area's `.relative()` wrapper — so it
    /// draws *upward* and stays inside the window, unlike the fork's `deferred()`
    /// menu that painted off the bottom edge (`completion-menu-draws-out-of-bounds`).
    pub(super) fn completion_card_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let card = self.completion.as_ref();
        let mut list = v_flex()
            .id("completion-card-list")
            .flex_1()
            .min_h_0()
            .max_h(px(280.0))
            .overflow_y_scroll()
            .track_scroll(&self.completion_scroll)
            .p_1()
            .gap_1();

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

        h_flex()
            .id("completion-card")
            .absolute()
            .bottom(gpui::relative(1.0))
            .left_0()
            .right_0()
            .mb_2()
            .occlude()
            .max_h(px(280.0))
            .bg(theme::field_base())
            .border_1()
            .border_color(theme::border())
            .rounded_lg()
            .overflow_hidden()
            .child(list)
            .child(
                Scrollbar::vertical(&self.completion_scroll)
                    .scrollbar_show(ScrollbarShow::Hover)
            )
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
                    // Flat #101010 field colour — opaque, so the field can't bleed
                    // through, and matches the About dialog to the solid background
                    // (Jake's request). Settings/app-menu still use modal_surface.
                    .bg(theme::field_base())
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

    /// Best-effort, read-only probe of whether `hadron-gluon` currently holds
    /// `gluon.lock` — the same flock check `main.rs` runs once at chamber startup
    /// (`gluon_running`), made callable live each time the Process Manager opens.
    /// Any lock this acquires is released immediately; it never blocks the daemon.
    fn gluon_running(&self) -> bool {
        let field_dir = hadron_lattice::hadron_dir_of(&self.path);
        let lock_path = field_dir.join("gluon.lock");
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let Ok(file) = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&lock_path)
            else {
                return false;
            };
            let fd = file.as_raw_fd();
            // SAFETY: `fd` is a valid, open descriptor owned by `file` for this call.
            let acquired = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0;
            if acquired {
                unsafe { libc::flock(fd, libc::LOCK_UN) };
            }
            // We could take the lock ourselves → nobody holds it → gluon is NOT running.
            !acquired
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// Live rows for the Process Manager overlay: the daemon's real running state
    /// (a flock probe — only the OS knows) plus every adopted roster seat. See
    /// [`crate::model::build_process_rows`] for the pure row-building logic.
    pub(super) fn resolve_running_processes(&self) -> Vec<crate::model::ProcessRow> {
        crate::model::build_process_rows(self.gluon_running(), &self.view.roster)
    }

    /// The Process Manager overlay: a dim backdrop (click to dismiss) behind a card
    /// listing the daemon and every adopted quark seat, each with its live status
    /// and whichever *real* control action applies — force-restart (`Kind::Reboot`,
    /// [`Self::reboot_quark`]) for an enabled resident ACP seat, and the
    /// enable/disable toggle ([`Self::toggle_quark_enabled`]) every adopted seat
    /// already has in Settings. Deliberately no OS-level "Kill": the chamber is a
    /// separate process from the daemon and never sees a quark's child PID, so a
    /// kill switch here would have nothing real to act on.
    pub(super) fn process_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.resolve_running_processes();

        let live_dir = hadron_lattice::live::live_dir(&self.path);
        let now = chrono::Utc::now();

        let mut list = v_flex().gap_1p5();
        for row in rows {
            // Determine dot color & presence label using Hadron SSOT theme presence logic
            let (dot, status_label) = if row.id == "gluon" {
                if row.status == "Running" {
                    (theme::presence(hadron_lattice::QuarkState::Ground), "Running".to_string())
                } else {
                    (theme::presence(hadron_lattice::QuarkState::Error), "Stopped".to_string())
                }
            } else if let Some(roster_row) = self.view.roster.iter().find(|r| r.id == row.id) {
                let activity = hadron_lattice::live::read(
                    &live_dir,
                    &hadron_lattice::QuarkId::new(&row.id),
                    now,
                );
                let effective_state = effective_presence_state(
                    roster_row.state,
                    roster_row.adopted,
                    roster_row.enabled,
                    activity.is_some(),
                );
                if roster_row.adopted && roster_row.enabled {
                    (
                        theme::presence(effective_state),
                        theme::presence_label(effective_state).to_string(),
                    )
                } else if !roster_row.adopted {
                    (theme::presence_disabled(), "available".to_string())
                } else {
                    (theme::presence_disabled(), "disabled".to_string())
                }
            } else {
                (theme::presence_disabled(), row.status.clone())
            };

            let avatar_or_icon = if row.id == "gluon" {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .rounded_full()
                    .bg(theme::bg_surface_raised())
                    .text_color(theme::text_secondary())
                    .child(Icon::new(IconName::Cpu).small())
                    .into_any_element()
            } else {
                let resolved = self.resolve_identity(&row.id);
                identity_avatar(&resolved, 24.0).into_any_element()
            };

            let mut row_actions = h_flex().gap_1p5();
            if row.can_restart {
                let id = row.id.clone();
                row_actions = row_actions.child(
                    text_button(SharedString::from(format!("proc-restart-{}", row.id)), "Restart")
                        .on_click(cx.listener(move |this, _, _, cx| this.reboot_quark(&id, cx))),
                );
            }
            if row.can_toggle {
                let id = row.id.clone();
                let label = if row.enabled { "Disable" } else { "Enable" };
                row_actions = row_actions.child(
                    text_button(SharedString::from(format!("proc-toggle-{}", row.id)), label).on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_quark_enabled(&id, cx)),
                    ),
                );
            }

            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("proc-row-{}", row.id)))
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(theme::bg_surface())
                    .hover(|s| s.bg(theme::bg_surface_raised()))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2p5()
                            .child(avatar_or_icon)
                            .child(div().size(px(7.0)).rounded_full().bg(dot))
                            .child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).text_color(theme::text()).child(row.label)),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(status_label),
                            )
                            .child(row_actions),
                    ),
            );
        }

        let card = v_flex()
            .occlude()
            .w(px(500.0))
            .max_h(px(560.0))
            .p_4()
            .gap_4()
            .rounded(INNER_RADIUS)
            .bg(theme::modal_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {}) // swallow inner clicks
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text())
                                    .child("Processes"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child("Daemon & Quark Seat Control"),
                            ),
                    )
                    .child(
                        div()
                            .id("processes-close")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(24.0))
                            .rounded_full()
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
                            .child(Icon::new(IconName::WindowClose).small())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_process_manager(cx))),
                    ),
            )
            .child(
                div()
                    .id("processes-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            );

        div()
            .id("processes-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| this.toggle_process_manager(cx)),
            )
            .child(card)
    }

    /// The per-quark permission ladder (Ask / Write / Auto / Bypass) as an explicit
    /// segmented picker for Settings. Unlike the roster's cycle-on-click tag, each rung is
    /// directly selectable, the current resolved mode is highlighted on its risk colour,
    /// and a gloss explains what the choice delegates. The leading **Default** rung clears
    /// any override (`ModeClear`) so the quark follows the global default; the four posture
    /// rungs each pin a per-quark `ModeSet` override. The daemon honours it next tick.
    pub(crate) fn mode_select(&self, id: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
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
}
