use super::*;

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
    pub(super) fn general_settings_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_6()
            .child(settings_field(
                "Max exchanges",
                v_flex()
                    .gap_1()
                    .child(Input::new(&self.settings_max_exchanges))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(
                                "Caps quark\u{2194}quark exchanges before the swarm \
                                 stops. Blank or 0 = daemon default.",
                            ),
                    )
                    .into_any_element(),
            ))
            .child(settings_field(
                "Close Gluon on Exit",
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(
                                "Terminate the hadron-gluon daemon when the Chamber window closes.",
                            ),
                    )
                    .child(
                        Switch::new("close-gluon-on-exit")
                            .checked(self.prefs.close_gluon_on_exit)
                            .on_click(cx.listener(|this, checked, _window, cx| {
                                this.prefs.close_gluon_on_exit = *checked;
                                let _ = config::save(&this.prefs);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            ))
    }

    pub(super) fn providers_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.wizard_state {
            WizardState::None => {
                let mut list = v_flex().gap_3();
                for provider in &self.providers {
                    let (state_text, state_color) = match &provider.state {
                        ProviderState::NotConnected => {
                            ("Not Connected".to_string(), theme::text_muted())
                        }
                        ProviderState::Connecting => {
                            ("Connecting…".to_string(), theme::text_muted())
                        }
                        ProviderState::NeedsAuth(_) => ("Needs Auth".to_string(), theme::accent()),
                        ProviderState::Ready { model } => {
                            (format!("Ready ({})", model), gpui::rgb(0x22c55e))
                        }
                        ProviderState::Failed(e) => (format!("Failed: {}", e), theme::danger()),
                    };
                    list = list.child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
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
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .size(px(8.0))
                                                    .rounded_full()
                                                    .bg(state_color),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(theme::text_secondary())
                                                    .child(state_text),
                                            ),
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
                                |this, _, window, cx| {
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
                let presets = hadron_gluon::adapter::registry::QuarkKind::available_presets()
                    .into_iter()
                    .map(|(id, name, description, cmd, args)| AgentDescriptor {
                        id: id.into(),
                        name: name.into(),
                        description: description.into(),
                        command: cmd.into(),
                        args: args.into_iter().map(String::from).collect(),
                    })
                    .collect::<Vec<_>>();

                // Case-insensitive substring match on name + command. Empty filter shows
                // all. (A custom provider is added via the "Custom CLI…" option, which
                // has its own working wizard — the old empty-command "Custom command…"
                // escape hatch was a dead end and has been removed.)
                let filter = self.preset_filter.read(cx).value().trim().to_lowercase();
                let presets: Vec<_> = presets
                    .into_iter()
                    .filter(|p| {
                        filter.is_empty()
                            || p.name.to_lowercase().contains(&filter)
                            || p.command.to_lowercase().contains(&filter)
                    })
                    .collect();

                let mut list = v_flex().gap_2();
                for preset in presets {
                    let preset_clone = preset.clone();
                    list = list.child(
                        h_flex()
                            .id(SharedString::from(format!("preset-{}", preset.id)))
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(theme::bg_surface())
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
                                            .child(preset.name.clone()),
                                    )
                                    .child(div().text_xs().text_color(theme::text_muted()).child(
                                        // A human blurb for first-class agents; the raw
                                        // command line for best-effort presets (which have
                                        // no description) so they're still identifiable.
                                        if preset.description.is_empty() {
                                            format!("{} {}", preset.command, preset.args.join(" "))
                                        } else {
                                            preset.description.clone()
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .child("Configure →"),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.wizard_state = WizardState::Connecting(
                                    preset_clone.clone(),
                                    ProviderState::NotConnected,
                                );
                                cx.notify();
                            })),
                    );
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
                        .bg(theme::bg_surface())
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
                        |this, _, window, cx| {
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
                            .child(text_button("connect-btn", "Connect").on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.wizard_state = WizardState::Connecting(
                                        desc_clone.clone(),
                                        ProviderState::Connecting,
                                    );
                                    cx.notify();

                                    // Connect = boot the agent and complete ACP's `initialize`.
                                    // The probe lives in the daemon (`hadron-gluon`), which is the
                                    // thing that will actually drive this agent — so the UI cannot
                                    // claim a provider works over a client the daemon never uses.
                                    let target = hadron_gluon::adapter::registry::AcpTarget {
                                        program: desc_clone.command.clone(),
                                        args: desc_clone.args.clone(),
                                        env: Vec::new(),
                                    };
                                    let desc_for_task = desc_clone.clone();
                                    cx.spawn(
                                        |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                                            // The async block outlives the borrow, so it gets its own
                                            // handle on the app rather than holding a reference.
                                            let mut cx = cx.clone();
                                            async move {
                                                // Blocking boot, off the UI thread: a slow `npx` must not
                                                // freeze the window.
                                                let result = cx
                                                    .background_spawn(async move {
                                                        hadron_gluon::adapter::acp::probe(&target)
                                                    })
                                                    .await
                                                    .map_err(|e| e.to_string());

                                                this.update(&mut cx, |this, cx| {
                                                    let state = match result {
                                                        Ok(model) => ProviderState::Ready { model },
                                                        Err(e) => ProviderState::Failed(e),
                                                    };
                                                    this.wizard_state = WizardState::Connecting(
                                                        desc_for_task,
                                                        state,
                                                    );
                                                    cx.notify();
                                                })
                                                .ok();
                                            }
                                        },
                                    )
                                    .detach();
                                },
                            )))
                            .into_any_element()
                    }
                    ProviderState::NeedsAuth(methods) => {
                        let mut auth_list = v_flex().gap_2();
                        for method in methods {
                            let method_clone = method.clone();
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
                                                this.wizard_state = WizardState::Connecting(
                                                    desc_inner.clone(),
                                                    ProviderState::Connecting,
                                                );
                                                cx.notify();

                                                let target = hadron_gluon::adapter::registry::AcpTarget {
                                                    program: desc_inner.command.clone(),
                                                    args: desc_inner.args.clone(),
                                                    env: Vec::new(),
                                                };
                                                let desc_for_task = desc_inner.clone();
                                                cx.spawn(
                                                    |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                                                        let mut cx = cx.clone();
                                                        async move {
                                                            let result = cx
                                                                .background_spawn(async move {
                                                                    hadron_gluon::adapter::acp::probe(&target)
                                                                })
                                                                .await
                                                                .map_err(|e| e.to_string());

                                                            this.update(&mut cx, |this, cx| {
                                                                let state = match result {
                                                                    Ok(model) => ProviderState::Ready { model },
                                                                    Err(e) => ProviderState::Failed(e),
                                                                };
                                                                this.wizard_state = WizardState::Connecting(
                                                                    desc_for_task,
                                                                    state,
                                                                );
                                                                cx.notify();
                                                            })
                                                            .ok();
                                                        }
                                                    },
                                                )
                                                .detach();
                                            },
                                        )),
                                    ),
                            );
                        }
                        auth_list.into_any_element()
                    }
                    ProviderState::Ready { model } => {
                        let desc_inner = desc.clone();
                        let state_inner = state.clone();
                        let model_inner = model.clone();
                        v_flex()
                            .gap_4()
                            .child(
                                div()
                                    .text_color(theme::accent())
                                    .child(format!("Ready! Model available: {}", model)),
                            )
                            .child(text_button("save-provider", "Save Provider").on_click(
                                cx.listener(move |this, _, window, cx| {
                                    // `desc_inner.id` is the PURE vendor now (Task 3 re-keyed
                                    // `available_presets()`/`AgentDescriptor` off `AcpAgentSpec.vendor`,
                                    // e.g. "claude" — it no longer carries the old smeared "acp-claude"
                                    // preset key). The seat's id is the `<transport>-<vendor>` form,
                                    // derived once via `conventional_id` and reused for BOTH records
                                    // below so they never diverge — `remove_quark` keys the roster off
                                    // `ConfiguredQuark.id`, so it must match `Seat.id` exactly.
                                    let base_id = hadron_lattice::Transport::Acp.conventional_id(&desc_inner.id);
                                    // Saving the same provider again must create a SECOND seat
                                    // (same vendor, its own model/identity), not re-adopt the
                                    // first — so mint a fresh id when the conventional one is
                                    // taken anywhere this chamber can see a seat.
                                    let seat_id = {
                                        let taken = |id: &str| {
                                            this.providers.iter().any(|p| p.id == id)
                                                || this.global.quarks.iter().any(|s| s.id.as_str() == id)
                                                || this.team.quarks.iter().any(|s| s.id.as_str() == id)
                                                || this.team.roster.iter().any(|o| o.id.as_str() == id)
                                        };
                                        unique_seat_id(&base_id, &taken)
                                    };

                                    this.providers.push(ConfiguredQuark {
                                        id: seat_id.clone(),
                                        transport: "acp".to_string(),
                                        state: state_inner.clone(),
                                    });

                                    // An ACP seat, and it carries the command the wizard
                                    // just proved boots — so the daemon reaches this agent
                                    // over the same transport the human tested it on. Its
                                    // definition lands in the global catalogue; this repo
                                    // auto-adopts it (see `add_configured_quark`).
                                    let mut seat = hadron_lattice::Seat {
                                        id: hadron_lattice::QuarkId::new(&seat_id),
                                        display_name: None,
                                        vendor: desc_inner.id.clone(),
                                        model: model_inner.clone(),
                                        flavor: hadron_lattice::Flavor::Worker, // default flavor
                                        transport: hadron_lattice::Transport::Acp,
                                        command: Some(hadron_lattice::AcpCommand {
                                            program: desc_inner.command.clone(),
                                            args: desc_inner.args.clone(),
                                        }),
                                        cli: None,
                                        // A seat the human just proved and saved is on.
                                        enabled: true,
                                        effort: None,
                                        mode_config: None,
                                        roles: vec![],
                                        exclusive: false,
                                        commands: hadron_lattice::SeatCommands::default(),
                                        secret_env: Vec::new(),
                                        energy_limit: None,
                                        deny_skills: vec![],
                                    };
                                    // `vendor` is already pure (Task 3's re-keyed preset list), so this
                                    // is a no-op today — left in as a defensive strip in case a vendor
                                    // string ever carries a transport prefix again.
                                    seat.normalize_vendor();
                                    // Advisory only, never blocking: `seat_id` is already built from
                                    // `conventional_id`, so this is dormant on this common path — it
                                    // stays as future-proofing for a later custom-CLI id path where a
                                    // hand-typed id might not match its transport prefix.
                                    if !hadron_lattice::id_follows_convention(seat.id.as_str(), seat.transport) {
                                        eprintln!(
                                            "chamber: note — id '{}' does not match the '{}-' convention",
                                            seat.id.as_str(),
                                            seat.transport.code()
                                        );
                                    }
                                    this.add_configured_quark(seat, cx);

                                    this.wizard_state = WizardState::None;
                                    cx.notify();
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
                        |this, _, window, cx| {
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
                                d.bg(theme::accent()).text_color(theme::text())
                            })
                            .when(!stdin_selected, |d| {
                                d.text_color(theme::text_secondary())
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
                                d.bg(theme::accent()).text_color(theme::text())
                            })
                            .when(stdin_selected, |d| {
                                d.text_color(theme::text_secondary())
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
                        |this, _, window, cx| {
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
                    .child(settings_field(
                        "Vendor (short label, e.g. \"ollama\")",
                        Input::new(&self.custom_cli_vendor).into_any_element(),
                    ))
                    .child(settings_field(
                        "Program (the binary to spawn)",
                        Input::new(&self.custom_cli_program).into_any_element(),
                    ))
                    .child(settings_field(
                        "Args (space-separated, optional)",
                        Input::new(&self.custom_cli_args).into_any_element(),
                    ))
                    .child(settings_field(
                        "Model (optional)",
                        Input::new(&self.custom_cli_model).into_any_element(),
                    ))
                    .child(settings_field("Prompt channel", channel_toggle.into_any_element()));

                if !stdin_selected {
                    form = form.child(settings_field(
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
                        b.on_click(cx.listener(|this, _, window, cx| {
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
                                state: ProviderState::Ready { model: seat.model.clone() },
                            });

                            this.add_configured_quark(seat, cx);

                            this.wizard_state = WizardState::None;
                            cx.notify();
                        }))
                    }),
                )
            }
        }
    }
}
