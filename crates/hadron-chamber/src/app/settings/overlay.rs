use super::*;

impl super::Chamber {
    // `pub(crate)`, not `pub(super)`: called from `app::render::mod` (a sibling of
    // `settings`), the same reach it had when this fn lived directly in settings.rs.
    pub(crate) fn settings_overlay(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.settings_target.clone();

        // Left nav: every editable identity — the human, then each quark.
        let mut nav = v_flex()
            .gap_0p5()
            .child(
                div()
                    .px_1()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("GLOBAL"),
            )
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
                    .px_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("SETTINGS"),
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
                if target == SettingsTarget::Providers {
                    "Providers".to_string()
                } else {
                    format!("Editing {}", preview.name)
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

        let fields = if target == SettingsTarget::Providers {
            self.providers_view(cx).into_any_element()
        } else {
            let is_quark = matches!(target, SettingsTarget::Quark(_));
            // ACP quarks get a live model dropdown (re-probed from the agent) in place of
            // the free-text Model box; everything else keeps the text field.
            let acp_quark = matches!(&target, SettingsTarget::Quark(id) if self.is_acp_quark(id));
            v_flex()
                .gap_4()
                .child(settings_field("Preview", preview_row.into_any_element()))
                .child(settings_field(
                    "Display name",
                    Input::new(&self.settings_name).into_any_element(),
                ))
                // Model + Effort configure an agent's session, so they are quark-only. The
                // human has no such controls. Model here is a **per-repo** override: blank
                // inherits the catalogue default (e.g. acp-claude = Opus), a value pins this
                // repo (= Sonnet) without touching the shared catalogue or any other repo.
                //
                // Permission is the single authority control — it REPLACES the old ACP
                // "Mode" field (default/plan/acceptEdits/bypassPermissions), which set the
                // same posture axis by a cruder route. The ladder subsumes it: it is live,
                // turn-granular, and applied over any boot-time `mode_config` per turn. The
                // `mode_config` seat field is intentionally left in place (still round-trips
                // through load/commit) but is no longer human-editable here.
                .when(is_quark, |v| {
                    let model_field = if acp_quark {
                        self.acp_model_select(cx)
                    } else {
                        Input::new(&self.settings_model).into_any_element()
                    };
                    v.child(settings_field("Model", model_field))
                    .child(settings_field(
                        "Effort",
                        self.session_select(
                            "effort",
                            &self.settings_effort,
                            &["low", "medium", "high"],
                            cx,
                        ),
                    ))
                    // The permission ladder: how much authority the human delegates to
                    // this quark (Ask → Bypass). Stored on the field as a per-quark
                    // `ModeSet`, so it is live-honoured and independent of team.json. A
                    // per-quark choice persists even when the global default later changes.
                    .child(settings_field("Permission", self.mode_select(target.key(), cx)))
                    .child(settings_field(
                        "Roles",
                        self.role_selector(cx),
                    ))
                    .child(settings_field(
                        "Denied skills",
                        Input::new(&self.settings_deny_skills).into_any_element(),
                    ))
                    // The secret env-var value (e.g. `GEMINI_API_KEY`) goes to the OS
                    // keychain via `SecretStore`, never into team.json or this panel's
                    // rendered state — see `secret_field`. Shown ONLY for a quark whose
                    // provider actually needs a key (per the catalogue), not universally.
                    .when(self.settings_secret_applies, |v| {
                        v.child(settings_field("API key", self.secret_field(cx)))
                    })
                })
                .child(settings_field("Color", swatches.into_any_element()))
                .child(settings_field(
                    "Image",
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
                ))
                .into_any_element()
        };

        let footer = if target == SettingsTarget::Providers {
            div().into_any_element()
        } else {
            h_flex()
                .flex_none()
                .justify_between()
                .pt_1()
                .child(text_button("settings-reset", "Reset to default").on_click(
                    cx.listener(|this, _, window, cx| this.reset_settings_target(window, cx)),
                ))
                .child(
                    div()
                        .id("settings-done")
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme::accent())
                        .text_color(theme::text())
                        .hover(|s| s.opacity(0.9))
                        .active(|s| s.opacity(0.8))
                        .child("Done")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                        ),
                )
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
            .max_w(px(960.0))
            .max_h(px(640.0))
            .rounded_lg()
            .overflow_hidden()
            // Opaque: a focused settings modal shouldn't let the bright field bleed through
            // (it read as too transparent). Solid, not glass — shared with the info panel.
            .bg(theme::modal_surface())
            .border_1()
            .border_color(theme::glass_highlight())
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
            .bg(rgba(0x00000088))
            .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
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
            .child(if who == SettingsTarget::Providers {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Cpu).small())
                    .into_any_element()
            } else {
                identity_avatar(&resolved, 24.0).into_any_element()
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
                    .child(if who == SettingsTarget::Providers {
                        "Providers".to_string()
                    } else {
                        resolved.name.clone()
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_settings_target(who.clone(), window, cx)
            }))
    }
}
