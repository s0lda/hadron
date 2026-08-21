use super::*;

impl super::Chamber {
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
                    .gap_2()
                    .child(menu_button(&cx.entity()))
                    .children(self.update_pill(cx)),
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
            // Folded state still needs to answer "who's here and what are they
            // doing" — a per-quark avatar + status dot, reusing the exact
            // composition `roster_row` uses (`identity_avatar` + the presence
            // dot), just smaller. Scrolls within the strip (mirrors the expanded
            // roster's own `overflow_y_scroll` rows) so a long roster doesn't
            // push the pinned Settings button off the bottom.
            // A touch of horizontal padding + a smaller (20px) avatar so the folded
            // avatars keep a clear gutter from the strip edges instead of touching them.
            let live_dir = hadron_lattice::live::live_dir(&self.path);
            let mut avatars = v_flex().w_full().gap_2p5().items_center().px_1();
            let mut sorted_roster: Vec<_> = self
                .view
                .roster
                .iter()
                .map(|r| {
                    let id = self.resolve_identity(&r.id);
                    (r, id)
                })
                .collect();
            sorted_roster.sort_by(|a, b| {
                a.1.name
                    .to_lowercase()
                    .cmp(&b.1.name.to_lowercase())
                    .then_with(|| a.0.id.to_lowercase().cmp(&b.0.id.to_lowercase()))
            });
            for (r, id) in sorted_roster {
                let activity = hadron_lattice::live::read(
                    &live_dir,
                    &hadron_lattice::QuarkId::new(&r.id),
                    chrono::Utc::now(),
                );
                let effective_state =
                    effective_presence_state(r.state, r.adopted, r.enabled, activity.is_some());
                let dot_color = if r.adopted && r.enabled {
                    theme::presence(effective_state)
                } else {
                    theme::presence_disabled()
                };
                let dot = div()
                    .absolute()
                    .bottom_0()
                    .right_0()
                    .size(px(7.0))
                    .rounded_full()
                    .bg(dot_color)
                    .border_2()
                    .border_color(theme::bg_elevated());
                avatars = avatars.child(
                    div()
                        .id(SharedString::from(format!("rail-avatar-{}", r.id)))
                        .relative()
                        .child(
                            identity_avatar_with_state(&id, 20.0, Some(effective_state), r.enabled),
                        )
                        .child(dot),
                );
            }
            col = col
                .child(
                    div()
                        .id("rail-roster-scroll")
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(avatars),
                )
                .child(self.processes_button(cx, true))
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

    pub(super) fn update_pill(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (label, tip, bg_color, text_color, icon) = match &self.update_state {
            UpdateState::Available { version, .. } => (
                format!("v{} Available", version),
                format!("Hadron v{} is available. Click to install update.", version),
                theme::bg_surface_raised(),
                theme::accent(),
                IconName::ArrowUp,
            ),
            UpdateState::Installing { version } => (
                format!("Installing v{}...", version),
                // A cold build of the workspace is minutes long, and a pill that says
                // only "in background" for that long reads as hung — it was killed and
                // retried four times on 2026-07-30. Naming the log gives it somewhere
                // to be watched from.
                match crate::app::update::update_log_path() {
                    Some(p) => format!(
                        "Installing Hadron v{} in background (a few minutes). Log: {}",
                        version,
                        p.display()
                    ),
                    None => format!("Installing Hadron v{} in background...", version),
                },
                theme::accent_soft(),
                theme::text(),
                IconName::ArrowUp,
            ),
            UpdateState::Installed { version } => (
                format!("v{} installed — restarting…", version),
                format!(
                    "Hadron v{} is installed. The chamber is stopping the daemon and \
                     relaunching itself on the new build — this window will reappear in \
                     a moment.",
                    version
                ),
                theme::accent_soft(),
                theme::accent(),
                IconName::CircleCheck,
            ),
            UpdateState::Checking => (
                "Checking...".to_string(),
                "Checking for updates...".to_string(),
                theme::bg_surface_raised(),
                theme::text_muted(),
                IconName::ArrowUp,
            ),
            UpdateState::UpToDate => (
                format!("v{} (Up to date)", env!("CARGO_PKG_VERSION")),
                format!("Hadron v{} is up to date.", env!("CARGO_PKG_VERSION")),
                theme::bg_surface_raised(),
                theme::text_muted(),
                IconName::CircleCheck,
            ),
            UpdateState::Failed(msg) => (
                "Update Failed".to_string(),
                format!("Could not update: {}", msg),
                theme::bg_surface_raised(),
                theme::danger(),
                IconName::Info,
            ),
            UpdateState::Idle => return None,
        };

        Some(
            div()
                .id("update-pill")
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(bg_color)
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text_color)
                .cursor_pointer()
                .hover(|s| s.opacity(0.85))
                .active(|s| s.opacity(0.7))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.trigger_update_flow(window, cx);
                }))
                .child(Icon::new(icon).small())
                .child(label)
                .tooltip(move |window, cx| Tooltip::new(SharedString::from(tip.clone())).build(window, cx)),
        )
    }
}
