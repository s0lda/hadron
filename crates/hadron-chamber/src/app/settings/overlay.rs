use super::*;

impl super::Chamber {
    // `pub(crate)`, not `pub(super)`: called from `app::render::mod` (a sibling of
    // `settings`), the same reach it had when this fn lived directly in settings.rs.
    pub(crate) fn settings_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.settings_target.clone();

        // Left nav: Settings (General: Appearance, Execution, Environment; Providers; Skills), then Roster (Human, Quarks)
        let is_general_active = matches!(
            target,
            SettingsTarget::General
                | SettingsTarget::Appearance
                | SettingsTarget::Execution
                | SettingsTarget::Environment
        );
        let mut nav = v_flex()
            .gap_0p5()
            .child(
                div()
                    .px_1()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted())
                    .child("SETTINGS"),
            )
            .child(
                h_flex()
                    .id("settings-group-general")
                    .items_center()
                    .justify_between()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .bg(if is_general_active && !self.settings_general_expanded {
                        theme::bg_surface_raised()
                    } else if is_general_active {
                        theme::bg_surface()
                    } else {
                        theme::bg_base()
                    })
                    .hover(|s| s.bg(theme::bg_surface()))
                    .cursor_pointer()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(px(24.0))
                                    .text_color(if is_general_active {
                                        theme::accent()
                                    } else {
                                        theme::text_muted()
                                    })
                                    .child(Icon::new(IconName::Settings).small()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if is_general_active {
                                        theme::text()
                                    } else {
                                        theme::text_secondary()
                                    })
                                    .child("General"),
                            ),
                    )
                    .child(
                        Icon::new(if self.settings_general_expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .text_color(theme::text_muted()),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.settings_general_expanded = !this.settings_general_expanded;
                        if this.settings_general_expanded
                            && !matches!(
                                this.settings_target,
                                SettingsTarget::Appearance
                                    | SettingsTarget::Execution
                                    | SettingsTarget::Environment
                            )
                        {
                            this.select_settings_target(SettingsTarget::Appearance, window, cx);
                        }
                        cx.notify();
                    })),
            )
            .when(self.settings_general_expanded, |nav| {
                nav.child(self.settings_sub_nav_row(SettingsTarget::Appearance, &target, cx))
                    .child(self.settings_sub_nav_row(SettingsTarget::Execution, &target, cx))
                    .child(self.settings_sub_nav_row(SettingsTarget::Environment, &target, cx))
            })
            .child(self.settings_nav_row(SettingsTarget::Providers, &target, cx))
            .child(self.settings_nav_row(SettingsTarget::Skills, &target, cx))
            .child(
                div()
                    .px_1()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted())
                    .child("ROSTER"),
            )
            .child(self.settings_nav_row(SettingsTarget::Human, &target, cx));
        for r in &self.view.roster {
            nav =
                nav.child(self.settings_nav_row(SettingsTarget::Quark(r.id.clone()), &target, cx));
        }

        // "+ Add Quark" quick action at the bottom of sidebar roster list
        nav = nav.child(
            h_flex()
                .id("settings-add-quark-btn")
                .items_center()
                .gap_2()
                .w_full()
                .px_2()
                .py_1p5()
                .mt_2()
                .rounded_md()
                .border_1()
                .border_color(theme::border())
                .bg(theme::bg_base())
                .hover(|s| s.bg(theme::bg_surface_raised()))
                .cursor_pointer()
                .child(Icon::new(IconName::Plus).small().text_color(theme::accent()))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme::accent())
                        .child("Add Quark"),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_settings_target(SettingsTarget::Providers, window, cx)
                })),
        );

        // Live preview: the target resolved, but with the in-progress name/image
        // from the inputs so it tracks typing.
        let live_name = self.settings_name.read(cx).value().to_string();
        let live_path = self.settings_path.read(cx).value().trim().to_string();
        let mut preview = self.resolve_identity(target.key());
        if !live_name.trim().is_empty() {
            preview.name = live_name;
        }
        preview.image = (!live_path.is_empty()).then_some(live_path);

        // Color swatches; the stored color (if any) gets a bright ring.
        let selected = self.settings_color();
        let mut swatches = h_flex().gap_1().items_center().justify_end();
        for hex in IDENTITY_SWATCHES {
            let is_sel = selected.as_deref() == Some(format!("#{hex:06x}").as_str());
            swatches = swatches.child(
                div()
                    .id(SharedString::from(format!("swatch-{hex:06x}")))
                    .size(px(16.0))
                    .rounded_full()
                    .bg(rgb(hex))
                    .border_2()
                    .border_color(if is_sel {
                        theme::text()
                    } else {
                        theme::border()
                    })
                    .hover(|s| s.border_color(theme::text_secondary()))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_settings_color(hex, cx))),
            );
        }
        // A full picker for any colour beyond the presets; its Change event writes the
        // identity's colour (subscribed in `new`). Not value-synced to the target on
        // switch — the swatch ring already shows the current selection, and syncing would
        // re-emit Change and write the colour back on every target change.
        swatches = swatches.child(ColorPicker::new(&self.color_picker).label("Custom"));

        // Left sidebar: a recessed, scrollable nav column of identities.
        let sidebar = v_flex()
            .flex_none()
            .w(px(220.0))
            .h_full()
            .p_3()
            .gap_2()
            .bg(theme::bg_base())
            .border_r(px(1.0))
            .border_color(theme::border())
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .px_1()
                    .pb_2()
                    .border_b_1()
                    .border_color(theme::border())
                    .child(Icon::new(IconName::Settings).small().text_color(theme::accent()))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text())
                            .child("Settings"),
                    ),
            )
            .child(
                div()
                    .id("settings-nav-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(nav),
            );

        // Right panel: header (who + close), the scrollable editor fields, and a
        // pinned footer (Reset / Done).
        let header = h_flex()
            .flex_none()
            .items_center()
            .justify_between()
            .child(div().text_color(theme::text_secondary()).child(
                match target {
                    SettingsTarget::General => "General Settings".to_string(),
                    SettingsTarget::Appearance => "Appearance & Typography".to_string(),
                    SettingsTarget::Execution => "Execution & Swarm Limits".to_string(),
                    SettingsTarget::Environment => "Environment & Defaults".to_string(),
                    SettingsTarget::Providers => "Providers".to_string(),
                    SettingsTarget::Skills => "Skills".to_string(),
                    _ => format!("Editing {}", preview.name),
                },
            ))
            .child(
                div()
                    .id("settings-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .rounded_full()
                    .text_color(theme::text_secondary())
                    .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
                    .child(Icon::new(IconName::WindowClose).small())
                    .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx))),
            );

        let fields = match target {
            SettingsTarget::General => self.general_settings_view(window, cx).into_any_element(),
            SettingsTarget::Appearance => self.appearance_settings_view(window, cx).into_any_element(),
            SettingsTarget::Execution => self.execution_settings_view(window, cx).into_any_element(),
            SettingsTarget::Environment => self.environment_settings_view(window, cx).into_any_element(),
            SettingsTarget::Providers => self.providers_view(window, cx).into_any_element(),
            SettingsTarget::Skills => self.skills_settings_view(window, cx).into_any_element(),
            _ => {
                let is_quark = matches!(target, SettingsTarget::Quark(_));
                let acp_quark = matches!(&target, SettingsTarget::Quark(id) if self.is_acp_quark(id));
                let http_quark = matches!(&target, SettingsTarget::Quark(id) if self.is_http_quark(id));

                // 0. Hero Banner Header Card
                let hero_pills = {
                    let mut pills = h_flex().gap_2().items_center().flex_wrap();
                    match &target {
                        SettingsTarget::Human => {
                            pills = pills.child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(theme::bg_surface_raised())
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme::text_secondary())
                                    .child("Human Operator"),
                            );
                        }
                        SettingsTarget::Quark(id) => {
                            if let Some(r) = self.view.roster.iter().find(|r| &r.id == id) {
                                let transport_str = match r.transport {
                                    hadron_lattice::Transport::Acp => "ACP Agent",
                                    hadron_lattice::Transport::Http => "HTTP Provider",
                                    hadron_lattice::Transport::Cli => "CLI Subprocess",
                                    hadron_lattice::Transport::Sdk => "SDK Agent",
                                };
                                pills = pills.child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(theme::bg_surface_raised())
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(theme::text_secondary())
                                        .child(transport_str),
                                );

                                let current_model = self.settings_model.read(cx).value().to_string();
                                let display_model = if current_model.trim().is_empty() {
                                    r.model.clone()
                                } else {
                                    current_model
                                };
                                if !display_model.trim().is_empty() {
                                    pills = pills.child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .bg(theme::bg_surface_raised())
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(theme::text_secondary())
                                            .child(display_model),
                                    );
                                }

                                let mode_str = if r.mode_is_override {
                                    format!("{:?}", r.mode)
                                } else {
                                    "Default".to_string()
                                };
                                pills = pills.child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(theme::bg_surface_raised())
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(theme::accent())
                                        .child(format!("Mode: {mode_str}")),
                                );
                            }
                        }
                        _ => {}
                    }
                    pills
                };

                let hero_card = v_flex()
                    .w_full()
                    .p_4()
                    .gap_3()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::border())
                    .child(
                        div()
                            .w_full()
                            .h(px(3.0))
                            .bg(preview.color)
                            .rounded_t_lg(),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_4()
                            .items_center()
                            .child(identity_avatar(&preview, 56.0))
                            .child(
                                v_flex()
                                    .gap_1()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(theme::text())
                                            .child(preview.name),
                                    )
                                    .child(hero_pills),
                            ),
                    );

                // 1. Profile & Visual Identity Card
                let profile_card = settings_card_section(
                    "Profile & Appearance",
                    Some(IconName::Info),
                    v_flex()
                        .gap_3()
                        .child(settings_field(
                            "Display name",
                            Some("Shown in chat and the roster."),
                            Input::new(&self.settings_name).w_full().into_any_element(),
                        ))
                        .child(settings_field("Color accent", Some("Accent color for identity badges and hero banner."), swatches.into_any_element()))
                        .child(settings_field(
                            "Avatar image",
                            Some("Custom avatar image shown in the roster and chat."),
                            h_flex()
                                .gap_2()
                                .items_center()
                                .w_full()
                                .child(div().flex_1().min_w(px(140.0)).child(Input::new(&self.settings_path).w_full()))
                                .child(text_button("settings-browse-img", "Browse").on_click(
                                    cx.listener(|this, _, _, cx| this.pick_avatar_image(cx)),
                                ))
                                .child(text_button("settings-clear-img", "Clear").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.clear_settings_image(window, cx)
                                    }),
                                ))
                                .into_any_element(),
                        )),
                );

                let mut view = v_flex().gap_4().child(hero_card).child(profile_card);

                if is_quark {
                    // 2. Model & Reasoning Card
                    let model_field = if acp_quark {
                        self.acp_model_select(window, cx)
                    } else if http_quark {
                        self.http_model_select(window, cx)
                    } else {
                        self.general_model_select(window, cx)
                    };

                    let effort_field = if acp_quark {
                        self.acp_effort_select(window, cx)
                    } else {
                        self.general_effort_select(window, cx)
                    };

                    let model_card_content = v_flex()
                        .gap_3()
                        .child(settings_field(
                            "Model",
                            Some("Per-repo override; blank inherits catalogue default."),
                            model_field,
                        ))
                        .when_some(self.agy_bridge_status_row(cx), |v, row| v.child(row))
                        .child(settings_field(
                            "Effort",
                            Some("Reasoning effort spent per turn."),
                            effort_field,
                        ))
                        .when(self.settings_model_params_applies, |v| {
                            v.child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .cursor_pointer()
                                            .id("advanced-model-params-toggle")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.settings_advanced_expanded = !this.settings_advanced_expanded;
                                                cx.notify();
                                            }))
                                            .child(
                                                Icon::new(if self.settings_advanced_expanded {
                                                    IconName::ChevronDown
                                                } else {
                                                    IconName::ChevronRight
                                                })
                                                .small()
                                                .text_color(theme::text_muted()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(theme::text_secondary())
                                                    .child("Advanced Model Parameters"),
                                            ),
                                    )
                                    .when(self.settings_advanced_expanded, |v| {
                                        v.child(
                                            h_flex()
                                                .w_full()
                                                .gap_3()
                                                .pt_2()
                                                .child(div().flex_1().child(settings_field_stacked(
                                                    "Temperature",
                                                    Input::new(&self.settings_temperature).w_full().into_any_element(),
                                                )))
                                                .child(div().flex_1().child(settings_field_stacked(
                                                    "Top P",
                                                    Input::new(&self.settings_top_p).w_full().into_any_element(),
                                                )))
                                                .child(div().flex_1().child(settings_field_stacked(
                                                    "Max Tokens",
                                                    Input::new(&self.settings_max_tokens).w_full().into_any_element(),
                                                ))),
                                        )
                                    }),
                            )
                        });

                    view = view.child(settings_card_section(
                        "Model & Reasoning",
                        Some(IconName::Cpu),
                        model_card_content,
                    ));

                    // 3. Governance & Security Card
                    let gov_card_content = v_flex()
                        .gap_3()
                        .child(settings_field(
                            "Permission",
                            Some("Authority level, from asking every time to full autonomy."),
                            self.mode_select(target.key(), window, cx),
                        ))
                        .child(settings_field_stacked(
                            "Skills",
                            v_flex()
                                .gap_1p5()
                                .child(div().text_xs().text_color(theme::text_muted()).child("Skills active for this quark in the swarm. Unselect a skill to disable (deny) it."))
                                .child(self.skill_selector(cx))
                                .into_any_element(),
                        ))
                        .when(self.settings_secret_applies, |v| {
                            v.child(settings_field(
                                "API key",
                                Some("Stored in OS keychain — never written to team.json."),
                                self.secret_field(cx),
                            ))
                        })
                        .child(settings_field(
                            "Energy limit",
                            Some("Token budget before throttling. Blank = unlimited."),
                            Input::new(&self.settings_energy_limit).w_full().into_any_element(),
                        ));

                    view = view.child(settings_card_section(
                        "Governance & Security",
                        Some(IconName::Settings),
                        gov_card_content,
                    ));
                }

                view.into_any_element()
            }
        };

        // Settings auto-saves on every change (chip clicks commit immediately; free-text
        // fields commit on nav-away or on close via ✕/backdrop — see
        // `commit_settings_inputs`), so there is nothing left for a "Done" button to do
        // that closing the panel any other way doesn't already do.
        let footer = if matches!(
            target,
            SettingsTarget::General
                | SettingsTarget::Appearance
                | SettingsTarget::Execution
                | SettingsTarget::Environment
                | SettingsTarget::Providers
                | SettingsTarget::Skills
        ) {
            div().into_any_element()
        } else {
            h_flex()
                .flex_none()
                .justify_end()
                .pt_1()
                .child(text_button("settings-reset", "Reset to default").on_click(
                    cx.listener(|this, _, window, cx| this.reset_settings_target(window, cx)),
                ))
                .into_any_element()
        };
        let panel = v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .p_4()
            .gap_4()
            .child(header)
            .child(
                div()
                    .id("settings-fields-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(fields),
            )
            .child(footer);

        let card = h_flex()
            .occlude()
            .w_full()
            .h_full()
            .max_w(px(1080.0))
            .max_h(px(720.0))
            .rounded(INNER_RADIUS)
            .overflow_hidden()
            // Opaque: a focused settings modal shouldn't let the bright field bleed through
            // (it read as too transparent). `glass_card()` is ~95% alpha by design (see its
            // `test_translucency_invariants`), which is exactly the readable-through-it bug
            // this comment describes — `modal_surface()` is the token built for this: fully
            // opaque, matching the quark-info/About panels, and previously unwired.
            .bg(theme::modal_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {}) // swallow inner clicks
            .child(sidebar)
            .child(panel);

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .p_8()
            .flex()
            // Center on both axes deterministically (was relying on default
            // align + a top margin, which sank the card to the window's foot).
            .flex_col()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
            )
            .child(card)
    }

    /// One indented sub-row under a category group (e.g. Appearance under General).
    pub(super) fn settings_sub_nav_row(
        &self,
        who: SettingsTarget,
        current: &SettingsTarget,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = &who == current;
        let id = SharedString::from(format!("settings-sub-{}", who.key()));

        let (icon, label) = match &who {
            SettingsTarget::Appearance => (IconName::Palette, "Appearance"),
            SettingsTarget::Execution => (IconName::Cpu, "Execution"),
            SettingsTarget::Environment => (IconName::Settings, "Environment"),
            _ => (IconName::Info, "Details"),
        };

        h_flex()
            .id(id)
            .items_center()
            .w_full()
            .pl(px(24.0))
            .pr_2()
            .py_1()
            .rounded_md()
            .bg(if selected {
                theme::bg_surface_raised()
            } else {
                theme::bg_base()
            })
            .hover(|s| s.bg(theme::bg_surface()))
            .cursor_pointer()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(18.0))
                            .text_color(if selected {
                                theme::accent()
                            } else {
                                theme::text_muted()
                            })
                            .child(Icon::new(icon).xsmall()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_weight(if selected {
                                gpui::FontWeight::SEMIBOLD
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .truncate()
                            .text_color(if selected {
                                theme::text()
                            } else {
                                theme::text_secondary()
                            })
                            .child(label),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_settings_target(who.clone(), window, cx)
            }))
    }

    /// One row in the Settings identity nav: avatar + name + optional status dot,
    /// highlighted when it's the identity currently being edited.
    pub(super) fn settings_nav_row(
        &self,
        who: SettingsTarget,
        current: &SettingsTarget,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let resolved = self.resolve_identity(who.key());
        let selected = &who == current;
        let id = SharedString::from(format!("settings-id-{}", who.key()));

        let (icon_or_avatar, status_dot) = match &who {
            SettingsTarget::General => (
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Settings).small())
                    .into_any_element(),
                None,
            ),
            SettingsTarget::Appearance => (
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Palette).small())
                    .into_any_element(),
                None,
            ),
            SettingsTarget::Execution => (
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Cpu).small())
                    .into_any_element(),
                None,
            ),
            SettingsTarget::Environment => (
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Settings).small())
                    .into_any_element(),
                None,
            ),
            SettingsTarget::Providers => (
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Cpu).small())
                    .into_any_element(),
                None,
            ),
            SettingsTarget::Skills => (
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Folder).small())
                    .into_any_element(),
                None,
            ),
            SettingsTarget::Human => (
                identity_avatar(&resolved, 24.0).into_any_element(),
                None,
            ),
            SettingsTarget::Quark(qid) => {
                let dot = if let Some(r) = self.view.roster.iter().find(|r| &r.id == qid) {
                    let effective_state = effective_presence_state(r.state, r.adopted, r.enabled, false);
                    Some(self.render_halo_dot(effective_state, r.enabled).into_any_element())
                } else {
                    None
                };
                (identity_avatar(&resolved, 24.0).into_any_element(), dot)
            }
        };

        h_flex()
            .id(id)
            .items_center()
            .justify_between()
            .w_full()
            .px_2()
            .py_1p5()
            .rounded_md()
            .bg(if selected {
                theme::bg_surface_raised()
            } else {
                theme::bg_base()
            })
            .hover(|s| s.bg(theme::bg_surface()))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .flex_1()
                    .min_w_0()
                    .child(icon_or_avatar)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .truncate()
                            .text_color(if selected {
                                theme::text()
                            } else {
                                theme::text_secondary()
                            })
                            .child(match &who {
                                SettingsTarget::General => "General".to_string(),
                                SettingsTarget::Appearance => "Appearance".to_string(),
                                SettingsTarget::Execution => "Execution".to_string(),
                                SettingsTarget::Environment => "Environment".to_string(),
                                SettingsTarget::Providers => "Providers".to_string(),
                                SettingsTarget::Skills => "Skills".to_string(),
                                _ => resolved.name.clone(),
                            }),
                    ),
            )
            .when_some(status_dot, |this, dot| this.child(dot))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_settings_target(who.clone(), window, cx)
            }))
    }

    /// Dedicated Skills management view: standard swarm procedures, global & repo skills (with Edit & Delete), and skill creation.
    pub(super) fn skills_settings_view(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let skills = self.loaded_skills();

        // 1. Standard Skills Accordion Card
        let builtins = hadron_gluon::skills::builtins();
        let builtins_count = builtins.len();

        let standard_section = v_flex()
            .w_full()
            .p_4()
            .gap_3()
            .rounded_lg()
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::border())
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .id("standard-skills-accordion-toggle")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings_standard_skills_expanded = !this.settings_standard_skills_expanded;
                        cx.notify();
                    }))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(IconName::CircleCheck).small().text_color(theme::accent()))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child("Standard Swarm Skills"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .bg(theme::bg_surface_raised())
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(format!("{builtins_count}")),
                            ),
                    )
                    .child(
                        Icon::new(if self.settings_standard_skills_expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .small()
                        .text_color(theme::text_muted()),
                    ),
            )
            .when(self.settings_standard_skills_expanded, |this| {
                let mut standard_list = v_flex().gap_2().w_full();
                for b in &builtins {
                    let desc = b.description.as_deref().unwrap_or("Standard swarm procedure");
                    standard_list = standard_list.child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .p_2()
                            .rounded_md()
                            .bg(theme::bg_surface_raised())
                            .border_1()
                            .border_color(theme::border())
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .bg(theme::glass_card())
                                            .border_1()
                                            .border_color(theme::accent())
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(theme::accent())
                                            .flex_none()
                                            .child(b.id.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_secondary())
                                            .truncate()
                                            .child(desc.to_string()),
                                    ),
                            ),
                    );
                }
                this.child(
                    v_flex()
                        .gap_3()
                        .w_full()
                        .pt_1()
                        .border_t_1()
                        .border_color(theme::border())
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child("Core procedures built into Hadron swarm orchestrator and quarks:"),
                        )
                        .child(standard_list),
                )
            });

        // 2. Installed Skills Card
        let mut skills_list = v_flex().gap_2();
        if skills.is_empty() {
            skills_list = skills_list.child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("No custom skills found. Create a global (~/.hadron/skills/) or per-repo (.hadron/skills/) skill below."),
            );
        } else {
            for (idx, s_item) in skills.iter().enumerate() {
                let skill_name = s_item.name.clone();
                let skill_path = s_item.path.clone();
                let is_global = s_item.is_global;
                let scope_label = if is_global {
                    "global (~/.hadron/skills/)"
                } else {
                    "repo (.hadron/skills/)"
                };

                let path_to_open = skill_path.clone();
                let path_to_del = skill_path.clone();

                skills_list = skills_list.child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .p_2()
                        .rounded_md()
                        .bg(theme::bg_surface())
                        .border_1()
                        .border_color(theme::border())
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(theme::glass_card())
                                        .border_1()
                                        .border_color(theme::accent())
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(theme::accent())
                                        .child(skill_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(scope_label),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    text_button(SharedString::from(format!("edit-skill-{idx}")), "Edit")
                                        .on_click(cx.listener(move |this, _, _window, cx| {
                                            this.open_in_editor(&path_to_open, None);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    text_button(SharedString::from(format!("del-skill-{idx}")), "Delete")
                                        .on_click(cx.listener(move |this, _, _window, cx| {
                                            this.delete_skill_file(&path_to_del);
                                            cx.notify();
                                        })),
                                ),
                        ),
                );
            }
        }

        let skills_section = settings_card_section(
            "Installed Custom Skills",
            Some(IconName::Folder),
            skills_list,
        );

        // 3. Create New Skill Card
        let f_repo = self.settings_new_role.clone();
        let f_global = self.settings_new_role.clone();

        let create_content = v_flex()
            .gap_3()
            .w_full()
            .child(Input::new(&self.settings_new_role).w_full())
            .child(
                h_flex()
                    .gap_3()
                    .w_full()
                    .child(
                        div()
                            .id("create-repo-skill-btn")
                            .flex_1()
                            .py_2()
                            .px_3()
                            .rounded_md()
                            .border_1()
                            .border_color(theme::border())
                            .bg(theme::bg_surface_raised())
                            .hover(|s| s.bg(theme::glass_card()).border_color(theme::accent()))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .child(Icon::new(IconName::Folder).small().text_color(theme::accent()))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme::text())
                                    .child("Repo Skill"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child("(.hadron/skills)"),
                            )
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                let val = f_repo.read(cx).value().trim().to_string();
                                if !val.is_empty() {
                                    if let Some(path) = this.add_custom_skill(&val, false) {
                                        f_repo.update(cx, |s, cx| s.set_value("", _window, cx));
                                        this.open_in_editor(&path, None);
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("create-global-skill-btn")
                            .flex_1()
                            .py_2()
                            .px_3()
                            .rounded_md()
                            .border_1()
                            .border_color(theme::border())
                            .bg(theme::bg_surface_raised())
                            .hover(|s| s.bg(theme::glass_card()).border_color(theme::accent()))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .child(Icon::new(IconName::Folder).small().text_color(theme::accent()))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme::text())
                                    .child("Global Skill"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child("(~/.hadron/skills)"),
                            )
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                let val = f_global.read(cx).value().trim().to_string();
                                if !val.is_empty() {
                                    if let Some(path) = this.add_custom_skill(&val, true) {
                                        f_global.update(cx, |s, cx| s.set_value("", _window, cx));
                                        this.open_in_editor(&path, None);
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            );

        let create_section = settings_card_section(
            "Create New Skill",
            Some(IconName::Plus),
            create_content,
        );

        v_flex()
            .gap_4()
            .child(standard_section)
            .child(skills_section)
            .child(create_section)
    }
}
