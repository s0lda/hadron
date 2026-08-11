use super::*;

impl super::Chamber {
    // `pub(crate)`, not `pub(super)`: called from `app::render::mod` (a sibling of
    // `settings`), the same reach it had when this fn lived directly in settings.rs.
    pub(crate) fn settings_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.settings_target.clone();

        // Left nav: Settings (General, Providers), then Identities (Human, Quarks)
        let mut nav = v_flex()
            .gap_0p5()
            .child(
                div()
                    .px_1()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("SETTINGS"),
            )
            .child(self.settings_nav_row(SettingsTarget::General, &target, cx))
            .child(self.settings_nav_row(SettingsTarget::Providers, &target, cx))
            .child(
                div()
                    .px_1()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("IDENTITIES"),
            )
            .child(self.settings_nav_row(SettingsTarget::Human, &target, cx));
        for r in &self.view.roster {
            nav =
                nav.child(self.settings_nav_row(SettingsTarget::Quark(r.id.clone()), &target, cx));
        }

        // Live preview: the target resolved, but with the in-progress name/image
        // from the inputs so it tracks typing.
        let live_name = self.settings_name.read(cx).value().to_string();
        let live_path = self.settings_path.read(cx).value().trim().to_string();
        let mut preview = self.resolve_identity(target.key());
        if !live_name.trim().is_empty() {
            preview.name = live_name;
        }
        preview.image = (!live_path.is_empty()).then_some(live_path);
        let preview_row = h_flex()
            .items_center()
            .gap_3()
            .child(identity_avatar(&preview, 44.0))
            .child(div().text_color(preview.color).child(preview.name.clone()));

        // Color swatches; the stored color (if any) gets a bright ring.
        let selected = self.settings_color();
        let mut swatches = h_flex().gap_2().flex_wrap();
        for hex in IDENTITY_SWATCHES {
            let is_sel = selected.as_deref() == Some(format!("#{hex:06x}").as_str());
            swatches = swatches.child(
                div()
                    .id(SharedString::from(format!("swatch-{hex:06x}")))
                    .size(px(22.0))
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
            .w(px(190.0))
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::bg_base())
            .border_r(px(1.0))
            .border_color(theme::border())
            .child(div().px_1().text_color(theme::text()).child("Settings"))
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
                    SettingsTarget::Providers => "Providers".to_string(),
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
            SettingsTarget::General => self.general_settings_view(cx).into_any_element(),
            SettingsTarget::Providers => self.providers_view(window, cx).into_any_element(),
            _ => {
                let is_quark = matches!(target, SettingsTarget::Quark(_));
                let acp_quark = matches!(&target, SettingsTarget::Quark(id) if self.is_acp_quark(id));
                let http_quark = matches!(&target, SettingsTarget::Quark(id) if self.is_http_quark(id));

                // 1. Profile & Visual Identity Card
                let profile_card = settings_card_section(
                    "Profile & Appearance",
                    Some(IconName::Info),
                    v_flex()
                        .gap_3()
                        .child(settings_field("Preview", None, preview_row.into_any_element()))
                        .child(settings_field(
                            "Display name",
                            Some("Shown in chat and the roster."),
                            Input::new(&self.settings_name).w_full().into_any_element(),
                        ))
                        .child(settings_field("Color", Some("Accent color for identity badges."), swatches.into_any_element()))
                        .child(settings_field(
                            "Image",
                            Some("Avatar shown in the roster and chat."),
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(div().flex_1().child(Input::new(&self.settings_path)))
                                .child(text_button("settings-browse-img", "Browse…").on_click(
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

                let mut view = v_flex().gap_4().child(profile_card);

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
                        self.acp_effort_select(cx)
                    } else {
                        self.session_select(
                            "effort",
                            &self.settings_effort,
                            &["low", "medium", "high"],
                            cx,
                        )
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
                                            .id("advanced-model-params-toggle")
                                            .justify_between()
                                            .items_center()
                                            .cursor_pointer()
                                            .py_1()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.settings_advanced_expanded = !this.settings_advanced_expanded;
                                                cx.notify();
                                            }))
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        Icon::new(if self.settings_advanced_expanded {
                                                            IconName::ChevronDown
                                                        } else {
                                                            IconName::ChevronRight
                                                        })
                                                        .small()
                                                        .text_color(theme::text_secondary()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(gpui::FontWeight::MEDIUM)
                                                            .text_color(theme::text())
                                                            .child("Advanced Model Parameters"),
                                                    ),
                                            ),
                                    )
                                    .when(self.settings_advanced_expanded, |adv| {
                                        adv.child(settings_field(
                                            "Temperature",
                                            Some("Sampling temperature (e.g. 0.1 for code, 0.8 for creative). Blank = default."),
                                            Input::new(&self.settings_temperature).w_full().into_any_element(),
                                        ))
                                        .child(settings_field(
                                            "Top P",
                                            Some("Nucleus sampling probability (e.g. 0.95). Blank = default."),
                                            Input::new(&self.settings_top_p).w_full().into_any_element(),
                                        ))
                                        .child(settings_field(
                                            "Max tokens",
                                            Some("Max response token limit. Blank = default."),
                                            Input::new(&self.settings_max_tokens).w_full().into_any_element(),
                                        ))
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
                            self.mode_select(target.key(), cx),
                        ))
                        .child(settings_field(
                            "Roles",
                            Some("Roles this quark takes on in the swarm."),
                            self.role_selector(cx),
                        ))
                        .child(settings_field(
                            "Denied skills",
                            Some("Comma-separated skill names this quark may not invoke."),
                            Input::new(&self.settings_deny_skills).w_full().into_any_element(),
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
        let footer = if target == SettingsTarget::Providers {
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

    /// One row in the Settings identity nav: avatar + name, highlighted when it's
    /// the identity currently being edited.
    pub(super) fn settings_nav_row(
        &self,
        who: SettingsTarget,
        current: &SettingsTarget,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let resolved = self.resolve_identity(who.key());
        let selected = &who == current;
        let id = SharedString::from(format!("settings-id-{}", who.key()));
        h_flex()
            .id(id)
            .items_center()
            .gap_2()
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
            .child(match &who {
                SettingsTarget::General => div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Settings).small())
                    .into_any_element(),
                SettingsTarget::Providers => div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Cpu).small())
                    .into_any_element(),
                _ => identity_avatar(&resolved, 24.0).into_any_element(),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(if selected {
                        theme::text()
                    } else {
                        theme::text_secondary()
                    })
                    .child(match &who {
                        SettingsTarget::General => "General".to_string(),
                        SettingsTarget::Providers => "Providers".to_string(),
                        _ => resolved.name.clone(),
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_settings_target(who.clone(), window, cx)
            }))
    }
}
