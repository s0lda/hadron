use super::*;
use crate::app::providers::{nucleus_budget_kb_for, CLOUD_API_KEY_VAR, NUCLEUS_BUDGET_LADDER_KB};

/// First free seat id: the conventional one, else `<base>-2`, `-3`, … A second
/// seat of the same provider is a real, wanted thing (same vendor, different
/// model — "Claude on Fable" next to "Claude on Opus"), so a collision must mint
/// a NEW id rather than silently re-adopting the existing seat.
pub(super) fn unique_seat_id(base: &str, taken: &dyn Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2u32..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .expect("an unbounded counter always finds a free id")
}

impl super::Chamber {
    /// Connect = GET the vendor's model-list endpoint, off the UI thread — a slow
    /// or unreachable local server must not freeze the window. Mirrors
    /// `start_acp_model_probe`'s exact shape (a method, not an inline closure —
    /// GPUI's `Fn`-bound `cx.listener` closure fights the borrow checker over a
    /// nested `cx.spawn` in ways a plain method body does not).
    pub(super) fn start_local_provider_probe(
        &mut self,
        vendor: hadron_gluon::adapter::local::HttpVendor,
        base_url: String,
        api_key: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.wizard_state = WizardState::LocalProvider(vendor, ProviderState::Connecting);
        cx.notify();

        let target = hadron_gluon::adapter::local::HttpTarget { vendor, base_url: base_url.clone(), api_key: api_key.clone() };
        let base_url_for_task = base_url.clone();
        let api_key_for_task = api_key.clone();
        cx.spawn(move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = cx
                    .background_spawn(async move { hadron_gluon::adapter::local::fetch_models(&target) })
                    .await
                    .map_err(|e| e.to_string());

                this.update(&mut cx, |this, cx| {
                    // Only the still-open probe may write its result — the human may have
                    // backed out of this wizard entirely while the request was in flight.
                    if !matches!(&this.wizard_state, WizardState::LocalProvider(v, _) if *v == vendor) {
                        return;
                    }
                    match result {
                        Ok(models) if !models.is_empty() => {
                            let ids: Vec<String> = models.into_iter().map(|m| m.id).collect();
                            let first = ids[0].clone();
                            this.local_models = ids;
                            this.local_selected_model = Some(first.clone());

                            if !vendor.requires_api_key() {
                                // 1-Click zero-friction add for local servers (Ollama, LM Studio)
                                this.save_and_add_http_quark(vendor, &base_url_for_task, &first, api_key_for_task, cx);
                            } else {
                                this.wizard_state = WizardState::LocalProvider(
                                    vendor,
                                    ProviderState::Ready { model: first },
                                );
                                cx.notify();
                            }
                        }
                        Ok(_) => {
                            this.wizard_state = WizardState::LocalProvider(
                                vendor,
                                ProviderState::Failed(format!(
                                    "connected, but {} has no models loaded",
                                    vendor.display_name()
                                )),
                            );
                            cx.notify();
                        }
                        Err(e) => {
                            this.wizard_state = WizardState::LocalProvider(
                                vendor,
                                ProviderState::Failed(e),
                            );
                            cx.notify();
                        }
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// Connect and probe an ACP agent in the background, automatically saving and adopting
    /// the quark upon successful model detection without requiring manual confirmation.
    pub(super) fn start_acp_preset_probe(
        &mut self,
        desc: AgentDescriptor,
        cx: &mut Context<Self>,
    ) {
        self.wizard_state = WizardState::Connecting(desc.clone(), ProviderState::Connecting);
        cx.notify();

        let target = hadron_gluon::adapter::registry::AcpTarget {
            program: desc.command.clone(),
            args: desc.args.clone(),
            env: Vec::new(),
        };
        let desc_for_task = desc.clone();
        cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = cx
                    .background_spawn(async move {
                        hadron_gluon::adapter::acp::probe(&target)
                    })
                    .await
                    .map_err(|e| e.to_string());

                this.update(&mut cx, |this, cx| {
                    if !matches!(&this.wizard_state, WizardState::Connecting(d, _) if d.id == desc_for_task.id) {
                        return;
                    }
                    match result {
                        Ok(model) => {
                            this.save_and_add_acp_quark(&desc_for_task, &model, cx);
                        }
                        Err(e) => {
                            this.wizard_state = WizardState::Connecting(
                                desc_for_task,
                                ProviderState::Failed(e),
                            );
                            cx.notify();
                        }
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// Save a probed ACP seat directly into the global catalogue and current repo team.
    pub(super) fn save_and_add_acp_quark(
        &mut self,
        desc: &AgentDescriptor,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        let base_id = hadron_lattice::Transport::Acp.conventional_id(&desc.id);
        let seat_id = {
            let taken = |id: &str| {
                self.providers.iter().any(|p| p.id == id)
                    || self.global.quarks.iter().any(|s| s.id.as_str() == id)
                    || self.team.quarks.iter().any(|s| s.id.as_str() == id)
                    || self.team.roster.iter().any(|o| o.id.as_str() == id)
            };
            unique_seat_id(&base_id, &taken)
        };

        self.providers.push(ConfiguredQuark {
            id: seat_id.clone(),
            transport: "acp".to_string(),
            model: model.to_string(),
        });

        let mut seat = hadron_lattice::Seat {
            id: hadron_lattice::QuarkId::new(&seat_id),
            display_name: None,
            vendor: desc.id.clone(),
            model: model.to_string(),
            flavor: hadron_lattice::Flavor::Worker,
            transport: hadron_lattice::Transport::Acp,
            command: Some(hadron_lattice::AcpCommand {
                program: desc.command.clone(),
                args: desc.args.clone(),
            }),
            cli: None,
            enabled: true,
            effort: None,
            mode_config: None,
            roles: vec![],
            exclusive: false,
            commands: hadron_lattice::SeatCommands::default(),
            secret_env: Vec::new(),
            energy_limit: None,
            deny_skills: vec![],
            external_roots: vec![],
            http_base_url: None,
            model_params: hadron_lattice::ModelParams::default(),
        };
        seat.normalize_vendor();
        if !hadron_lattice::id_follows_convention(seat.id.as_str(), seat.transport) {
            eprintln!(
                "chamber: note — id '{}' does not match the '{}-' convention",
                seat.id.as_str(),
                seat.transport.code()
            );
        }
        self.add_configured_quark(seat, cx);
        self.wizard_state = WizardState::None;
        cx.notify();
    }

    /// Save a configured HTTP seat directly into the global catalogue and current repo team.
    pub(super) fn save_and_add_http_quark(
        &mut self,
        vendor: hadron_gluon::adapter::local::HttpVendor,
        base_url: &str,
        model: &str,
        api_key: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let base_id = hadron_lattice::Transport::Http.conventional_id(vendor.code());
        let seat_id = {
            let taken = |id: &str| {
                self.providers.iter().any(|p| p.id == id)
                    || self.global.quarks.iter().any(|s| s.id.as_str() == id)
                    || self.team.quarks.iter().any(|s| s.id.as_str() == id)
                    || self.team.roster.iter().any(|o| o.id.as_str() == id)
            };
            unique_seat_id(&base_id, &taken)
        };
        let qid = hadron_lattice::QuarkId::new(&seat_id);

        let mut secret_env = Vec::new();
        if vendor.requires_api_key() {
            if let Some(ref key) = api_key {
                if !key.is_empty() {
                    match self.secret_store.set(&qid, CLOUD_API_KEY_VAR, key) {
                        Ok(()) => secret_env.push(CLOUD_API_KEY_VAR.to_string()),
                        Err(e) => eprintln!(
                            "chamber: failed to write API key to the OS credential store: {e}"
                        ),
                    }
                }
            }
        }

        self.providers.push(ConfiguredQuark {
            id: seat_id.clone(),
            transport: "http".to_string(),
            model: model.to_string(),
        });

        let mut seat = hadron_lattice::Seat {
            id: qid,
            display_name: None,
            vendor: vendor.code().to_string(),
            model: model.to_string(),
            flavor: hadron_lattice::Flavor::Worker,
            transport: hadron_lattice::Transport::Http,
            command: None,
            cli: None,
            enabled: true,
            effort: None,
            mode_config: None,
            roles: vec![],
            exclusive: false,
            commands: hadron_lattice::SeatCommands::default(),
            secret_env,
            energy_limit: None,
            deny_skills: vec![],
            external_roots: vec![],
            http_base_url: Some(base_url.to_string()),
            model_params: hadron_lattice::ModelParams::default(),
        };
        seat.normalize_vendor();
        self.add_configured_quark(seat, cx);
        self.wizard_state = WizardState::None;
        cx.notify();
    }

    pub(super) fn appearance_settings_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_custom_active = self.prefs.custom_theme.is_some();
        let active_theme_label = if let Some(custom) = &self.prefs.custom_theme {
            format!("Custom: {}", custom.name)
        } else {
            self.prefs.theme_preset.unwrap_or_default().label().to_string()
        };

        let typography_card = settings_card_section(
            "Typography & Appearance",
            Some(IconName::Palette),
            v_flex()
                .gap_3()
                .child(settings_field(
                    "Color theme preset",
                    Some("Curated dark surfaces (Obsidian Neutral, OLED True Black, Midnight Slate, Tokyo Dark) or custom themes."),
                    self.theme_preset_select(window, cx),
                ))
                .child(settings_field(
                    "Primary accent color",
                    Some("Accent hue for active indicators, focus outlines, and badges."),
                    self.accent_choice_select(window, cx),
                ))
                .child(settings_field(
                    "UI font family",
                    Some("Font used across buttons, menus, labels, and chat prose. Verified regular + bold faces."),
                    self.ui_font_select(window, cx),
                ))
                .child(settings_field(
                    "UI font size",
                    Some("Base font size for application interface elements."),
                    self.ui_font_size_select(window, cx),
                ))
                .child(settings_field(
                    "Code / Monospace font family",
                    Some("Font used for terminals, diffs, hashes, and code blocks."),
                    self.mono_font_select(window, cx),
                ))
                .child(settings_field(
                    "Code / Monospace font size",
                    Some("Font size for code blocks, terminal grid, and inspector diffs."),
                    self.mono_font_size_select(window, cx),
                )),
        );

        let custom_theme_card = settings_card_section(
            "Custom Theme Engine & Palettes",
            Some(IconName::Palette),
            v_flex()
                .gap_3()
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).text_color(theme::text()).child("Active Palette:"))
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(theme::bg_surface_raised())
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(theme::accent())
                                        .child(active_theme_label),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .when(!is_custom_active, |this| {
                                    this.child(
                                        text_button("btn-create-theme", "Create Custom Theme")
                                            .bg(theme::bg_surface_raised())
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                let base_preset = this.prefs.theme_preset.unwrap_or_default();
                                                let mut new_theme = config::ThemeDefinition::from_preset(base_preset);
                                                new_theme.id = format!("custom-{}", chrono::Utc::now().timestamp());
                                                new_theme.name = format!("Custom {}", base_preset.label());
                                                let _ = theme::save_custom_theme(&new_theme);
                                                this.prefs.custom_theme = Some(new_theme);
                                                this.prefs.theme_preset = None;
                                                let _ = config::save(&this.prefs);
                                                Self::apply_theme_and_typography(cx, &this.prefs);
                                                this.show_toast(
                                                    toasts::ToastKind::Success,
                                                    "Created new custom theme",
                                                    Some(3),
                                                    cx,
                                                );
                                                cx.notify();
                                            })),
                                    )
                                })
                                .when(is_custom_active, |this| {
                                    this.child(
                                        text_button("btn-save-theme", "Save to ~/.hadron/themes")
                                            .bg(theme::bg_surface_raised())
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                if let Some(custom) = &this.prefs.custom_theme {
                                                    let _ = theme::save_custom_theme(custom);
                                                    this.show_toast(
                                                        toasts::ToastKind::Success,
                                                        format!("Saved theme '{}'", custom.name),
                                                        Some(3),
                                                        cx,
                                                    );
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        text_button("btn-reset-theme", "Reset to Built-in Preset")
                                            .bg(theme::bg_surface_raised())
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                this.prefs.custom_theme = None;
                                                this.prefs.theme_preset = Some(config::ThemePreset::Obsidian);
                                                let _ = config::save(&this.prefs);
                                                Self::apply_theme_and_typography(cx, &this.prefs);
                                                this.show_toast(
                                                    toasts::ToastKind::Success,
                                                    "Reset to Obsidian Neutral preset",
                                                    Some(3),
                                                    cx,
                                                );
                                                cx.notify();
                                            })),
                                    )
                                }),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(div().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme::text_muted()).child("PALETTE SWATCHES & LIVE TOKEN VALUES"))
                        .child(
                            h_flex()
                                .w_full()
                                .flex_wrap()
                                .gap_2()
                                .child(swatch_chip("Canvas", theme::canvas_base().into()))
                                .child(swatch_chip("Surface", theme::bg_surface()))
                                .child(swatch_chip("Raised", theme::bg_surface_raised()))
                                .child(swatch_chip("Border", theme::border()))
                                .child(swatch_chip("Accent", theme::accent()))
                                .child(swatch_chip("Keyword", theme::syntax_keyword()))
                                .child(swatch_chip("Function", theme::syntax_function()))
                                .child(swatch_chip("Type", theme::syntax_type()))
                                .child(swatch_chip("String", theme::syntax_string()))
                                .child(swatch_chip("Number", theme::syntax_number()))
                                .child(swatch_chip("Comment", theme::syntax_comment()))
                                .child(swatch_chip("Prompt", theme::term_prompt()))
                        ),
                ),
        );

        let mono_family_name = self.prefs.mono_font_family.clone().unwrap_or_else(|| "Cascadia Code".into());
        let live_preview_card = settings_card_section(
            "Live UI & Syntax Preview",
            Some(IconName::Eye),
            v_flex()
                .w_full()
                .gap_3()
                .child(
                    // Code snippet syntax preview
                    v_flex()
                        .w_full()
                        .p_3()
                        .rounded_md()
                        .bg(theme::term_bg())
                        .border_1()
                        .border_color(theme::border())
                        .font_family(mono_family_name)
                        .text_xs()
                        .gap_1()
                        .child(
                            div()
                                .text_color(theme::syntax_comment())
                                .child("// Dynamic syntax token & font preview"),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .child(div().text_color(theme::syntax_keyword()).child("pub async fn"))
                                .child(div().text_color(theme::syntax_function()).child("launch_swarm"))
                                .child(div().text_color(theme::syntax_punctuation()).child("("))
                                .child(div().text_color(theme::syntax_variable()).child("id"))
                                .child(div().text_color(theme::syntax_punctuation()).child(": &"))
                                .child(div().text_color(theme::syntax_type()).child("str"))
                                .child(div().text_color(theme::syntax_punctuation()).child(") -> "))
                                .child(div().text_color(theme::syntax_type()).child("Result<Quark>"))
                                .child(div().text_color(theme::syntax_punctuation()).child(" {")),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .pl_4()
                                .child(div().text_color(theme::syntax_keyword()).child("let"))
                                .child(div().text_color(theme::syntax_variable()).child("config"))
                                .child(div().text_color(theme::syntax_operator()).child("="))
                                .child(div().text_color(theme::syntax_string()).child("\"hadron/orch.json\""))
                                .child(div().text_color(theme::syntax_punctuation()).child(";")),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .pl_4()
                                .child(div().text_color(theme::syntax_type()).child("Quark"))
                                .child(div().text_color(theme::syntax_punctuation()).child("::"))
                                .child(div().text_color(theme::syntax_function()).child("spawn"))
                                .child(div().text_color(theme::syntax_punctuation()).child("("))
                                .child(div().text_color(theme::syntax_variable()).child("id"))
                                .child(div().text_color(theme::syntax_punctuation()).child(", "))
                                .child(div().text_color(theme::syntax_number()).child("42"))
                                .child(div().text_color(theme::syntax_punctuation()).child(")")),
                        )
                        .child(div().text_color(theme::syntax_punctuation()).child("}")),
                )
                .child(
                    // Chamber UI Components Preview
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .p_3()
                        .rounded_md()
                        .bg(theme::bg_base())
                        .border_1()
                        .border_color(theme::border())
                        .child(
                            h_flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_md()
                                        .bg(theme::accent())
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(0x050505))
                                        .child("Active Accent Button"),
                                )
                                .child(
                                    div()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_md()
                                        .bg(theme::bg_surface_raised())
                                        .border_1()
                                        .border_color(theme::border())
                                        .text_xs()
                                        .text_color(theme::text())
                                        .child("Secondary Control"),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(div().text_xs().text_color(theme::text_muted()).child("Status Indicators:"))
                                .child(div().size(px(8.0)).rounded_full().bg(theme::halo_dot(QuarkState::Ground)))
                                .child(div().size(px(8.0)).rounded_full().bg(theme::halo_dot(QuarkState::Thinking)))
                                .child(div().size(px(8.0)).rounded_full().bg(theme::halo_dot(QuarkState::Excited)))
                                .child(div().size(px(8.0)).rounded_full().bg(theme::halo_dot(QuarkState::Blocked))),
                        ),
                ),
        );

        v_flex()
            .w_full()
            .gap_4()
            .child(typography_card)
            .child(custom_theme_card)
            .child(live_preview_card)
    }

    pub(super) fn execution_settings_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let execution_card = settings_card_section(
            "Execution & Swarm Limits",
            Some(IconName::Cpu),
            v_flex()
                .gap_3()
                .child(settings_field(
                    "Max exchanges",
                    Some(
                        "Caps quark↔quark exchanges before the swarm stops. \
                         Blank or 0 = daemon default (12).",
                    ),
                    Input::new(&self.settings_max_exchanges).w_full().into_any_element(),
                ))
                .child(settings_field(
                    "Nucleus index budget",
                    Some(
                        "How big .hadron/nucleus/index.md may grow before a quark is shown \
                         counts instead of the index.",
                    ),
                    self.nucleus_budget_select(window, cx),
                ))
                .child(settings_field(
                    "Merge strategy",
                    Some(
                        "Strategy for landing merged quark branches onto main: Fast-forward (default), Squash commit, or GitHub PR mirror.",
                    ),
                    self.merge_strategy_select(window, cx),
                )),
        );

        v_flex().w_full().gap_4().child(execution_card)
    }

    pub(super) fn environment_settings_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let environment_card = settings_card_section(
            "Environment & Defaults",
            Some(IconName::Settings),
            v_flex()
                .gap_3()
                .child(settings_field(
                    "Code editor",
                    Some(
                        "Which program opens a file you click (file:// link or file tree). \
                         System default uses desktop default (xdg-open).",
                    ),
                    self.editor_select(window, cx),
                ))
                .child(settings_field(
                    "Default permission mode",
                    Some(
                        "The mode a new session starts on. /clear wipes the field and seeds this default.",
                    ),
                    self.default_mode_select(window, cx),
                ))
                .child(settings_field(
                    "Close Gluon on Exit",
                    Some("Terminate the hadron-gluon daemon when the Chamber window closes."),
                    Switch::new("close-gluon-on-exit")
                        .checked(self.prefs.close_gluon_on_exit)
                        .on_click(cx.listener(|this, checked, _window, cx| {
                            this.prefs.close_gluon_on_exit = *checked;
                            let _ = config::save(&this.prefs);
                            cx.notify();
                        }))
                        .into_any_element(),
                )),
        );

        v_flex().w_full().gap_4().child(environment_card)
    }

    pub(super) fn general_settings_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = self.appearance_settings_view(window, cx);
        let execution = self.execution_settings_view(window, cx);
        let environment = self.environment_settings_view(window, cx);

        v_flex()
            .w_full()
            .gap_4()
            .child(typography)
            .child(execution)
            .child(environment)
    }

    /// The default-permission-mode picker using native Select dropdown component
    /// with mode color coding.
    pub(super) fn default_mode_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current = self.prefs.default_mode;
        if self.default_mode_select_key != Some(current) {
            self.default_mode_select_key = Some(current);
            let modes = vec![
                "Ask".to_string(),
                "Write".to_string(),
                "Auto".to_string(),
                "Bypass".to_string(),
            ];
            let current_str = widgets::mode_label(current);
            let delegate = create_model_delegate(&current_str, &modes, Some(&current_str));
            self.default_mode_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_str.into(), window, cx);
            });
        }
        v_flex()
            .gap_1p5()
            .w_full()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(140.0))
                            .child(
                                Select::new(&self.default_mode_select_state)
                                    .w_full()
                                    .placeholder("Select default mode..."),
                            ),
                    )
                    .child(widgets::mode_tag(current, false)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(widgets::mode_hint(current)),
            )
            .into_any_element()
    }

    /// The code-editor picker using native Select dropdown component.
    pub(super) fn editor_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current = self.prefs.editor.clone();
        if self.editor_select_key.as_ref() != Some(&current) {
            self.editor_select_key = Some(current.clone());
            let choices: Vec<String> = crate::sys::EDITOR_LADDER.iter().map(|e| e.label().to_string()).collect();
            let current_label = current.label();
            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.editor_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.editor_select_state)
            .w_full()
            .min_w(px(180.0))
            .placeholder("Select code editor...")
            .into_any_element()
    }

    /// The nucleus index budget picker using native Select dropdown component.
    pub(super) fn nucleus_budget_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_kb = nucleus_budget_kb_for(&self.team);
        if self.nucleus_budget_select_key != Some(current_kb) {
            self.nucleus_budget_select_key = Some(current_kb);
            let choices: Vec<String> = NUCLEUS_BUDGET_LADDER_KB.iter().map(|kb| format!("{kb} KiB")).collect();
            let current_label = format!("{current_kb} KiB");
            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.nucleus_budget_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.nucleus_budget_select_state)
            .w_full()
            .min_w(px(180.0))
            .placeholder("Select budget...")
            .into_any_element()
    }

    /// The merge strategy picker using native Select dropdown component.
    pub(super) fn merge_strategy_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_strategy = self.team.merge_strategy();
        if self.merge_strategy_select_key != Some(current_strategy) {
            self.merge_strategy_select_key = Some(current_strategy);
            let choices: Vec<String> = vec![
                "Fast-forward".to_string(),
                "Squash commit".to_string(),
                "GitHub PR mirror".to_string(),
            ];
            let current_label = match current_strategy {
                hadron_lattice::MergeStrategy::FastForward => "Fast-forward",
                hadron_lattice::MergeStrategy::Squash => "Squash commit",
                hadron_lattice::MergeStrategy::GitHubPr => "GitHub PR mirror",
            }.to_string();
            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.merge_strategy_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.merge_strategy_select_state)
            .w_full()
            .min_w(px(180.0))
            .placeholder("Select strategy...")
            .into_any_element()
    }

    /// The color theme preset picker supporting both built-in presets and custom themes.
    pub(super) fn theme_preset_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_pref = self.prefs.theme_preset;
        let mut choices: Vec<String> = config::ThemePreset::ALL.iter().map(|p| p.label().to_string()).collect();
        for custom in theme::load_custom_themes() {
            let label = format!("Custom: {}", custom.name);
            if !choices.contains(&label) {
                choices.push(label);
            }
        }
        let current_label = if let Some(custom) = &self.prefs.custom_theme {
            format!("Custom: {}", custom.name)
        } else {
            current_pref.unwrap_or_default().label().to_string()
        };

        if self.theme_preset_select_key != Some(current_pref) || self.prefs.custom_theme.is_some() {
            self.theme_preset_select_key = Some(current_pref);
            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.theme_preset_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.theme_preset_select_state)
            .w_full()
            .min_w(px(180.0))
            .placeholder("Select theme preset...")
            .into_any_element()
    }

    /// The primary accent color picker.
    pub(super) fn accent_choice_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_pref = self.prefs.accent_choice;
        if self.accent_choice_select_key != Some(current_pref) {
            self.accent_choice_select_key = Some(current_pref);
            let choices: Vec<String> = config::AccentChoice::ALL.iter().map(|a| a.label().to_string()).collect();
            let current_accent = current_pref.unwrap_or_default();
            let current_label = current_accent.label().to_string();
            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.accent_choice_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.accent_choice_select_state)
            .w_full()
            .min_w(px(180.0))
            .placeholder("Select accent color...")
            .into_any_element()
    }

    /// The UI font picker listing bundled fonts first, then bold-verified system fonts.
    pub(super) fn ui_font_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_pref = self.prefs.ui_font_family.clone();
        if self.ui_font_select_key != Some(current_pref.clone()) {
            self.ui_font_select_key = Some(current_pref.clone());
            let mut choices = vec![
                "Inter (Default)".to_string(),
                "Geist".to_string(),
                "Noto Sans".to_string(),
            ];
            let text_system = cx.text_system();
            let mut sys_names = text_system.all_font_names();
            sys_names.sort();
            sys_names.dedup();
            for name in sys_names {
                if crate::fonts::BUNDLED_UI_FAMILIES.contains(&name.as_str())
                    || name == ".SystemUIFont"
                    || crate::fonts::is_emoji_or_symbol_font(&name)
                {
                    continue;
                }
                let regular = gpui::font(&name);
                if text_system.resolve_font(&regular.clone().bold()) != text_system.resolve_font(&regular) {
                    choices.push(name);
                }
            }
            let current_label = match &current_pref {
                Some(name) => {
                    if name == "Inter" {
                        "Inter (Default)".to_string()
                    } else {
                        name.clone()
                    }
                }
                None => "Inter (Default)".to_string(),
            };
            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.ui_font_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.ui_font_select_state)
            .w_full()
            .min_w(px(180.0))
            .placeholder("Select UI font...")
            .into_any_element()
    }

    /// The UI font size picker (12px - 20px, default 14px).
    pub(super) fn ui_font_size_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_pref = self.prefs.ui_font_size;
        if self.ui_font_size_select_key != Some(current_pref) {
            self.ui_font_size_select_key = Some(current_pref);
            let choices = vec![
                "12px".to_string(),
                "13px".to_string(),
                "14px (Default)".to_string(),
                "15px".to_string(),
                "16px".to_string(),
                "18px".to_string(),
                "20px".to_string(),
            ];
            let current_label = match current_pref {
                Some(sz) => {
                    if (sz - 14.0).abs() < 0.01 {
                        "14px (Default)".to_string()
                    } else {
                        format!("{sz}px")
                    }
                }
                None => "14px (Default)".to_string(),
            };
            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.ui_font_size_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.ui_font_size_select_state)
            .w_full()
            .min_w(px(140.0))
            .placeholder("Select UI font size...")
            .into_any_element()
    }

    /// The code / monospace font picker listing bundled fonts first, then bold-verified system fonts.
    pub(super) fn mono_font_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_pref = self.prefs.mono_font_family.clone();
        if self.mono_font_select_key != Some(current_pref.clone()) {
            self.mono_font_select_key = Some(current_pref.clone());
            let mut choices = vec![
                "Cascadia Code (Default)".to_string(),
                "JetBrains Mono".to_string(),
                "Fira Code".to_string(),
            ];
            let text_system = cx.text_system();
            let mut sys_names = text_system.all_font_names();
            sys_names.sort();
            sys_names.dedup();
            for name in sys_names {
                if crate::fonts::BUNDLED_MONO_FAMILIES.contains(&name.as_str())
                    || name == ".SystemUIFont"
                    || crate::fonts::is_emoji_or_symbol_font(&name)
                {
                    continue;
                }
                let regular = gpui::font(&name);
                if text_system.resolve_font(&regular.clone().bold()) != text_system.resolve_font(&regular) {
                    choices.push(name);
                }
            }
            let current_label = match &current_pref {
                Some(name) => {
                    if name == "Cascadia Code" {
                        "Cascadia Code (Default)".to_string()
                    } else {
                        name.clone()
                    }
                }
                None => "Cascadia Code (Default)".to_string(),
            };

            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.mono_font_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.mono_font_select_state)
            .w_full()
            .min_w(px(180.0))
            .placeholder("Select code font...")
            .into_any_element()
    }

    /// The code / monospace font size picker (10px - 18px, default 13px).
    pub(super) fn mono_font_size_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_pref = self.prefs.mono_font_size;
        if self.mono_font_size_select_key != Some(current_pref) {
            self.mono_font_size_select_key = Some(current_pref);
            let choices = vec![
                "10px".to_string(),
                "11px".to_string(),
                "12px".to_string(),
                "13px (Default)".to_string(),
                "14px".to_string(),
                "15px".to_string(),
                "16px".to_string(),
                "18px".to_string(),
            ];
            let current_label = match current_pref {
                Some(sz) => {
                    if (sz - 13.0).abs() < 0.01 {
                        "13px (Default)".to_string()
                    } else {
                        format!("{sz}px")
                    }
                }
                None => "13px (Default)".to_string(),
            };
            let delegate = create_model_delegate(&current_label, &choices, Some(&current_label));
            self.mono_font_size_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                s.set_selected_value(&current_label.into(), window, cx);
            });
        }
        Select::new(&self.mono_font_size_select_state)
            .w_full()
            .min_w(px(140.0))
            .placeholder("Select code font size...")
            .into_any_element()
    }

    pub(super) fn providers_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.wizard_state {
            WizardState::None => {
                let mut list = v_flex().gap_3();
                for provider in &self.providers {
                    let model_text = if provider.model.trim().is_empty() {
                        "—".to_string()
                    } else {
                        provider.model.clone()
                    };
                    list = list.child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(theme::border())
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_base()
                                            .text_color(theme::text())
                                            .child(provider.id.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(format!("Transport: {}", provider.transport)),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::text_secondary())
                                            .child(format!("Model: {}", model_text)),
                                    )
                                    .child({
                                        let pid = provider.id.clone();
                                        text_button(
                                            format!("remove-{}", provider.id),
                                            "Remove",
                                        )
                                        .on_click(cx.listener(
                                            move |this, _, _window, cx| {
                                                this.remove_quark(&pid, cx);
                                            },
                                        ))
                                    }),
                            ),
                    );
                }

                v_flex()
                    .size_full()
                    .gap_6()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(theme::text())
                                    .child("Configured Providers"),
                            )
                            .child(text_button("add-quark", "Add Quark").on_click(cx.listener(
                                |this, _, _window, cx| {
                                    this.wizard_state = WizardState::PickPreset;
                                    cx.notify();
                                },
                            ))),
                    )
                    // Scroll the roster so a long provider list stays reachable while the
                    // "Configured Providers" header + Add Quark button stay pinned. Same
                    // reason as the preset list: a `size_full` wizard can't be scrolled by
                    // the ancestor, so extra rows would clip off the card.
                    .child(
                        div()
                            .id("providers-list-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(list),
                    )
            }
            WizardState::PickPreset => {
                // The merged catalogue: compiled presets + the published ACP registry,
                // so "seat an agent" can offer any registry agent with no CLI install —
                // not just the ones we've hand-written a preset for. `available_presets`
                // only ever saw the compiled list; extending that one view rather than
                // keeping a second is the point (see `available_agents`'s doc comment).
                let entries = hadron_gluon::adapter::registry::QuarkKind::available_agents();

                // Case-insensitive substring match on name + command. Empty filter shows
                // all. (A custom provider is added via the "Custom CLI…" option, which
                // has its own working wizard — the old empty-command "Custom command…"
                // escape hatch was a dead end and has been removed.)
                let filter = self.preset_filter.read(cx).value().trim().to_lowercase();
                let entries: Vec<_> = entries
                    .into_iter()
                    .filter(|e| {
                        let command_line = e
                            .command
                            .as_ref()
                            .map(|(program, args)| format!("{program} {}", args.join(" ")))
                            .unwrap_or_default();
                        filter.is_empty()
                            || e.name.to_lowercase().contains(&filter)
                            || command_line.to_lowercase().contains(&filter)
                    })
                    .collect();

                let mut list = v_flex().gap_2();

                // Ollama / LM Studio (keyless local HTTP servers) and the cloud
                // OpenAI-compatible endpoint (keyed), pinned at the top since none of the
                // three need an install check or a boot-and-probe — just a Connect button
                // against a server that's either reachable or isn't.
                for vendor in [
                    hadron_gluon::adapter::local::HttpVendor::Ollama,
                    hadron_gluon::adapter::local::HttpVendor::LmStudio,
                    hadron_gluon::adapter::local::HttpVendor::OpenAiCompatible,
                ] {
                    if !filter.is_empty() && !vendor.display_name().to_lowercase().contains(&filter) {
                        continue;
                    }
                    list = list.child(
                        h_flex()
                            .id(SharedString::from(format!("preset-local-{}", vendor.code())))
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(theme::border())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                            .cursor_pointer()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_base()
                                                    .text_color(theme::text())
                                                    .child(vendor.display_name()),
                                            )
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_full()
                                                    .bg(theme::tab_bar_bg())
                                                    .border_1()
                                                    .border_color(theme::glass_highlight())
                                                    .text_xs()
                                                    .text_color(theme::accent())
                                                    .child("Local HTTP"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(format!("Connect over HTTP \u{2014} {}", vendor.default_base_url())),
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::accent())
                                    .child("1-Click Add \u{2192}"),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.local_base_url.update(cx, |s, cx| {
                                    s.set_value(vendor.default_base_url(), window, cx)
                                });
                                // A stale key from a previous Cloud attempt must not leak
                                // into a fresh Ollama/LM Studio row (or into a different
                                // cloud key the human is about to type).
                                this.local_api_key.update(cx, |s, cx| s.set_value("", window, cx));
                                this.local_models = Vec::new();
                                this.local_selected_model = None;
                                if !vendor.requires_api_key() {
                                    this.start_local_provider_probe(vendor, vendor.default_base_url().to_string(), None, cx);
                                } else {
                                    this.wizard_state =
                                        WizardState::LocalProvider(vendor, ProviderState::NotConnected);
                                    cx.notify();
                                }
                            })),
                    );
                }

                for entry in entries {
                    let command_line = entry
                        .command
                        .as_ref()
                        .map(|(program, args)| format!("{program} {}", args.join(" ")))
                        .unwrap_or_default();
                    // A human blurb for first-class agents; the raw command line for
                    // best-effort presets and registry rows with none, so they're still
                    // identifiable.
                    let subtitle = if entry.description.is_empty() {
                        command_line
                    } else {
                        entry.description.clone()
                    };

                    let row = h_flex()
                        .id(SharedString::from(format!("preset-{}", entry.vendor)))
                        .items_center()
                        .justify_between()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(theme::input_bg())
                        .border_1()
                        .border_color(theme::border())
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_base()
                                                .text_color(theme::text())
                                                .child(entry.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_full()
                                                .bg(theme::tab_bar_bg())
                                                .border_1()
                                                .border_color(theme::glass_highlight())
                                                .text_xs()
                                                .text_color(theme::accent_secondary())
                                                .child("Resident ACP"),
                                        ),
                                )
                                .child(div().text_xs().text_color(theme::text_muted()).child(subtitle)),
                        );

                    // A registry `binary` entry has no resolvable command — Hadron does
                    // not download and execute a third-party archive, so this row is a
                    // real agent worth listing but stays greyed and unclickable rather
                    // than offering a command that cannot work.
                    let row = match entry.command {
                        Some((program, args)) => {
                            let preset = AgentDescriptor {
                                id: entry.vendor.clone(),
                                name: entry.name.clone(),
                                description: entry.description.clone(),
                                command: program,
                                args,
                            };
                            row.hover(|s| s.bg(theme::bg_surface_raised()))
                                .cursor_pointer()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::text_muted())
                                        .child("Connect →"),
                                )
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.start_acp_preset_probe(preset.clone(), cx);
                                }))
                        }
                        // No command we can synthesise — a `binary`-only registry row
                        // (we do not download and execute a third-party archive) or an
                        // agent with no preset. This used to be greyed and unclickable,
                        // which told the human a command was needed and offered nowhere
                        // to put one. It now opens the SAME `Connecting` form with empty
                        // program/args inputs, so the manual command has somewhere to go
                        // and is booted and probed exactly like a preset's.
                        None => {
                            let preset = AgentDescriptor {
                                id: entry.vendor.clone(),
                                name: entry.name.clone(),
                                description: entry.description.clone(),
                                command: String::new(),
                                args: Vec::new(),
                            };
                            row.hover(|s| s.bg(theme::bg_surface_raised()))
                                .cursor_pointer()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::text_muted())
                                        .child("Set command \u{2192}"),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.acp_program.update(cx, |s, cx| s.set_value("", window, cx));
                                    this.acp_args.update(cx, |s, cx| s.set_value("", window, cx));
                                    this.wizard_state = WizardState::Connecting(
                                        preset.clone(),
                                        ProviderState::NotConnected,
                                    );
                                    cx.notify();
                                }))
                        }
                    };
                    list = list.child(row);
                }


                // Custom CLI: a generic `Transport::Cli` seat for a vendor with no ACP
                // agent (or none the human wants to probe right now) — a raw
                // program+args+prompt-channel, not a boot-and-probe like the presets
                // above. See `WizardState::CustomCli` / `cli_seat_from`.
                list = list.child(
                    h_flex()
                        .id("preset-custom-cli")
                        .items_center()
                        .justify_between()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(theme::input_bg())
                        .border_1()
                        .border_color(theme::border())
                        .hover(|s| s.bg(theme::bg_surface_raised()))
                        .cursor_pointer()
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_base()
                                        .text_color(theme::text())
                                        .child("Custom CLI…"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child("A one-shot CLI Hadron has no built-in preset for"),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Configure →"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            // Fresh form every time the wizard is (re-)entered — a stale
                            // vendor/program from a prior custom-CLI attempt must not leak
                            // into this one.
                            this.custom_cli_vendor.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_program.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_args.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_model.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_flag.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_channel = CliChannelChoice::Stdin;
                            this.wizard_state = WizardState::CustomCli;
                            cx.notify();
                        })),
                );

                v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-wizard", "← Back").on_click(cx.listener(
                        |this, _, _window, cx| {
                            this.wizard_state = WizardState::None;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child("Select a Preset"),
                    )
                    // Search box: filters the catalogue as you type (name + command), so
                    // the right provider is one query away instead of a scroll through ~37.
                    .child(Input::new(&self.preset_filter))
                    // The catalogue is ~37 presets — taller than the card. Give the list
                    // its own bounded scroll region (like `settings-nav-scroll`) so every
                    // provider is reachable while Back/title stay pinned. Without this the
                    // `size_full` wizard reports full height to the ancestor scroll, which
                    // then can't scroll, and everything past the first few presets clips.
                    .child(
                        div()
                            .id("preset-list-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(list),
                    )
            }

            WizardState::Connecting(desc, state) => {
                // A catalogue row with no boot command (a `binary`-only registry entry, or
                // an agent nobody wrote a preset for) arrives here with `command` empty and
                // the human types one. Resolving it HERE — before the probe, the auth retry
                // and the save all read `desc` — is what keeps those three in step: the
                // command that gets probed is the command that gets saved.
                let needs_command = desc.command.trim().is_empty();
                let desc = if needs_command {
                    let program = self.acp_program.read(cx).value().trim().to_string();
                    let args = self
                        .acp_args
                        .read(cx)
                        .value()
                        .split_whitespace()
                        .map(str::to_string)
                        .collect::<Vec<_>>();
                    &AgentDescriptor { command: program, args, ..desc.clone() }
                } else {
                    desc
                };
                // Nothing to boot until a program is typed, so Connect stays inert rather
                // than spawning `""` and reporting a bare ENOENT that names no path.
                let command_ready = !desc.command.trim().is_empty();
                let command_form = needs_command.then(|| {
                    v_flex()
                        .gap_4()
                        .child(settings_field_stacked(
                            "Program (the binary to spawn)",
                            Input::new(&self.acp_program).into_any_element(),
                        ))
                        .child(settings_field_stacked(
                            "Args (space-separated)",
                            Input::new(&self.acp_args).into_any_element(),
                        ))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child(
                                    "Hadron has no built-in command for this agent. Give it the \
                                     one its own docs use to speak ACP over stdio \u{2014} that is \
                                     also how you seat a local model: point an ACP agent \
                                     (opencode, goose, qwen-code\u{2026}) at Ollama or LM Studio in \
                                     its own config, then name it here.",
                                ),
                        )
                });
                let desc_clone = desc.clone();
                let state_ui = match state {
                    ProviderState::Connecting => v_flex()
                        .gap_4()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Connecting..."),
                        )
                        .into_any_element(),
                    ProviderState::NotConnected => {
                        v_flex()
                            .gap_4()
                            .children(command_form)
                            .child(text_button("connect-btn", "Connect").when(command_ready, |b| b.on_click(cx.listener(
                                move |this, _, _window, cx| {
                                    this.start_acp_preset_probe(desc_clone.clone(), cx);
                                },
                            ))))
                            .into_any_element()
                    }
                    ProviderState::NeedsAuth(methods) => {
                        let mut auth_list = v_flex().gap_2();
                        for method in methods {
                            let desc_inner = desc.clone();
                            auth_list = auth_list.child(
                                v_flex()
                                    .gap_2()
                                    .p_3()
                                    .border_1()
                                    .border_color(theme::border())
                                    .rounded_md()
                                    .child(
                                        div().text_color(theme::text()).child(method.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::text_muted())
                                            .child(method.description.clone()),
                                    )
                                    .child(
                                        text_button(
                                            &format!("auth-btn-{}", method.id),
                                            &method.name,
                                        )
                                        .on_click(cx.listener(
                                            move |this, _, _, cx| {
                                                this.start_acp_preset_probe(desc_inner.clone(), cx);
                                            },
                                        )),
                                    ),
                            );
                        }
                        auth_list.into_any_element()
                    }
                    ProviderState::Ready { model } => {
                        let desc_inner = desc.clone();
                        let model_inner = model.clone();
                        v_flex()
                            .gap_4()
                            .child(
                                div()
                                    .text_color(theme::accent())
                                    .child(format!("Ready! Model available: {}", model)),
                            )
                            .child(text_button("save-provider", "Save Provider").on_click(
                                cx.listener(move |this, _, _window, cx| {
                                    this.save_and_add_acp_quark(&desc_inner, &model_inner, cx);
                                }),
                            ))
                            .into_any_element()
                    }
                    ProviderState::Failed(err) => div()
                        .text_color(theme::text_secondary())
                        .child(err.clone())
                        .into_any_element(),
                };

                v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-presets", "← Back").on_click(cx.listener(
                        |this, _, _window, cx| {
                            this.wizard_state = WizardState::PickPreset;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child(format!("Connecting to {}", desc.name)),
                    )
                    // Scroll the connection form (config fields / auth / errors can run tall)
                    // while Back + title stay pinned — the ancestor can't scroll a `size_full`
                    // wizard, so a tall form would otherwise clip off the card's bottom.
                    .child(
                        div()
                            .id("connecting-form-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(state_ui),
                    )
            }

            WizardState::CustomCli => {
                // No boot-and-probe here (unlike `Connecting`) — a custom CLI is saved
                // straight from its fields via `cli_seat_from`, the same way the ACP
                // path saves straight from a `ProviderState::Ready` probe result.
                let stdin_selected = self.custom_cli_channel == CliChannelChoice::Stdin;
                let channel_toggle = h_flex()
                    .gap_2()
                    .child(
                        div()
                            .id("cli-channel-stdin")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .when(stdin_selected, |d| {
                                d.bg(theme::glass_card())
                                    .border_1()
                                    .border_color(theme::accent())
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::accent())
                            })
                            .when(!stdin_selected, |d| {
                                d.border_1()
                                    .border_color(theme::border())
                                    .text_color(theme::text_secondary())
                                    .hover(|s| s.bg(theme::bg_surface_raised()))
                            })
                            .child("Stdin")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.custom_cli_channel = CliChannelChoice::Stdin;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("cli-channel-arg")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .when(!stdin_selected, |d| {
                                d.bg(theme::glass_card())
                                    .border_1()
                                    .border_color(theme::accent())
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::accent())
                            })
                            .when(stdin_selected, |d| {
                                d.border_1()
                                    .border_color(theme::border())
                                    .text_color(theme::text_secondary())
                                    .hover(|s| s.bg(theme::bg_surface_raised()))
                            })
                            .child("Argv flag")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.custom_cli_channel = CliChannelChoice::Arg;
                                cx.notify();
                            })),
                    );

                // Required fields, checked live so "Save" is only wired up once the
                // form can actually produce a valid seat (`cli_seat_from` needs both to
                // build `id`/`cli.program`).
                let vendor_text = self.custom_cli_vendor.read(cx).value().trim().to_string();
                let vendor_present = !vendor_text.is_empty();
                let program_present = !self.custom_cli_program.read(cx).value().trim().is_empty();
                // This wizard is the FIRST UI that feeds freely-typed text into a
                // `QuarkId` — which becomes a worktree DIRECTORY name, a git BRANCH ref
                // segment, and a live-file name. A vendor like "foo/bar" would derive
                // an id `validate_quark_id` (the SSOT for what's safe there) rejects, so
                // check it here too and never even wire up Save for a bad one.
                let vendor_id_valid = vendor_present && custom_cli_vendor_is_valid(&vendor_text);
                let can_save = vendor_id_valid && program_present;

                let mut form = v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-custom-cli", "← Back").on_click(cx.listener(
                        |this, _, _window, cx| {
                            this.wizard_state = WizardState::PickPreset;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child("Custom CLI"),
                    )
                    .child(settings_field_stacked(
                        "Vendor (short label, e.g. \"ollama\")",
                        Input::new(&self.custom_cli_vendor).into_any_element(),
                    ))
                    .child(settings_field_stacked(
                        "Program (the binary to spawn)",
                        Input::new(&self.custom_cli_program).into_any_element(),
                    ))
                    .child(settings_field_stacked(
                        "Args (space-separated, optional)",
                        Input::new(&self.custom_cli_args).into_any_element(),
                    ))
                    .child(settings_field_stacked(
                        "Model (optional)",
                        Input::new(&self.custom_cli_model).into_any_element(),
                    ))
                    .child(settings_field_stacked(
                        "Prompt channel",
                        channel_toggle.into_any_element(),
                    ));

                if !stdin_selected {
                    form = form.child(settings_field_stacked(
                        "Flag (blank = positional argument, e.g. \"--prompt\")",
                        Input::new(&self.custom_cli_flag).into_any_element(),
                    ));
                }

                if !can_save {
                    let msg = if vendor_present && !vendor_id_valid {
                        "Vendor may only use letters, digits, '.', '_', and '-' — it becomes \
                         part of a worktree path and a git branch name."
                    } else {
                        "Vendor and program are required."
                    };
                    form = form.child(div().text_sm().text_color(theme::text_muted()).child(msg));
                }

                form.child(
                    text_button("save-custom-cli", "Save Custom CLI").when(can_save, |b| {
                        b.on_click(cx.listener(|this, _, _window, cx| {
                            let vendor =
                                this.custom_cli_vendor.read(cx).value().trim().to_string();
                            let program =
                                this.custom_cli_program.read(cx).value().trim().to_string();
                            if vendor.is_empty() || program.is_empty() || !custom_cli_vendor_is_valid(&vendor) {
                                return; // re-checked: `can_save` already gates this button
                            }
                            let args: Vec<String> = this
                                .custom_cli_args
                                .read(cx)
                                .value()
                                .split_whitespace()
                                .map(str::to_string)
                                .collect();
                            let model =
                                this.custom_cli_model.read(cx).value().trim().to_string();
                            let flag = this.custom_cli_flag.read(cx).value().trim().to_string();
                            let channel = prompt_channel_from(this.custom_cli_channel, &flag);

                            // Pure derivation (unit-tested directly) — the same shape as the
                            // ACP path's inline `Seat { .. }` literal, just for a Cli
                            // transport + a generic `CliSpec` instead of an `AcpCommand`.
                            // `cli_seat_from` already normalizes a stray transport prefix off
                            // `vendor` BEFORE deriving `id` (see its doc comment), so — unlike
                            // the ACP path — there is no separate `seat.normalize_vendor()`
                            // call needed here; doing it after would desync id from vendor.
                            let seat = cli_seat_from(&vendor, &program, args, channel, &model);
                            // Advisory only, never blocking — same as the ACP path.
                            if !hadron_lattice::id_follows_convention(
                                seat.id.as_str(),
                                seat.transport,
                            ) {
                                eprintln!(
                                    "chamber: note — id '{}' does not match the '{}-' convention",
                                    seat.id.as_str(),
                                    seat.transport.code()
                                );
                            }

                            this.providers.push(ConfiguredQuark {
                                id: seat.id.0.clone(),
                                transport: seat.transport.code().to_string(),
                                model: seat.model.clone(),
                            });

                            this.add_configured_quark(seat, cx);

                            this.wizard_state = WizardState::None;
                            cx.notify();
                        }))
                    }),
                )
            }

            WizardState::LocalProvider(vendor, state) => {
                let vendor = *vendor;
                let state_ui = match state {
                    ProviderState::Connecting => v_flex()
                        .gap_4()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Connecting..."),
                        )
                        .into_any_element(),
                    ProviderState::NotConnected => {
                        let mut form = v_flex().gap_4().child(settings_field_stacked(
                            "Base URL",
                            Input::new(&self.local_base_url).into_any_element(),
                        ));
                        if vendor.requires_api_key() {
                            form = form.child(settings_field_stacked(
                                "API Key",
                                Input::new(&self.local_api_key).into_any_element(),
                            ));
                        }
                        form.child(text_button("local-connect-btn", "Connect").on_click(cx.listener(
                            move |this, _, _window, cx| {
                                let typed = this.local_base_url.read(cx).value().trim().to_string();
                                let base_url =
                                    if typed.is_empty() { vendor.default_base_url().to_string() } else { typed };
                                let api_key = this.local_api_key.read(cx).value().trim().to_string();
                                let api_key = (!api_key.is_empty()).then_some(api_key);
                                this.start_local_provider_probe(vendor, base_url, api_key, cx);
                            },
                        )))
                        .into_any_element()
                    }
                    // A keyless local server has no auth flow to complete.
                    ProviderState::NeedsAuth(_) => div().into_any_element(),
                    ProviderState::Ready { model: _ } => {
                        let picker = self.wizard_model_select(window, cx);
                        let typed = self.local_base_url.read(cx).value().trim().to_string();
                        let base_url =
                            if typed.is_empty() { vendor.default_base_url().to_string() } else { typed };
                        v_flex()
                            .gap_4()
                            .child(div().text_color(theme::accent()).child("Connected"))
                            .child(settings_field_stacked("Model", picker))
                            .child(text_button("save-local-provider", "Save Provider").on_click(
                                cx.listener(move |this, _, window, cx| {
                                    let model = this.local_selected_model.clone().unwrap_or_default();
                                    if model.is_empty() {
                                        return;
                                    }
                                    let api_key = this.local_api_key.read(cx).value().trim().to_string();
                                    let api_key = (!api_key.is_empty()).then_some(api_key);
                                    this.local_api_key
                                        .update(cx, |s, cx| s.set_value(String::new(), window, cx));
                                    this.save_and_add_http_quark(vendor, &base_url, &model, api_key, cx);
                                }),
                            ))
                            .into_any_element()
                    }
                    ProviderState::Failed(err) => v_flex()
                        .gap_4()
                        .child(div().text_color(theme::text_secondary()).child(err.clone()))
                        .child(text_button("local-retry", "Try Again").on_click(cx.listener(
                            move |this, _, _window, cx| {
                                this.wizard_state = WizardState::LocalProvider(vendor, ProviderState::NotConnected);
                                cx.notify();
                            },
                        )))
                        .into_any_element(),
                };

                v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-local-provider", "← Back").on_click(cx.listener(
                        |this, _, _window, cx| {
                            this.wizard_state = WizardState::PickPreset;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child(vendor.display_name().to_string()),
                    )
                    .child(
                        div()
                            .id("local-provider-form-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(state_ui),
                    )
            }
        }
    }

    pub(super) fn wizard_model_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selected = self.local_selected_model.clone();
        let key = (selected.clone(), self.local_models.clone());
        if self.wizard_model_select_key.as_ref() != Some(&key) {
            self.wizard_model_select_key = Some(key);
            let delegate = create_model_delegate("Default", &self.local_models, selected.as_deref());
            self.wizard_model_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                if let Some(ref sel) = selected {
                    if !sel.is_empty() {
                        s.set_selected_value(&sel.clone().into(), window, cx);
                    } else {
                        s.set_selected_value(&"".into(), window, cx);
                    }
                } else {
                    s.set_selected_value(&"".into(), window, cx);
                }
            });
        }
        Select::new(&self.wizard_model_select_state)
            .w_full()
            .min_w(px(280.0))
            .placeholder("Select model...")
            .search_placeholder("Search models...")
            .into_any_element()
    }
}

fn swatch_chip(label: &'static str, color: gpui::Rgba) -> impl IntoElement {
    let hex = config::format_rgba_hex(color);
    h_flex()
        .items_center()
        .gap_1p5()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(theme::bg_surface_raised())
        .border_1()
        .border_color(theme::border())
        .child(
            div()
                .size(px(12.0))
                .rounded_sm()
                .bg(color)
                .border_1()
                .border_color(theme::border()),
        )
        .child(div().text_xs().font_weight(gpui::FontWeight::MEDIUM).text_color(theme::text()).child(label))
        .child(div().text_xs().text_color(theme::text_muted()).child(hex))
}
