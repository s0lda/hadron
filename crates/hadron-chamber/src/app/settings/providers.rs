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

        let target = hadron_gluon::adapter::local::HttpTarget { vendor, base_url, api_key };
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
                    this.wizard_state = WizardState::LocalProvider(
                        vendor,
                        match result {
                            Ok(models) if !models.is_empty() => {
                                let ids: Vec<String> = models.into_iter().map(|m| m.id).collect();
                                let first = ids[0].clone();
                                this.local_models = ids;
                                this.local_selected_model = Some(first.clone());
                                cx.notify();
                                ProviderState::Ready { model: first }
                            }
                            Ok(_) => ProviderState::Failed(format!(
                                "connected, but {} has no models loaded",
                                vendor.display_name()
                            )),
                            Err(e) => ProviderState::Failed(e),
                        },
                    );
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(super) fn general_settings_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_6()
            .child(settings_field(
                "Max exchanges",
                Some(
                    "Caps quark\u{2194}quark exchanges before the swarm stops. \
                     Blank or 0 = daemon default.",
                ),
                Input::new(&self.settings_max_exchanges).w_full().into_any_element(),
            ))
            .child(settings_field(
                "Nucleus index budget",
                Some(
                    "How big .hadron/nucleus/index.md may grow before a quark is shown \
                     counts instead of the index. Bigger buys more shared lessons at the \
                     cost of context every quark pays on every turn.",
                ),
                self.nucleus_budget_ladder(cx),
            ))
            .child(settings_field(
                "Code editor",
                Some(
                    "Which program opens a file you click \u{2014} a file:// link in a message, \
                     or \"Open in editor\" in the file tree. System default hands it to the \
                     desktop (xdg-open), which is how you end up in Vim.",
                ),
                self.editor_ladder(cx),
            ))
            .child(settings_field(
                "Default permission mode",
                Some(
                    "The mode a new session starts on. /clear wipes the field, which is \
                     where the current mode lives \u{2014} without this it always came back \
                     as Ask. Changing it does not touch the session you are in.",
                ),
                self.default_mode_ladder(cx),
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
            ))
    }

    /// The default-permission-mode picker: the full [`MODE_LADDER`], each chip in its
    /// own risk colour, with the selected mode's gloss underneath so the choice is not
    /// just a label. Same direct-write-and-save shape as [`Self::editor_ladder`].
    ///
    /// Writes ONLY the preference — it deliberately appends no `ModeSet`, so choosing a
    /// default never silently re-arms the session the human is already in. `/clear`
    /// seeds it into the fresh field; the global chip (and `F6`) still owns *now*.
    pub(super) fn default_mode_ladder(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current = self.prefs.default_mode;
        let mut row = h_flex().gap_1p5().flex_wrap();
        for mode in widgets::MODE_LADDER {
            let selected = mode == current;
            row = row.child(
                div()
                    .id(SharedString::from(format!("default-mode-{}", widgets::mode_label(mode))))
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .text_xs()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme::glass_card())
                            .border_color(widgets::mode_color(mode))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(widgets::mode_color(mode))
                    })
                    .when(!selected, |d| {
                        d.bg(theme::bg_surface())
                            .border_color(theme::border())
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                    })
                    .child(widgets::mode_label(mode).to_string())
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.prefs.default_mode = mode;
                        let _ = config::save(&this.prefs);
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
                    .child(widgets::mode_hint(current)),
            )
            .into_any_element()
    }

    /// The code-editor picker. Same direct-write chip row as the nucleus budget
    /// ladder below, over [`crate::sys::EDITOR_LADDER`] — and like that one, the UI
    /// offers the ladder while a hand-edited `chamber.json` may carry a
    /// `Custom` command. A `Custom` choice therefore has no chip to highlight; the
    /// row shows none selected rather than silently pretending it is `System`.
    pub(super) fn editor_ladder(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut row = h_flex().gap_1p5().flex_wrap();
        for choice in crate::sys::EDITOR_LADDER {
            let selected = choice == self.prefs.editor;
            let pick = choice.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("editor-{}", choice.label())))
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .text_xs()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme::glass_card())
                            .border_color(theme::accent())
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::accent())
                    })
                    .when(!selected, |d| {
                        d.bg(theme::bg_surface())
                            .border_color(theme::border())
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                    })
                    .child(choice.label().to_string())
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.prefs.editor = pick.clone();
                        let _ = config::save(&this.prefs);
                        cx.notify();
                    })),
            );
        }
        row.into_any_element()
    }

    /// The nucleus index budget picker: a fixed ladder (16/32/64/128 KiB), not a
    /// free-text field — a hand-edited `team.json` may still carry any other
    /// positive value (`Team::nucleus_index_budget_kb` stays a plain `Option<usize>`
    /// for that reason), but the UI only ever offers this ladder. Writes directly on
    /// click and saves, the same direct-write shape as the identity color swatches —
    /// no separate `Entity<InputState>` + commit round-trip needed for four buttons.
    pub(super) fn nucleus_budget_ladder(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current = nucleus_budget_kb_for(&self.team);
        let mut row = h_flex().gap_1p5();
        for kb in NUCLEUS_BUDGET_LADDER_KB {
            let selected = kb == current;
            row = row.child(
                div()
                    .id(SharedString::from(format!("nucleus-budget-{kb}")))
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .text_xs()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme::glass_card())
                            .border_color(theme::accent())
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::accent())
                    })
                    .when(!selected, |d| {
                        d.bg(theme::bg_surface())
                            .border_color(theme::border())
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                    })
                    .child(format!("{kb} KiB"))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.team.nucleus_index_budget_kb = Some(kb);
                        this.save_repo_team(cx);
                    })),
            );
        }
        row.into_any_element()
    }

    pub(super) fn providers_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                                            .child(vendor.display_name()),
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
                                    .text_color(theme::text_muted())
                                    .child("Connect \u{2192}"),
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
                                this.wizard_state =
                                    WizardState::LocalProvider(vendor, ProviderState::NotConnected);
                                cx.notify();
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
                                        .child(entry.name.clone()),
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
                                        .child("Configure →"),
                                )
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.wizard_state = WizardState::Connecting(
                                        preset.clone(),
                                        ProviderState::NotConnected,
                                    );
                                    cx.notify();
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
                                cx.listener(move |this, _, _window, cx| {
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
                                        // A brand-new seat reaches nothing outside its
                                        // worktree until a human grants it a root.
                                        external_roots: vec![],
                                        http_base_url: None,
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
                                state: ProviderState::Ready { model: seat.model.clone() },
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
                                    let base_id = hadron_lattice::Transport::Http.conventional_id(vendor.code());
                                    let seat_id = {
                                        let taken = |id: &str| {
                                            this.providers.iter().any(|p| p.id == id)
                                                || this.global.quarks.iter().any(|s| s.id.as_str() == id)
                                                || this.team.quarks.iter().any(|s| s.id.as_str() == id)
                                                || this.team.roster.iter().any(|o| o.id.as_str() == id)
                                        };
                                        unique_seat_id(&base_id, &taken)
                                    };
                                    let qid = hadron_lattice::QuarkId::new(&seat_id);

                                    // The key itself never reaches `team.json` — write it to the
                                    // keyring first (same store/order `set_settings_secret` uses)
                                    // and only declare the VAR NAME on the seat if that succeeds.
                                    let mut secret_env = Vec::new();
                                    if vendor.requires_api_key() {
                                        let api_key = this.local_api_key.read(cx).value().trim().to_string();
                                        if !api_key.is_empty() {
                                            match this.secret_store.set(&qid, CLOUD_API_KEY_VAR, &api_key) {
                                                Ok(()) => secret_env.push(CLOUD_API_KEY_VAR.to_string()),
                                                Err(e) => eprintln!(
                                                    "chamber: failed to write API key to the OS credential \
                                                     store: {e}"
                                                ),
                                            }
                                        }
                                    }

                                    this.providers.push(ConfiguredQuark {
                                        id: seat_id.clone(),
                                        transport: "http".to_string(),
                                        state: ProviderState::Ready { model: model.clone() },
                                    });

                                    let seat = hadron_lattice::Seat {
                                        id: qid,
                                        display_name: None,
                                        vendor: vendor.code().to_string(),
                                        model,
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
                                        http_base_url: Some(base_url.clone()),
                                    };
                                    this.add_configured_quark(seat, cx);

                                    // Write-only: never re-shown, same as the per-quark secret field.
                                    this.local_api_key
                                        .update(cx, |s, cx| s.set_value(String::new(), window, cx));
                                    this.wizard_state = WizardState::None;
                                    cx.notify();
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
            .placeholder("Select model...")
            .search_placeholder("Search models...")
            .into_any_element()
    }
}
